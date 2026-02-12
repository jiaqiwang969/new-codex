//! Session- and turn-scoped helpers for talking to model provider APIs.
//!
//! `ModelClient` is intended to live for the lifetime of a Codex session and holds the stable
//! configuration and state needed to talk to a provider (auth, provider selection, conversation id,
//! and feature-gated request behavior).
//!
//! Per-turn settings (model selection, reasoning controls, telemetry context, and turn metadata)
//! are passed explicitly to streaming and unary methods so that the turn lifetime is visible at the
//! call site.
//!
//! A [`ModelClientSession`] is created per turn and is used to stream one or more Responses API
//! requests during that turn. It caches a Responses WebSocket connection (opened lazily) and stores
//! per-turn state such as the `x-codex-turn-state` token used for sticky routing.
//!
//! Prewarm is intentionally handshake-only: it may warm a socket and capture sticky-routing
//! state, but the first `response.create` payload is still sent only when a turn starts.
//!
//! Startup prewarm is owned by turn-scoped callers (for example, a pre-created regular task). When
//! a warmed [`ModelClientSession`] is available, turn execution can reuse it; otherwise the turn
//! lazily opens a websocket on first stream call.
//!
//! ## Retry-Budget Tradeoff
//!
//! Startup prewarm is treated as the first websocket connection attempt for the first turn. If
//! it fails, the stream attempt fails and the retry/fallback loop decides whether to retry or fall
//! back. This avoids duplicate handshakes but means a failed prewarm can consume one retry
//! budget slot before any turn payload is sent.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::api_bridge::CoreAuthProvider;
use crate::api_bridge::auth_provider_from_auth;
use crate::api_bridge::map_api_error;
use crate::auth::UnauthorizedRecovery;
use codex_api::CompactClient as ApiCompactClient;
use codex_api::CompactionInput as ApiCompactionInput;
use codex_api::MemoriesClient as ApiMemoriesClient;
use codex_api::MemorySummarizeInput as ApiMemorySummarizeInput;
use codex_api::MemorySummarizeOutput as ApiMemorySummarizeOutput;
use codex_api::RawMemory as ApiRawMemory;
use codex_api::RequestTelemetry;
use codex_api::ReqwestTransport;
use codex_api::ResponseAppendWsRequest;
use codex_api::ResponseCreateWsRequest;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesClient as ApiResponsesClient;
use codex_api::ResponsesOptions as ApiResponsesOptions;
use codex_api::ResponsesWebsocketClient as ApiWebSocketResponsesClient;
use codex_api::ResponsesWebsocketConnection as ApiWebSocketConnection;
use codex_api::SseTelemetry;
use codex_api::TransportError;
use codex_api::WebsocketTelemetry;
use codex_api::build_conversation_headers;
use codex_api::common::Reasoning;
use codex_api::common::ResponsesWsRequest;
use codex_api::create_text_param_for_request;
use codex_api::error::ApiError;
use codex_api::requests::responses::Compression;
use codex_otel::OtelManager;

use crate::turn_metadata::build_turn_metadata_header;
use crate::turn_metadata::resolve_turn_metadata_header_with_timeout;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::config_types::Verbosity as VerbosityConfig;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::SessionSource;
use eventsource_stream::Event;
use eventsource_stream::EventStreamError;
use futures::StreamExt;
use http::HeaderMap as ApiHeaderMap;
use http::HeaderValue;
use http::StatusCode as HttpStatusCode;
use reqwest::StatusCode;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::tungstenite::Message;
use tracing::trace;
use tracing::warn;

use crate::AuthManager;
use crate::auth::CodexAuth;
use crate::auth::RefreshTokenError;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::client_common::tools::ToolSpec;
use crate::default_client::build_reqwest_client;
use crate::error::CodexErr;
use crate::error::Result;
use crate::flags::CODEX_RS_SSE_FIXTURE;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::model_supports_data_url_input_images;
use crate::model_compat::model_supports_input_images;
use crate::model_compat::model_supports_memory_trace_summarize;
use crate::model_compat::model_supports_reasoning_effort;
use crate::model_compat::normalized_grok_model_slug;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::WireApi;
use crate::tools::spec::create_tools_json_for_responses_api;

pub const OPENAI_BETA_HEADER: &str = "OpenAI-Beta";
pub const OPENAI_BETA_RESPONSES_WEBSOCKETS: &str = "responses_websockets=2026-02-04";
pub const X_CODEX_TURN_STATE_HEADER: &str = "x-codex-turn-state";
pub const X_CODEX_TURN_METADATA_HEADER: &str = "x-codex-turn-metadata";
pub const X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER: &str =
    "x-responsesapi-include-timing-metrics";
const RESPONSES_WEBSOCKETS_V2_BETA_HEADER_VALUE: &str = "responses_websockets=2026-02-06";
/// Session-scoped state shared by all [`ModelClient`] clones.
///
/// This is intentionally kept minimal so `ModelClient` does not need to hold a full `Config`. Most
/// configuration is per turn and is passed explicitly to streaming/unary methods.
#[derive(Debug)]
struct ModelClientState {
    auth_manager: Option<Arc<AuthManager>>,
    conversation_id: ThreadId,
    provider: ModelProviderInfo,
    session_source: SessionSource,
    model_verbosity: Option<VerbosityConfig>,
    enable_responses_websockets: bool,
    enable_responses_websockets_v2: bool,
    enable_request_compression: bool,
    include_timing_metrics: bool,
    beta_features_header: Option<String>,
    disable_websockets: AtomicBool,
    preconnect: Mutex<PreconnectState>,
}

/// Resolved API client setup for a single request attempt.
///
/// Keeping this as a single bundle ensures prewarm and normal request paths
/// share the same auth/provider setup flow.
struct CurrentClientSetup {
    auth: Option<CodexAuth>,
    api_provider: codex_api::Provider,
    api_auth: CoreAuthProvider,
}

/// One-shot preconnected websocket slot consumed by the next turn.
///
/// This bundles the socket with optional sticky-routing state captured during
/// handshake so they are taken and cleared atomically.
struct PreconnectedWebSocket {
    _connection: ApiWebSocketConnection,
    _turn_state: Option<String>,
}

/// Session-level lifecycle of startup websocket preconnect.
///
/// `InFlight` tracks the startup task so the first turn can await it and reuse the same socket.
/// `Ready` stores a one-shot warmed socket for turn adoption.

#[allow(dead_code)]
enum PreconnectState {
    /// No startup preconnect task is active and no warmed socket is available.
    Idle,
    /// Startup preconnect is currently running; first turn may await this task.
    InFlight(JoinHandle<()>),
    /// Startup preconnect finished and produced a one-shot warmed socket.
    Ready(PreconnectedWebSocket),
}

impl std::fmt::Debug for PreconnectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::InFlight(_) => write!(f, "InFlight(...)"),
            Self::Ready(_) => write!(f, "Ready(...)"),
        }
    }
}

impl Clone for ModelClientState {
    fn clone(&self) -> Self {
        Self {
            auth_manager: self.auth_manager.clone(),
            conversation_id: self.conversation_id,
            provider: self.provider.clone(),
            session_source: self.session_source.clone(),
            model_verbosity: self.model_verbosity,
            enable_responses_websockets: self.enable_responses_websockets,
            enable_responses_websockets_v2: self.enable_responses_websockets_v2,
            enable_request_compression: self.enable_request_compression,
            include_timing_metrics: self.include_timing_metrics,
            beta_features_header: self.beta_features_header.clone(),
            disable_websockets: AtomicBool::new(
                self.disable_websockets
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            preconnect: Mutex::new(PreconnectState::Idle),
        }
    }
}

/// A session-scoped client for model-provider API calls.
///
/// This holds configuration and state that should be shared across turns within a Codex session
/// (auth, provider selection, conversation id, feature-gated request behavior, and transport
/// fallback state).
///
/// WebSocket fallback is session-scoped: once a turn activates the HTTP fallback, subsequent turns
/// will also use HTTP for the remainder of the session.
///
/// Turn-scoped settings (model selection, reasoning controls, telemetry context, and turn
/// metadata) are passed explicitly to the relevant methods to keep turn lifetime visible at the
/// call site.
#[derive(Debug, Clone)]
pub struct ModelClient {
    state: Arc<ModelClientState>,
}

/// A turn-scoped streaming session created from a [`ModelClient`].
///
/// The session establishes a Responses WebSocket connection lazily and reuses it across multiple
/// requests within the turn. It also caches per-turn state:
///
/// - The last full request, so subsequent calls can use `response.append` only when the current
///   request is an incremental extension of the previous one.
/// - The `x-codex-turn-state` sticky-routing token, which must be replayed for all requests within
///   the same turn.
///
/// Create a fresh `ModelClientSession` for each Codex turn. Reusing it across turns would replay
/// the previous turn's sticky-routing token into the next turn, which violates the client/server
/// contract and can cause routing bugs.
pub struct ModelClientSession {
    client: ModelClient,
    connection: Option<ApiWebSocketConnection>,
    websocket_last_request: Option<ResponsesApiRequest>,
    websocket_last_response_rx: Option<oneshot::Receiver<LastResponse>>,
    /// Turn state for sticky routing.
    ///
    /// This is an `OnceLock` that stores the turn state value received from the server
    /// on turn start via the `x-codex-turn-state` response header. Once set, this value
    /// should be sent back to the server in the `x-codex-turn-state` request header for
    /// all subsequent requests within the same turn to maintain sticky routing.
    ///
    /// This is a contract between the client and server: we receive it at turn start,
    /// keep sending it unchanged between turn requests (e.g., for retries, incremental
    /// appends, or continuation requests), and must not send it between different turns.
    turn_state: Arc<OnceLock<String>>,
}

#[derive(Debug, Clone)]
struct LastResponse {
    response_id: String,
    items_added: Vec<ResponseItem>,
    can_append: bool,
}

enum WebsocketStreamOutcome {
    Stream(ResponseStream),
    FallbackToHttp,
}

impl ModelClient {
    #[allow(clippy::too_many_arguments)]
    /// Creates a new session-scoped `ModelClient`.
    ///
    /// All arguments are expected to be stable for the lifetime of a Codex session. Per-turn values
    /// are passed to [`ModelClientSession::stream`] (and other turn-scoped methods) explicitly.
    pub fn new(
        auth_manager: Option<Arc<AuthManager>>,
        conversation_id: ThreadId,
        provider: ModelProviderInfo,
        session_source: SessionSource,
        model_verbosity: Option<VerbosityConfig>,
        enable_responses_websockets: bool,
        enable_responses_websockets_v2: bool,
        enable_request_compression: bool,
        include_timing_metrics: bool,
        beta_features_header: Option<String>,
    ) -> Self {
        Self {
            state: Arc::new(ModelClientState {
                auth_manager,
                conversation_id,
                provider,
                session_source,
                model_verbosity,
                enable_responses_websockets,
                enable_responses_websockets_v2,
                enable_request_compression,
                include_timing_metrics,
                beta_features_header,
                disable_websockets: AtomicBool::new(false),
                preconnect: Mutex::new(PreconnectState::Idle),
            }),
        }
    }

    /// Creates a fresh turn-scoped streaming session.
    ///
    /// This constructor does not perform network I/O itself; the session opens a websocket lazily
    /// when the first stream request is issued.
    pub fn new_session(&self) -> ModelClientSession {
        ModelClientSession {
            client: self.clone(),
            connection: None,
            websocket_last_request: None,
            websocket_last_response_rx: None,
            turn_state: Arc::new(OnceLock::new()),
        }
    }

    /// Creates a fresh turn-scoped streaming session for the requested provider.
    ///
    /// When the provider matches the session-scoped provider, this preserves session-level state
    /// such as websocket preconnect and sticky HTTP fallback across turns.
    pub fn new_session_for_provider(&self, provider: &ModelProviderInfo) -> ModelClientSession {
        if &self.state.provider == provider {
            return self.new_session();
        }

        self.new_session_with_provider(provider.clone())
    }

    pub fn session_provider_matches(&self, provider: &ModelProviderInfo) -> bool {
        &self.state.provider == provider
    }

    /// Spawns a best-effort task that warms a websocket for the first turn.
    ///
    /// This call performs only connection setup; it never sends prompt payloads.
    ///
    /// A timeout when computing turn metadata is treated the same as "no metadata" so startup
    /// cannot block indefinitely on optional preconnect context.
    pub fn pre_establish_connection(&self, otel_manager: OtelManager, cwd: PathBuf) {
        if !self.responses_websocket_enabled() || self.disable_websockets() {
            return;
        }

        let model_client = self.clone();
        let handle = tokio::spawn(async move {
            let turn_metadata_header = resolve_turn_metadata_header_with_timeout(
                build_turn_metadata_header(cwd.clone()),
                None,
            )
            .await;
            let _ = model_client
                .preconnect(&otel_manager, turn_metadata_header.as_deref())
                .await;
        });
        self.store_preconnect_task(handle);
    }

    /// Opportunistically pre-establishes a Responses WebSocket connection for this session.
    ///
    /// This method is best-effort: it returns `false` on any setup/connect failure and the caller
    /// should continue normally. A successful preconnect reduces first-turn latency but never sends
    /// an initial prompt; the first `response.create` is still sent only when a turn starts.
    ///
    /// The preconnected slot is single-consumer and single-use: the next `ModelClientSession` may
    /// adopt it once, after which later turns either keep using that same turn-local connection or
    /// create a new one.
    pub async fn preconnect(
        &self,
        otel_manager: &OtelManager,
        turn_metadata_header: Option<&str>,
    ) -> bool {
        if !self.responses_websocket_enabled() || self.disable_websockets() {
            return false;
        }

        let client_setup = match self.current_client_setup().await {
            Ok(client_setup) => client_setup,
            Err(err) => {
                warn!("failed to build websocket preconnect client setup: {err}");
                return false;
            }
        };
        let turn_state = Arc::new(OnceLock::new());

        match self
            .connect_websocket(
                otel_manager,
                client_setup.api_provider,
                client_setup.api_auth,
                Some(Arc::clone(&turn_state)),
                turn_metadata_header,
            )
            .await
        {
            Ok(connection) => {
                self.store_preconnected_websocket(connection, turn_state.get().cloned());
                true
            }
            Err(err) => {
                debug!("websocket preconnect failed: {err}");
                false
            }
        }
    }

    /// Stores the preconnect task handle so the first turn can await it.
    fn store_preconnect_task(&self, handle: JoinHandle<()>) {
        let mut guard = self.state.preconnect.blocking_lock();
        *guard = PreconnectState::InFlight(handle);
    }

    /// Stores a preconnected websocket for the next turn to adopt.
    fn store_preconnected_websocket(
        &self,
        connection: ApiWebSocketConnection,
        turn_state: Option<String>,
    ) {
        let mut guard = self.state.preconnect.blocking_lock();
        *guard = PreconnectState::Ready(PreconnectedWebSocket {
            _connection: connection,
            _turn_state: turn_state,
        });
    }
    /// Creates a fresh turn-scoped streaming session with a provider override.
    ///
    /// Use this when the turn's provider differs from the session-level provider
    /// (e.g. after a `/model` switch between GPT and Gemini families).
    pub fn new_session_with_provider(&self, provider: ModelProviderInfo) -> ModelClientSession {
        let mut state_copy = (*self.state).clone();
        state_copy.provider = provider;
        ModelClientSession {
            client: ModelClient {
                state: Arc::new(state_copy),
            },
            connection: None,
            websocket_last_items: Vec::new(),
            websocket_last_response_id: None,
            websocket_last_response_id_rx: None,
            turn_state: Arc::new(OnceLock::new()),
        }
    }

    /// Compacts the current conversation history using the Compact endpoint.
    ///
    /// This is a unary call (no streaming) that returns a new list of
    /// `ResponseItem`s representing the compacted transcript.
    ///
    /// The model selection and telemetry context are passed explicitly to keep `ModelClient`
    /// session-scoped.
    pub async fn compact_conversation_history(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        otel_manager: &OtelManager,
    ) -> Result<Vec<ResponseItem>> {
        if prompt.input.is_empty() {
            return Ok(Vec::new());
        }
        let client_setup = self.current_client_setup().await?;
        let transport = ReqwestTransport::new(build_reqwest_client());
        let request_telemetry = Self::build_request_telemetry(otel_manager);
        let client =
            ApiCompactClient::new(transport, client_setup.api_provider, client_setup.api_auth)
                .with_telemetry(Some(request_telemetry));

        let instructions = prompt.base_instructions.text.clone();
        let model_slug = canonical_model_slug_for_provider(&self.state.provider, &model_info.slug);
        let payload = ApiCompactionInput {
            model: model_slug.as_ref(),
            input: &prompt.input,
            instructions: &instructions,
        };

        let extra_headers = self.build_subagent_headers();
        client
            .compact_input(&payload, extra_headers)
            .await
            .map_err(map_api_error)
    }

    /// Builds memory summaries for each provided normalized raw memory.
    ///
    /// This is a unary call (no streaming) to `/v1/memories/trace_summarize`.
    ///
    /// The model selection, reasoning effort, and telemetry context are passed explicitly to keep
    /// `ModelClient` session-scoped.
    pub async fn summarize_memories(
        &self,
        raw_memories: Vec<ApiRawMemory>,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
        otel_manager: &OtelManager,
    ) -> Result<Vec<ApiMemorySummarizeOutput>> {
        if raw_memories.is_empty() {
            return Ok(Vec::new());
        }
        if !supports_memory_trace_summarize(&self.state.provider, &model_info.slug) {
            return Err(CodexErr::UnsupportedOperation(
                "Memory trace summarization is not supported by the Grok provider.".to_string(),
            ));
        }

        let client_setup = self.current_client_setup().await?;
        let transport = ReqwestTransport::new(build_reqwest_client());
        let request_telemetry = Self::build_request_telemetry(otel_manager);
        let client =
            ApiMemoriesClient::new(transport, client_setup.api_provider, client_setup.api_auth)
                .with_telemetry(Some(request_telemetry));
        let effort = sanitize_reasoning_effort_for_model(effort, &model_info.slug);

        let payload = ApiMemorySummarizeInput {
            model: model_info.slug.clone(),
            raw_memories,
            reasoning: effort.map(|effort| Reasoning {
                effort: Some(effort),
                summary: None,
            }),
        };

        client
            .summarize_input(&payload, self.build_subagent_headers())
            .await
            .map_err(map_api_error)
    }

    fn build_subagent_headers(&self) -> ApiHeaderMap {
        let mut extra_headers = ApiHeaderMap::new();
        if let SessionSource::SubAgent(sub) = &self.state.session_source {
            let subagent = match sub {
                crate::protocol::SubAgentSource::Review => "review".to_string(),
                crate::protocol::SubAgentSource::Compact => "compact".to_string(),
                crate::protocol::SubAgentSource::ThreadSpawn { .. } => "collab_spawn".to_string(),
                crate::protocol::SubAgentSource::Other(label) => label.clone(),
            };
            if let Ok(val) = HeaderValue::from_str(&subagent) {
                extra_headers.insert("x-openai-subagent", val);
            }
        }
        extra_headers
    }

    /// Builds request telemetry for unary API calls (e.g., Compact endpoint).
    fn build_request_telemetry(otel_manager: &OtelManager) -> Arc<dyn RequestTelemetry> {
        let telemetry = Arc::new(ApiTelemetry::new(otel_manager.clone()));
        let request_telemetry: Arc<dyn RequestTelemetry> = telemetry;
        request_telemetry
    }

    /// Returns whether this session is configured to use Responses-over-WebSocket.
    ///
    /// This combines provider capability and feature gating; both must be true for websocket paths
    /// to be eligible.
    pub fn responses_websocket_enabled(&self, model_info: &ModelInfo) -> bool {
        self.state.provider.supports_websockets
            && (self.state.enable_responses_websockets || model_info.prefer_websockets)
    }

    fn responses_websockets_v2_enabled(&self) -> bool {
        self.state.enable_responses_websockets_v2
    }

    /// Returns whether websocket transport has been permanently disabled for this session.
    ///
    /// Once set by fallback activation, subsequent turns must stay on HTTP transport.
    fn websockets_disabled(&self) -> bool {
        self.state.disable_websockets.load(Ordering::Relaxed)
    }

    /// Returns auth + provider configuration resolved from the current session auth state.
    ///
    /// This centralizes setup used by both prewarm and normal request paths so they stay in
    /// lockstep when auth/provider resolution changes.
    async fn current_client_setup(&self) -> Result<CurrentClientSetup> {
        let auth = match self.state.auth_manager.as_ref() {
            Some(manager) => manager.auth().await,
            None => None,
        };
        let api_provider = self
            .state
            .provider
            .to_api_provider(auth.as_ref().map(CodexAuth::auth_mode))?;
        let api_auth = auth_provider_from_auth(auth.clone(), &self.state.provider)?;
        Ok(CurrentClientSetup {
            auth,
            api_provider,
            api_auth,
        })
    }

    /// Opens a websocket connection using the same header and telemetry wiring as normal turns.
    ///
    /// Both startup prewarm and in-turn `needs_new` reconnects call this path so handshake
    /// behavior remains consistent across both flows.
    async fn connect_websocket(
        &self,
        otel_manager: &OtelManager,
        api_provider: codex_api::Provider,
        api_auth: CoreAuthProvider,
        turn_state: Option<Arc<OnceLock<String>>>,
        turn_metadata_header: Option<&str>,
    ) -> std::result::Result<ApiWebSocketConnection, ApiError> {
        let headers = self.build_websocket_headers(turn_state.as_ref(), turn_metadata_header);
        let websocket_telemetry = ModelClientSession::build_websocket_telemetry(otel_manager);
        ApiWebSocketResponsesClient::new(api_provider, api_auth)
            .connect(
                headers,
                crate::default_client::default_headers(),
                turn_state,
                Some(websocket_telemetry),
            )
            .await
    }

    /// Builds websocket handshake headers for both prewarm and turn-time reconnect.
    ///
    /// Callers should pass the current turn-state lock when available so sticky-routing state is
    /// replayed on reconnect within the same turn.
    fn build_websocket_headers(
        &self,
        turn_state: Option<&Arc<OnceLock<String>>>,
        turn_metadata_header: Option<&str>,
    ) -> ApiHeaderMap {
        let turn_metadata_header = parse_turn_metadata_header(turn_metadata_header);
        let mut headers = build_responses_headers(
            self.state.beta_features_header.as_deref(),
            turn_state,
            turn_metadata_header.as_ref(),
        );
        headers.extend(build_conversation_headers(Some(
            self.state.conversation_id.to_string(),
        )));
        let responses_websockets_beta_header = if self.responses_websockets_v2_enabled() {
            RESPONSES_WEBSOCKETS_V2_BETA_HEADER_VALUE
        } else {
            OPENAI_BETA_RESPONSES_WEBSOCKETS
        };
        headers.insert(
            OPENAI_BETA_HEADER,
            HeaderValue::from_static(responses_websockets_beta_header),
        );
        if self.state.include_timing_metrics {
            headers.insert(
                X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER,
                HeaderValue::from_static("true"),
            );
        }
        headers
    }
}

impl ModelClientSession {
    fn activate_http_fallback(&self, websocket_enabled: bool) -> bool {
        websocket_enabled
            && !self
                .client
                .state
                .disable_websockets
                .swap(true, Ordering::Relaxed)
    }

    fn build_responses_request(
        &self,
        provider: &codex_api::Provider,
        prompt: &Prompt,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
    ) -> Result<ResponsesApiRequest> {
        let instructions = &prompt.base_instructions.text;
        let input = prompt.get_formatted_input();
        let tools = create_tools_json_for_responses_api(&prompt.tools)?;
        let default_reasoning_effort = model_info.default_reasoning_level;
        let effort = sanitize_reasoning_effort_for_model(
            effort.or(default_reasoning_effort),
            &model_info.slug,
        );
        let reasoning =
            build_reasoning_payload(model_info.supports_reasoning_summaries, effort, summary);

        let include = if reasoning.is_some() {
            vec!["reasoning.encrypted_content".to_string()]
        } else {
            Vec::new()
        };
        let verbosity = if model_info.support_verbosity {
            self.client
                .state
                .model_verbosity
                .or(model_info.default_verbosity)
        } else {
            if self.client.state.model_verbosity.is_some() {
                warn!(
                    "model_verbosity is set but ignored as the model does not support verbosity: {}",
                    model_info.slug
                );
            }
            None
        };
        let text = create_text_param_for_request(verbosity, &prompt.output_schema);
        let prompt_cache_key = Some(self.client.state.conversation_id.to_string());
        let model_slug =
            canonical_model_slug_for_provider(&self.client.state.provider, &model_info.slug);
        let request = ResponsesApiRequest {
            model: model_slug.into_owned(),
            instructions: instructions.clone(),
            input,
            tools,
            tool_choice: "auto".to_string(),
            parallel_tool_calls: prompt.parallel_tool_calls,
            reasoning,
            store: provider.is_azure_responses_endpoint(),
            stream: true,
            include,
            prompt_cache_key,
            text,
        };
        debug!(
            model = %request.model,
            reasoning = ?request.reasoning,
            "build_responses_request: outgoing request model and reasoning"
        );
        Ok(request)
    }

    #[allow(clippy::too_many_arguments)]
    /// Builds shared Responses API transport options and request-body options.
    ///
    /// Keeping option construction in one place ensures request-scoped headers are consistent
    /// regardless of transport choice.
    fn build_responses_options(
        &self,
        turn_metadata_header: Option<&str>,
        compression: Compression,
    ) -> ApiResponsesOptions {
        let turn_metadata_header = parse_turn_metadata_header(turn_metadata_header);
        let conversation_id = self.client.state.conversation_id.to_string();

        ApiResponsesOptions {
            conversation_id: Some(conversation_id),
            session_source: Some(self.client.state.session_source.clone()),
            extra_headers: build_responses_headers(
                self.client.state.beta_features_header.as_deref(),
                Some(&self.turn_state),
                turn_metadata_header.as_ref(),
            ),
            compression,
            turn_state: Some(Arc::clone(&self.turn_state)),
        }
    }

    fn get_incremental_items(
        &self,
        request: &ResponsesApiRequest,
        last_response: Option<&LastResponse>,
    ) -> Option<Vec<ResponseItem>> {
        // Checks whether the current request is an incremental append to the previous request.
        // We only append when non-input request fields are unchanged and `input` is a strict
        // extension of the previous known input. Server-returned output items are treated as part
        // of the baseline so we do not resend them.
        let previous_request = self.websocket_last_request.as_ref()?;
        let mut previous_without_input = previous_request.clone();
        previous_without_input.input.clear();
        let mut request_without_input = request.clone();
        request_without_input.input.clear();
        if previous_without_input != request_without_input {
            trace!(
                "incremental request failed, properties didn't match {previous_without_input:?} != {request_without_input:?}"
            );
            return None;
        }

        let mut baseline = previous_request.input.clone();
        if let Some(last_response) = last_response {
            baseline.extend(last_response.items_added.clone());
        }

        let baseline_len = baseline.len();
        if baseline_len > 0
            && request.input.starts_with(&baseline)
            && baseline_len < request.input.len()
        {
            Some(request.input[baseline_len..].to_vec())
        } else {
            trace!("incremental request failed, items didn't match");
            None
        }
    }

    fn get_last_response(&mut self) -> Option<LastResponse> {
        self.websocket_last_response_rx
            .take()
            .and_then(|mut receiver| match receiver.try_recv() {
                Ok(last_response) => Some(last_response),
                Err(TryRecvError::Closed) | Err(TryRecvError::Empty) => None,
            })
    }

    fn prepare_websocket_request(
        &mut self,
        payload: ResponseCreateWsRequest,
        request: &ResponsesApiRequest,
    ) -> ResponsesWsRequest {
        let Some(last_response) = self.get_last_response() else {
            return ResponsesWsRequest::ResponseCreate(payload);
        };
        let responses_websockets_v2_enabled = self.client.responses_websockets_v2_enabled();
        if !responses_websockets_v2_enabled && !last_response.can_append {
            trace!("incremental request failed, can't append");
            return ResponsesWsRequest::ResponseCreate(payload);
        }
        let incremental_items = self.get_incremental_items(request, Some(&last_response));
        if let Some(append_items) = incremental_items {
            if responses_websockets_v2_enabled && !last_response.response_id.is_empty() {
                let payload = ResponseCreateWsRequest {
                    previous_response_id: Some(last_response.response_id),
                    input: append_items,
                    ..payload
                };
                return ResponsesWsRequest::ResponseCreate(payload);
            }

            if !responses_websockets_v2_enabled {
                return ResponsesWsRequest::ResponseAppend(ResponseAppendWsRequest {
                    input: append_items,
                });
            }
        }

        ResponsesWsRequest::ResponseCreate(payload)
    }

    /// Opportunistically warms a websocket for this turn-scoped client session.
    ///
    /// This performs only connection setup; it never sends prompt payloads.
    pub async fn prewarm_websocket(
        &mut self,
        otel_manager: &OtelManager,
        model_info: &ModelInfo,
        turn_metadata_header: Option<&str>,
    ) -> std::result::Result<(), ApiError> {
        if !self.client.responses_websocket_enabled(model_info) || self.client.websockets_disabled()
        {
            return Ok(());
        }
        if self.connection.is_some() {
            return Ok(());
        }

        let client_setup = self.client.current_client_setup().await.map_err(|err| {
            ApiError::Stream(format!(
                "failed to build websocket prewarm client setup: {err}"
            ))
        })?;

        let connection = self
            .client
            .connect_websocket(
                otel_manager,
                client_setup.api_provider,
                client_setup.api_auth,
                Some(Arc::clone(&self.turn_state)),
                turn_metadata_header,
            )
            .await?;
        self.connection = Some(connection);
        Ok(())
    }

    /// Returns a websocket connection for this turn.
    async fn websocket_connection(
        &mut self,
        otel_manager: &OtelManager,
        api_provider: codex_api::Provider,
        api_auth: CoreAuthProvider,
        turn_metadata_header: Option<&str>,
        options: &ApiResponsesOptions,
    ) -> std::result::Result<&ApiWebSocketConnection, ApiError> {
        let needs_new = match self.connection.as_ref() {
            Some(conn) => conn.is_closed().await,
            None => true,
        };

        if needs_new {
            self.websocket_last_request = None;
            self.websocket_last_response_rx = None;
            let turn_state = options
                .turn_state
                .clone()
                .unwrap_or_else(|| Arc::clone(&self.turn_state));
            let new_conn = self
                .client
                .connect_websocket(
                    otel_manager,
                    api_provider,
                    api_auth,
                    Some(turn_state),
                    turn_metadata_header,
                )
                .await?;
            self.connection = Some(new_conn);
        }

        self.connection.as_ref().ok_or(ApiError::Stream(
            "websocket connection is unavailable".to_string(),
        ))
    }

    fn responses_request_compression(&self, auth: Option<&crate::auth::CodexAuth>) -> Compression {
        if self.client.state.enable_request_compression
            && auth.is_some_and(CodexAuth::is_chatgpt_auth)
            && self.client.state.provider.is_openai()
        {
            Compression::Zstd
        } else {
            Compression::None
        }
    }

    /// Streams a turn via the OpenAI Responses API.
    ///
    /// Handles SSE fixtures, reasoning summaries, verbosity, and the
    /// `text` controls used for output schemas.
    #[allow(clippy::too_many_arguments)]
    async fn stream_responses_api(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        otel_manager: &OtelManager,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        turn_metadata_header: Option<&str>,
    ) -> Result<ResponseStream> {
        if let Some(path) = &*CODEX_RS_SSE_FIXTURE {
            warn!(path, "Streaming from fixture");
            let stream = codex_api::stream_from_fixture(
                path,
                self.client.state.provider.stream_idle_timeout(),
            )
            .map_err(map_api_error)?;
            let (stream, _last_request_rx) = map_response_stream(stream, otel_manager.clone());
            return Ok(stream);
        }

        validate_image_input_compat(prompt, &model_info.slug)?;

        let auth_manager = self.client.state.auth_manager.clone();
        let mut auth_recovery = auth_manager
            .as_ref()
            .map(super::auth::AuthManager::unauthorized_recovery);
        loop {
            let client_setup = self.client.current_client_setup().await?;
            let transport = ReqwestTransport::new(build_reqwest_client());
            let (request_telemetry, sse_telemetry) = Self::build_streaming_telemetry(otel_manager);
            let compression = self.responses_request_compression(client_setup.auth.as_ref());
            let options = self.build_responses_options(turn_metadata_header, compression);

            let request = self.build_responses_request(
                &client_setup.api_provider,
                prompt,
                model_info,
                effort,
                summary,
            )?;
            let client = ApiResponsesClient::new(
                transport,
                client_setup.api_provider,
                client_setup.api_auth,
            )
            .with_telemetry(Some(request_telemetry), Some(sse_telemetry));
            let stream_result = client.stream_request(request, options).await;

            match stream_result {
                Ok(stream) => {
                    let (stream, _) = map_response_stream(stream, otel_manager.clone());
                    return Ok(stream);
                }
                Err(ApiError::Transport(
                    unauthorized_transport @ TransportError::Http { status, .. },
                )) if status == StatusCode::UNAUTHORIZED => {
                    handle_unauthorized(unauthorized_transport, &mut auth_recovery).await?;
                    continue;
                }
                Err(err) => return Err(map_api_error(err)),
            }
        }
    }

    /// Streams a turn via the Responses API over WebSocket transport.
    #[allow(clippy::too_many_arguments)]
    async fn stream_responses_websocket(
        &mut self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        otel_manager: &OtelManager,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        turn_metadata_header: Option<&str>,
    ) -> Result<WebsocketStreamOutcome> {
        validate_image_input_compat(prompt, &model_info.slug)?;

        let auth_manager = self.client.state.auth_manager.clone();

        let mut auth_recovery = auth_manager
            .as_ref()
            .map(super::auth::AuthManager::unauthorized_recovery);
        loop {
            let client_setup = self.client.current_client_setup().await?;
            let compression = self.responses_request_compression(client_setup.auth.as_ref());

            let options = self.build_responses_options(turn_metadata_header, compression);
            let request = self.build_responses_request(
                &client_setup.api_provider,
                prompt,
                model_info,
                effort,
                summary,
            )?;
            let ws_payload = ResponseCreateWsRequest::from(&request);

            match self
                .websocket_connection(
                    otel_manager,
                    client_setup.api_provider,
                    client_setup.api_auth,
                    turn_metadata_header,
                    &options,
                )
                .await
            {
                Ok(_) => {}
                Err(ApiError::Transport(TransportError::Http { status, .. }))
                    if status == StatusCode::UPGRADE_REQUIRED =>
                {
                    return Ok(WebsocketStreamOutcome::FallbackToHttp);
                }
                Err(ApiError::Transport(
                    unauthorized_transport @ TransportError::Http { status, .. },
                )) if status == StatusCode::UNAUTHORIZED => {
                    handle_unauthorized(unauthorized_transport, &mut auth_recovery).await?;
                    continue;
                }
                Err(err) => return Err(map_api_error(err)),
            }

            let ws_request = self.prepare_websocket_request(ws_payload, &request);

            let stream_result = self
                .connection
                .as_ref()
                .ok_or_else(|| {
                    map_api_error(ApiError::Stream(
                        "websocket connection is unavailable".to_string(),
                    ))
                })?
                .stream_request(ws_request)
                .await
                .map_err(map_api_error)?;
            self.websocket_last_request = Some(request);
            let (stream, last_request_rx) =
                map_response_stream(stream_result, otel_manager.clone());
            self.websocket_last_response_rx = Some(last_request_rx);

            return Ok(WebsocketStreamOutcome::Stream(stream));
        }
    }

    /// Builds request and SSE telemetry for streaming API calls.
    fn build_streaming_telemetry(
        otel_manager: &OtelManager,
    ) -> (Arc<dyn RequestTelemetry>, Arc<dyn SseTelemetry>) {
        let telemetry = Arc::new(ApiTelemetry::new(otel_manager.clone()));
        let request_telemetry: Arc<dyn RequestTelemetry> = telemetry.clone();
        let sse_telemetry: Arc<dyn SseTelemetry> = telemetry;
        (request_telemetry, sse_telemetry)
    }

    /// Builds telemetry for the Responses API WebSocket transport.
    fn build_websocket_telemetry(otel_manager: &OtelManager) -> Arc<dyn WebsocketTelemetry> {
        let telemetry = Arc::new(ApiTelemetry::new(otel_manager.clone()));
        let websocket_telemetry: Arc<dyn WebsocketTelemetry> = telemetry;
        websocket_telemetry
    }

    #[allow(clippy::too_many_arguments)]
    /// Streams a single model request within the current turn.
    ///
    /// The caller is responsible for passing per-turn settings explicitly (model selection,
    /// reasoning settings, telemetry context, and turn metadata). This method will prefer the
    /// Responses WebSocket transport when enabled and healthy, and will fall back to the HTTP
    /// Responses API transport otherwise.
    pub async fn stream(
        &mut self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        otel_manager: &OtelManager,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        turn_metadata_header: Option<&str>,
    ) -> Result<ResponseStream> {
        let wire_api = self.client.state.provider.wire_api;
        match wire_api {
            WireApi::Responses => {
                let websocket_enabled = self.client.responses_websocket_enabled(model_info)
                    && !self.client.websockets_disabled();

                if websocket_enabled {
                    match self
                        .stream_responses_websocket(
                            prompt,
                            model_info,
                            otel_manager,
                            effort,
                            summary,
                            turn_metadata_header,
                        )
                        .await?
                    {
                        WebsocketStreamOutcome::Stream(stream) => return Ok(stream),
                        WebsocketStreamOutcome::FallbackToHttp => {
                            self.try_switch_fallback_transport(otel_manager, model_info);
                        }
                    }
                }

                self.stream_responses_api(
                    prompt,
                    model_info,
                    otel_manager,
                    effort,
                    summary,
                    turn_metadata_header,
                )
                .await
            }
            WireApi::Gemini => self.stream_gemini(prompt, model_info, effort).await,
        }
    }

    /// Permanently disables WebSockets for this Codex session and resets WebSocket state.
    ///
    /// This is used after exhausting the provider retry budget, to force subsequent requests onto
    /// the HTTP transport.
    ///
    /// Returns `true` if this call activated fallback, or `false` if fallback was already active.
    pub(crate) fn try_switch_fallback_transport(
        &mut self,
        otel_manager: &OtelManager,
        model_info: &ModelInfo,
    ) -> bool {
        let websocket_enabled = self.client.responses_websocket_enabled(model_info);
        let activated = self.activate_http_fallback(websocket_enabled);
        if activated {
            warn!("falling back to HTTP");
            otel_manager.counter(
                "codex.transport.fallback_to_http",
                1,
                &[("from_wire_api", "responses_websocket")],
            );

            self.connection = None;
            self.websocket_last_request = None;
            self.websocket_last_response_rx = None;
        }
        activated
    }

    /// Streams a turn via the Google Gemini `:streamGenerateContent` endpoint.
    async fn stream_gemini(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
    ) -> Result<ResponseStream> {
        use crate::gemini_content::*;
        use crate::gemini_streaming::*;
        use crate::gemini_types::*;

        let provider = &self.client.state.provider;
        let base_url = provider.base_url.as_ref().ok_or_else(|| {
            CodexErr::UnsupportedOperation("Gemini providers must define a base_url".to_string())
        })?;
        let base_url = normalize_gemini_base_url(base_url);

        let api_model = strip_model_suffix(&model_info.slug);

        let url = format!(
            "{}/models/{api_model}:streamGenerateContent?alt=sse",
            base_url.as_ref().trim_end_matches('/'),
        );

        let instructions = prompt.base_instructions.text.clone();
        let formatted_input = prompt.get_formatted_input();
        let contents = build_gemini_contents(&formatted_input, &prompt.reference_images, api_model);
        if contents.is_empty() {
            return Err(CodexErr::UnsupportedOperation(
                "Gemini requests require at least one message".to_string(),
            ));
        }

        let system_instruction = (!instructions.trim().is_empty()).then(|| GeminiContentRequest {
            role: None,
            parts: vec![GeminiPartRequest {
                text: Some(instructions),
                inline_data: None,
                function_call: None,
                function_response: None,
                thought_signature: None,
                compat_thought_signature: None,
            }],
        });

        let tools = build_gemini_tools(&prompt.tools, api_model);
        let has_function_tools = prompt
            .tools
            .iter()
            .any(|tool| matches!(tool, ToolSpec::Function(_)));
        let tool_config = (tools.is_some() && has_function_tools).then(|| GeminiToolConfig {
            function_calling_config: build_gemini_tool_config(
                &prompt.tools,
                &formatted_input,
                api_model,
            ),
        });

        let contents = ensure_active_loop_has_thought_signatures(&contents);

        let reasoning_effort = effort.or(model_info.default_reasoning_level);
        let thinking_config = build_gemini_thinking_config(api_model, reasoning_effort);
        let max_output_tokens = resolve_gemini_max_output_tokens(&model_info.slug);

        let generation_config = Some(GeminiGenerationConfig {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            max_output_tokens,
            thinking_config,
            media_resolution: None,
            response_modalities: if api_model.contains("image") {
                Some(vec![
                    GeminiResponseModality::Text,
                    GeminiResponseModality::Image,
                ])
            } else {
                None
            },
            image_config: if api_model.contains("image") {
                Some(GeminiImageConfig {
                    image_size: prompt.image_size,
                    aspect_ratio: prompt.aspect_ratio,
                })
            } else {
                None
            },
        });

        let safety_settings = Some(default_safety_settings());

        let request = GeminiRequest {
            system_instruction,
            contents,
            tools,
            tool_config,
            generation_config,
            safety_settings,
        };

        if std::env::var("CODEX_DEBUG_GEMINI_REQUEST").is_ok()
            && let Ok(json) = serde_json::to_string_pretty(&request)
        {
            tracing::debug!("DEBUG GEMINI REQUEST:\n{json}");
        }

        let client = crate::default_client::build_reqwest_client();

        let auth = match self.client.state.auth_manager.as_ref() {
            Some(manager) => manager.auth().await,
            None => None,
        };

        // Respect provider-local key rotation (account_pool updates env_key).
        let gemini_api_key = if let Some(env_key) = provider.env_key.as_deref() {
            std::env::var(env_key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    auth.as_ref()
                        .and_then(|auth| auth.api_key_for_env_key(env_key))
                        .map(str::to_string)
                })
        } else {
            crate::auth::read_gemini_api_key_from_env().or_else(|| {
                // Try to read from auth.json (~/.codex/auth.json) which may contain
                // a GEMINI_API_KEY field.
                if let Ok(codex_home) = codex_utils_home_dir::find_codex_home() {
                    crate::auth::read_gemini_api_key_from_auth_json(
                        &codex_home,
                        crate::auth::AuthCredentialsStoreMode::File,
                    )
                } else {
                    None
                }
            })
        };

        let make_request_builder = || {
            let mut req_builder = client.post(&url);
            req_builder = provider.apply_http_headers(req_builder);
            if let Some(api_key) = gemini_api_key.as_deref() {
                req_builder = req_builder.header("x-goog-api-key", api_key);
            }
            req_builder
        };

        const MAX_ATTEMPTS: u64 = 3;
        const INITIAL_DELAY_MS: u64 = 5000;
        const MAX_DELAY_MS: u64 = 30000;

        let mut attempt: u64 = 0;
        let mut current_delay = INITIAL_DELAY_MS;

        let response = loop {
            attempt += 1;
            let result = make_request_builder().json(&request).send().await;

            match result {
                Ok(resp) => break resp,
                Err(err) => {
                    let should_retry = if let Some(status) = err.status() {
                        status == StatusCode::TOO_MANY_REQUESTS
                            || (status.as_u16() >= 500 && status.as_u16() < 600)
                    } else {
                        err.is_connect() || err.is_timeout()
                    };

                    if should_retry && attempt < MAX_ATTEMPTS {
                        let jitter =
                            (current_delay as f64 * 0.3 * (rand::random::<f64>() * 2.0 - 1.0))
                                as u64;
                        let delay_with_jitter = current_delay.saturating_add(jitter);
                        tracing::debug!(
                            "Gemini request attempt {} failed, retrying after {}ms: {}",
                            attempt,
                            delay_with_jitter,
                            err
                        );
                        tokio::time::sleep(Duration::from_millis(delay_with_jitter)).await;
                        current_delay = std::cmp::min(MAX_DELAY_MS, current_delay * 2);
                        continue;
                    }

                    return Err(CodexErr::ResponseStreamFailed(
                        crate::error::ResponseStreamFailed {
                            source: err,
                            request_id: None,
                        },
                    ));
                }
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Graceful degradation for thought_signature validation errors.
            if (status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::BAD_REQUEST)
                && body.contains("missing a `thought_signature`")
            {
                let message = format!(
                    "Gemini backend rejected this request due to a thought_signature \
                     validation error.\n\nUpstream error:\n{}",
                    body.chars().take(2000).collect::<String>()
                );
                let item = ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![codex_protocol::models::ContentItem::OutputText {
                        text: message,
                    }],
                    end_turn: None,
                    phase: None,
                    thought_signature: None,
                };
                return Ok(spawn_gemini_response_stream(
                    Some(item),
                    "gemini-error-thought-signature".to_string(),
                    None,
                ));
            }

            return Err(CodexErr::UnexpectedStatus(
                crate::error::UnexpectedResponseError {
                    status,
                    body,
                    url: Some(url.clone()),
                    cf_ray: None,
                    request_id: None,
                },
            ));
        }

        let idle_timeout = provider.stream_idle_timeout();
        let byte_stream = response.bytes_stream();
        Ok(spawn_gemini_sse_stream(byte_stream, idle_timeout))
    }
}

fn validate_image_input_compat(prompt: &Prompt, model_slug: &str) -> Result<()> {
    if !model_supports_input_images(model_slug) && prompt_contains_input_images(prompt) {
        return Err(CodexErr::UnsupportedOperation(format!(
            "Model {model_slug} does not support image inputs."
        )));
    }

    if model_supports_data_url_input_images(model_slug) || !prompt_contains_data_url_images(prompt)
    {
        return Ok(());
    }

    Err(CodexErr::UnsupportedOperation(format!(
        "Model {model_slug} does not support `data:` image inputs. Use a public HTTPS image URL instead."
    )))
}

fn build_reasoning_payload(
    supports_reasoning_summaries: bool,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
) -> Option<Reasoning> {
    if !supports_reasoning_summaries {
        return None;
    }

    let summary = if summary == ReasoningSummaryConfig::None {
        None
    } else {
        Some(summary)
    };
    if effort.is_none() && summary.is_none() {
        return None;
    }

    Some(Reasoning { effort, summary })
}

fn sanitize_reasoning_effort_for_model(
    effort: Option<ReasoningEffortConfig>,
    model_slug: &str,
) -> Option<ReasoningEffortConfig> {
    if effort.is_none() || model_supports_reasoning_effort(model_slug) {
        return effort;
    }

    warn!(
        "model_reasoning_effort is set but ignored as the model does not support reasoning.effort: {model_slug}"
    );
    None
}

fn supports_memory_trace_summarize(provider: &ModelProviderInfo, model_slug: &str) -> bool {
    !provider.is_grok() && model_supports_memory_trace_summarize(model_slug)
}

fn canonical_model_slug_for_provider<'a>(
    provider: &ModelProviderInfo,
    model_slug: &'a str,
) -> Cow<'a, str> {
    if !provider.is_grok() {
        return Cow::Borrowed(model_slug);
    }

    let canonical = match normalized_grok_model_slug(model_slug) {
        Some("grok-4.1") => "grok-4-latest",
        Some(grok_slug) => grok_slug,
        None => model_slug,
    };
    Cow::Borrowed(canonical)
}

fn resolve_gemini_max_output_tokens(model_slug: &str) -> Option<i32> {
    if let Ok(raw) = std::env::var("CODEX_GEMINI_MAX_OUTPUT_TOKENS") {
        let trimmed = raw.trim();
        if let Ok(parsed) = trimmed.parse::<i32>()
            && parsed > 0
        {
            return Some(parsed);
        }
        warn!("Ignoring invalid CODEX_GEMINI_MAX_OUTPUT_TOKENS value: {trimmed}");
    }

    // Some local Gemma-compatible gateways default to tiny (or zero) output
    // token budgets when the field is omitted.
    if is_gemma_model_slug(model_slug) {
        Some(8192)
    } else {
        None
    }
}

fn prompt_contains_input_images(prompt: &Prompt) -> bool {
    prompt.input.iter().any(response_item_contains_input_image)
}

fn response_item_contains_input_image(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { content, .. } => content.iter().any(content_item_is_input_image),
        ResponseItem::FunctionCallOutput { output, .. } => output
            .content_items()
            .is_some_and(|items| items.iter().any(function_output_item_is_input_image)),
        _ => false,
    }
}

fn prompt_contains_data_url_images(prompt: &Prompt) -> bool {
    prompt
        .input
        .iter()
        .any(response_item_contains_data_url_image)
}

fn response_item_contains_data_url_image(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { content, .. } => content.iter().any(content_item_is_data_url_image),
        ResponseItem::FunctionCallOutput { output, .. } => output
            .content_items()
            .is_some_and(|items| items.iter().any(function_output_item_is_data_url_image)),
        _ => false,
    }
}

fn content_item_is_input_image(item: &ContentItem) -> bool {
    matches!(item, ContentItem::InputImage { .. })
}

fn content_item_is_data_url_image(item: &ContentItem) -> bool {
    matches!(
        item,
        ContentItem::InputImage { image_url } if is_data_url_image(image_url)
    )
}

fn function_output_item_is_input_image(item: &FunctionCallOutputContentItem) -> bool {
    matches!(item, FunctionCallOutputContentItem::InputImage { .. })
}

fn function_output_item_is_data_url_image(item: &FunctionCallOutputContentItem) -> bool {
    matches!(
        item,
        FunctionCallOutputContentItem::InputImage { image_url } if is_data_url_image(image_url)
    )
}

fn is_data_url_image(url: &str) -> bool {
    url.trim_start()
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
}

/// Parses per-turn metadata into an HTTP header value.
///
/// Invalid values are treated as absent so callers can compare and propagate
/// metadata with the same sanitization path used when constructing headers.
fn parse_turn_metadata_header(turn_metadata_header: Option<&str>) -> Option<HeaderValue> {
    turn_metadata_header.and_then(|value| HeaderValue::from_str(value).ok())
}

/// Builds the extra headers attached to Responses API requests.
///
/// These headers implement Codex-specific conventions:
///
/// - `x-codex-beta-features`: comma-separated beta feature keys enabled for the session.
/// - `x-codex-turn-state`: sticky routing token captured earlier in the turn.
/// - `x-codex-turn-metadata`: optional per-turn metadata for observability.
fn build_responses_headers(
    beta_features_header: Option<&str>,
    turn_state: Option<&Arc<OnceLock<String>>>,
    turn_metadata_header: Option<&HeaderValue>,
) -> ApiHeaderMap {
    let mut headers = ApiHeaderMap::new();
    if let Some(value) = beta_features_header
        && !value.is_empty()
        && let Ok(header_value) = HeaderValue::from_str(value)
    {
        headers.insert("x-codex-beta-features", header_value);
    }
    if let Some(turn_state) = turn_state
        && let Some(state) = turn_state.get()
        && let Ok(header_value) = HeaderValue::from_str(state)
    {
        headers.insert(X_CODEX_TURN_STATE_HEADER, header_value);
    }
    if let Some(header_value) = turn_metadata_header {
        headers.insert(X_CODEX_TURN_METADATA_HEADER, header_value.clone());
    }
    headers
}

fn map_response_stream<S>(
    api_stream: S,
    otel_manager: OtelManager,
) -> (ResponseStream, oneshot::Receiver<LastResponse>)
where
    S: futures::Stream<Item = std::result::Result<ResponseEvent, ApiError>>
        + Unpin
        + Send
        + 'static,
{
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);
    let (tx_last_response, rx_last_response) = oneshot::channel::<LastResponse>();

    tokio::spawn(async move {
        let mut logged_error = false;
        let mut tx_last_response = Some(tx_last_response);
        let mut items_added: Vec<ResponseItem> = Vec::new();
        let mut api_stream = api_stream;
        while let Some(event) = api_stream.next().await {
            match event {
                Ok(ResponseEvent::OutputItemDone(item)) => {
                    items_added.push(item.clone());
                    if tx_event
                        .send(Ok(ResponseEvent::OutputItemDone(item)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(ResponseEvent::Completed {
                    response_id,
                    token_usage,
                    can_append,
                }) => {
                    if let Some(usage) = &token_usage {
                        otel_manager.sse_event_completed(
                            usage.input_tokens,
                            usage.output_tokens,
                            Some(usage.cached_input_tokens),
                            Some(usage.reasoning_output_tokens),
                            usage.total_tokens,
                        );
                    }
                    if let Some(sender) = tx_last_response.take() {
                        let _ = sender.send(LastResponse {
                            response_id: response_id.clone(),
                            items_added: std::mem::take(&mut items_added),
                            can_append,
                        });
                    }
                    if tx_event
                        .send(Ok(ResponseEvent::Completed {
                            response_id,
                            token_usage,
                            can_append,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(event) => {
                    if tx_event.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Err(err) => {
                    let mapped = map_api_error(err);
                    if !logged_error {
                        otel_manager.see_event_completed_failed(&mapped);
                        logged_error = true;
                    }
                    if tx_event.send(Err(mapped)).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    (ResponseStream { rx_event }, rx_last_response)
}

/// Handles a 401 response by optionally refreshing ChatGPT tokens once.
///
/// When refresh succeeds, the caller should retry the API call; otherwise
/// the mapped `CodexErr` is returned to the caller.
async fn handle_unauthorized(
    transport: TransportError,
    auth_recovery: &mut Option<UnauthorizedRecovery>,
) -> Result<()> {
    if let Some(recovery) = auth_recovery
        && recovery.has_next()
    {
        return match recovery.next().await {
            Ok(_) => Ok(()),
            Err(RefreshTokenError::Permanent(failed)) => Err(CodexErr::RefreshTokenFailed(failed)),
            Err(RefreshTokenError::Transient(other)) => Err(CodexErr::Io(other)),
        };
    }

    Err(map_api_error(ApiError::Transport(transport)))
}

struct ApiTelemetry {
    otel_manager: OtelManager,
}

impl ApiTelemetry {
    fn new(otel_manager: OtelManager) -> Self {
        Self { otel_manager }
    }
}

impl RequestTelemetry for ApiTelemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<HttpStatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let error_message = error.map(std::string::ToString::to_string);
        self.otel_manager.record_api_request(
            attempt,
            status.map(|s| s.as_u16()),
            error_message.as_deref(),
            duration,
        );
    }
}

impl SseTelemetry for ApiTelemetry {
    fn on_sse_poll(
        &self,
        result: &std::result::Result<
            Option<std::result::Result<Event, EventStreamError<TransportError>>>,
            tokio::time::error::Elapsed,
        >,
        duration: Duration,
    ) {
        self.otel_manager.log_sse_event(result, duration);
    }
}

impl WebsocketTelemetry for ApiTelemetry {
    fn on_ws_request(&self, duration: Duration, error: Option<&ApiError>) {
        let error_message = error.map(std::string::ToString::to_string);
        self.otel_manager
            .record_websocket_request(duration, error_message.as_deref());
    }

    fn on_ws_event(
        &self,
        result: &std::result::Result<Option<std::result::Result<Message, Error>>, ApiError>,
        duration: Duration,
    ) {
        self.otel_manager.record_websocket_event(result, duration);
    }
}

#[cfg(test)]
mod tests {
    use super::ModelClient;
    use codex_otel::OtelManager;
    use codex_protocol::ThreadId;
    use codex_protocol::openai_models::ModelInfo;
    use codex_protocol::protocol::SessionSource;
    use codex_protocol::protocol::SubAgentSource;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn test_model_client(session_source: SessionSource) -> ModelClient {
        let provider = crate::model_provider_info::create_oss_provider_with_base_url(
            "https://example.com/v1",
            crate::model_provider_info::WireApi::Responses,
        );
        ModelClient::new(
            None,
            ThreadId::new(),
            provider,
            session_source,
            None,
            false,
            false,
            false,
            false,
            None,
        )
    }

    fn test_model_info() -> ModelInfo {
        serde_json::from_value(json!({
            "slug": "gpt-test",
            "display_name": "gpt-test",
            "description": "desc",
            "default_reasoning_level": "medium",
            "supported_reasoning_levels": [
                {"effort": "medium", "description": "medium"}
            ],
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": true,
            "priority": 1,
            "upgrade": null,
            "base_instructions": "base instructions",
            "model_messages": null,
            "supports_reasoning_summaries": false,
            "support_verbosity": false,
            "default_verbosity": null,
            "apply_patch_tool_type": null,
            "truncation_policy": {"mode": "bytes", "limit": 10000},
            "supports_parallel_tool_calls": false,
            "context_window": 272000,
            "auto_compact_token_limit": null,
            "experimental_supported_tools": []
        }))
        .expect("deserialize test model info")
    }

    fn test_otel_manager() -> OtelManager {
        OtelManager::new(
            ThreadId::new(),
            "gpt-test",
            "gpt-test",
            None,
            None,
            None,
            "test-originator".to_string(),
            false,
            "test-terminal".to_string(),
            SessionSource::Cli,
        )
    }

    #[test]
    fn build_subagent_headers_sets_other_subagent_label() {
        let client = test_model_client(SessionSource::SubAgent(SubAgentSource::Other(
            "memory_consolidation".to_string(),
        )));
        let headers = client.build_subagent_headers();
        let value = headers
            .get("x-openai-subagent")
            .and_then(|value| value.to_str().ok());
        assert_eq!(value, Some("memory_consolidation"));
    }

    #[tokio::test]
    async fn summarize_memories_returns_empty_for_empty_input() {
        let client = test_model_client(SessionSource::Cli);
        let model_info = test_model_info();
        let otel_manager = test_otel_manager();

        let output = client
            .summarize_memories(Vec::new(), &model_info, None, &otel_manager)
            .await
            .expect("empty summarize request should succeed");
        assert_eq!(output.len(), 0);
    }
}
