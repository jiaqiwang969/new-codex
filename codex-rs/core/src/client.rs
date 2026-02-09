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
//! requests during that turn. It caches a Responses WebSocket connection (opened lazily, or reused
//! from a session-level preconnect) and stores per-turn state such as the `x-codex-turn-state`
//! token used for sticky routing.
//!
//! Preconnect is intentionally handshake-only: it may warm a socket and capture sticky-routing
//! state, but the first `response.create` payload is still sent only when a turn starts.
//!
//! Internally, startup preconnect and warmed-socket adoption share one session-level lifecycle:
//! `Idle` (no task/socket), `InFlight` (startup preconnect task running), and `Ready` (one-shot
//! warmed socket available). On first use in a turn, the session tries to adopt `Ready`; if not
//! ready, it awaits `InFlight` and retries adoption before opening a new websocket. This prevents
//! racing duplicate first-turn handshakes while keeping preconnect best-effort.
//!
//! ## Retry-Budget Tradeoff
//!
//! `stream_max_retries` applies to retryable turn stream failures, not to background startup
//! preconnect handshakes. In failure cases this can produce two websocket handshakes on the first
//! turn (startup preconnect, then turn-time connect) before HTTP fallback becomes sticky. We keep
//! this split intentionally so opportunistic preconnect cannot consume the user-visible stream
//! retry budget before any turn payload is sent.
//!
//! If this policy needs to change later, preconnect can be modeled as an explicit first connection
//! attempt in the same retry budget as turn streaming. That would require plumbing websocket
//! attempt accounting from connection acquisition into the turn retry loop and updating fallback
//! expectations/tests accordingly.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
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
use codex_api::MemoryTrace as ApiMemoryTrace;
use codex_api::MemoryTraceSummarizeInput as ApiMemoryTraceSummarizeInput;
use codex_api::MemoryTraceSummaryOutput as ApiMemoryTraceSummaryOutput;
use codex_api::Prompt as ApiPrompt;
use codex_api::RequestTelemetry;
use codex_api::ReqwestTransport;
use codex_api::ResponseAppendWsRequest;
use codex_api::ResponseCreateWsRequest;
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
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;
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
use crate::model_compat::model_supports_data_url_input_images;
use crate::model_compat::model_supports_input_images;
use crate::model_compat::model_supports_memory_trace_summarize;
use crate::model_compat::model_supports_reasoning_effort;
use crate::model_compat::normalized_grok_model_slug;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::WireApi;
use crate::tools::spec::create_tools_json_for_responses_api;
use crate::turn_metadata::build_turn_metadata_header;
use crate::turn_metadata::resolve_turn_metadata_header_with_timeout;

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
    /// Session-scoped preconnect lifecycle state.
    ///
    /// This keeps startup preconnect task tracking and warmed-socket adoption in one lock so
    /// turn-time websocket setup observes a single, coherent state.
    preconnect: Mutex<PreconnectState>,
}

impl std::fmt::Debug for ModelClientState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelClientState")
            .field("auth_manager", &self.auth_manager)
            .field("conversation_id", &self.conversation_id)
            .field("provider", &self.provider)
            .field("session_source", &self.session_source)
            .field("model_verbosity", &self.model_verbosity)
            .field(
                "enable_responses_websockets",
                &self.enable_responses_websockets,
            )
            .field(
                "enable_request_compression",
                &self.enable_request_compression,
            )
            .field("include_timing_metrics", &self.include_timing_metrics)
            .field("beta_features_header", &self.beta_features_header)
            .field(
                "disable_websockets",
                &self.disable_websockets.load(Ordering::Relaxed),
            )
            .field("preconnect", &"<opaque>")
            .finish()
    }
}

/// Resolved API client setup for a single request attempt.
///
/// Keeping this as a single bundle ensures preconnect and normal request paths
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
    connection: ApiWebSocketConnection,
    turn_state: Option<String>,
}

/// Session-level lifecycle of startup websocket preconnect.
///
/// `InFlight` tracks the startup task so the first turn can await it and reuse the same socket.
/// `Ready` stores a one-shot warmed socket for turn adoption.
enum PreconnectState {
    /// No startup preconnect task is active and no warmed socket is available.
    Idle,
    /// Startup preconnect is currently running; first turn may await this task.
    InFlight(JoinHandle<()>),
    /// Startup preconnect finished and produced a one-shot warmed socket.
    Ready(PreconnectedWebSocket),
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
///
/// This type is cheap to clone.
#[derive(Debug, Clone)]
pub struct ModelClient {
    state: Arc<ModelClientState>,
}

/// A turn-scoped streaming session created from a [`ModelClient`].
///
/// The session establishes a Responses WebSocket connection lazily (or adopts a preconnected one)
/// and reuses it across multiple requests within the turn. It also caches per-turn state:
///
/// - The last request's input items, so subsequent calls can use `response.append` when the input
///   is an incremental extension of the previous request.
/// - The `x-codex-turn-state` sticky-routing token, which must be replayed for all requests within
///   the same turn.
///
/// When startup preconnect is still running, first use of this session awaits that in-flight task
/// before opening a new websocket so preconnect acts as the first connection attempt for the turn.
///
/// Create a fresh `ModelClientSession` for each Codex turn. Reusing it across turns would replay
/// the previous turn's sticky-routing token into the next turn, which violates the client/server
/// contract and can cause routing bugs.
pub struct ModelClientSession {
    client: ModelClient,
    connection: Option<ApiWebSocketConnection>,
    websocket_last_items: Vec<ResponseItem>,
    websocket_last_response_id: Option<String>,
    websocket_last_response_id_rx: Option<oneshot::Receiver<String>>,
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
    /// This constructor does not perform network I/O itself. The returned session either adopts a
    /// previously preconnected websocket or opens a websocket lazily when the first stream request
    /// is issued.
    pub fn new_session(&self) -> ModelClientSession {
        ModelClientSession {
            client: self.clone(),
            connection: None,
            websocket_last_items: Vec::new(),
            websocket_last_response_id: None,
            websocket_last_response_id_rx: None,
            turn_state: Arc::new(OnceLock::new()),
        }
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
                build_turn_metadata_header(cwd.as_path()),
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

    /// Builds memory summaries for each provided normalized trace.
    ///
    /// This is a unary call (no streaming) to `/v1/memories/trace_summarize`.
    ///
    /// The model selection, reasoning effort, and telemetry context are passed explicitly to keep
    /// `ModelClient` session-scoped.
    pub async fn summarize_memory_traces(
        &self,
        traces: Vec<ApiMemoryTrace>,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
        otel_manager: &OtelManager,
    ) -> Result<Vec<ApiMemoryTraceSummaryOutput>> {
        if traces.is_empty() {
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

        let payload = ApiMemoryTraceSummarizeInput {
            model: model_info.slug.clone(),
            traces,
            reasoning: effort.map(|effort| Reasoning {
                effort: Some(effort),
                summary: None,
            }),
        };

        client
            .trace_summarize_input(&payload, self.build_subagent_headers())
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
    fn responses_websocket_enabled(&self) -> bool {
        self.state.provider.supports_websockets && self.state.enable_responses_websockets
    }

    fn responses_websockets_v2_enabled(&self) -> bool {
        self.state.enable_responses_websockets_v2
    }

    /// Returns whether websocket transport has been permanently disabled for this session.
    ///
    /// Once set by fallback activation, subsequent turns must stay on HTTP transport.
    fn disable_websockets(&self) -> bool {
        self.state.disable_websockets.load(Ordering::Relaxed)
    }

    /// Returns auth + provider configuration resolved from the current session auth state.
    ///
    /// This centralizes setup used by both preconnect and normal request paths so they stay in
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
    /// Both startup preconnect and in-turn `needs_new` reconnects call this path so handshake
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
            .connect(headers, turn_state, Some(websocket_telemetry))
            .await
    }

    /// Builds websocket handshake headers for both preconnect and turn-time reconnect.
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

    /// Consumes the warmed websocket slot.
    fn take_preconnected_websocket(&self) -> Option<PreconnectedWebSocket> {
        let mut state = self
            .state
            .preconnect
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::mem::replace(&mut *state, PreconnectState::Idle);
        match previous {
            PreconnectState::Ready(preconnected) => Some(preconnected),
            other => {
                *state = other;
                None
            }
        }
    }

    /// Stores a freshly preconnected websocket and optional captured turn-state token.
    ///
    /// This overwrites any previously warmed socket because only one preconnect candidate is kept.
    fn store_preconnected_websocket(
        &self,
        connection: ApiWebSocketConnection,
        turn_state: Option<String>,
    ) {
        let mut state = self
            .state
            .preconnect
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.disable_websockets() {
            debug!("discarding startup websocket preconnect because websocket fallback is active");
            *state = PreconnectState::Idle;
            return;
        }
        *state = PreconnectState::Ready(PreconnectedWebSocket {
            connection,
            turn_state,
        });
    }

    /// Stores the latest startup preconnect task handle.
    ///
    /// If a previous task is still running, it is aborted so only one in-flight startup attempt
    /// is tracked.
    fn store_preconnect_task(&self, task: JoinHandle<()>) {
        let mut task = Some(task);
        let previous_in_flight = {
            let mut state = self
                .state
                .preconnect
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*state {
                // A very fast startup preconnect can complete before this method stores the
                // task handle; keep the warmed socket and drop the now-useless handle.
                PreconnectState::Ready(_) => None,
                _ => match task.take() {
                    Some(next_task) => {
                        match std::mem::replace(&mut *state, PreconnectState::InFlight(next_task)) {
                            PreconnectState::InFlight(previous) => Some(previous),
                            _ => None,
                        }
                    }
                    None => None,
                },
            }
        };
        if let Some(previous) = previous_in_flight {
            previous.abort();
        }
        if let Some(task) = task {
            task.abort();
        }
    }

    /// Awaits the startup preconnect task once, if one is currently tracked.
    ///
    /// This lets the first turn treat startup preconnect as the first websocket connection
    /// attempt, avoiding a redundant second connect while the preconnect attempt is in flight.
    ///
    /// This await intentionally has no separate timeout wrapper. WebSocket connect handshakes
    /// already run without an app-level timeout, so waiting on the in-flight preconnect task does
    /// not add a new unbounded wait class; it reuses the same first connection attempt.
    async fn await_preconnect_task(&self) {
        let task = {
            let mut state = self
                .state
                .preconnect
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let previous = std::mem::replace(&mut *state, PreconnectState::Idle);
            match previous {
                PreconnectState::InFlight(task) => Some(task),
                other => {
                    *state = other;
                    None
                }
            }
        };
        if let Some(task) = task {
            let in_flight = !task.is_finished();
            if in_flight {
                debug!("awaiting startup websocket preconnect before opening a new websocket");
            }
            if let Err(err) = task.await {
                debug!("startup websocket preconnect task failed: {err}");
            }
        }
    }

    /// Clears all startup preconnect state.
    ///
    /// This aborts any in-flight startup preconnect task and drops any warmed socket.
    fn clear_preconnect(&self) {
        let previous = {
            let mut state = self
                .state
                .preconnect
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut *state, PreconnectState::Idle)
        };
        if let PreconnectState::InFlight(task) = previous {
            task.abort();
        }
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

    fn build_responses_request(prompt: &Prompt) -> Result<ApiPrompt> {
        let instructions = prompt.base_instructions.text.clone();
        let tools_json: Vec<Value> = create_tools_json_for_responses_api(&prompt.tools)?;
        Ok(build_api_prompt(prompt, instructions, tools_json))
    }

    #[allow(clippy::too_many_arguments)]
    /// Builds shared Responses API request options for both HTTP and WebSocket streaming.
    ///
    /// Keeping option construction in one place ensures request-scoped headers are consistent
    /// regardless of transport choice.
    fn build_responses_options(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        turn_metadata_header: Option<&str>,
        compression: Compression,
    ) -> ApiResponsesOptions {
        let turn_metadata_header = parse_turn_metadata_header(turn_metadata_header);

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
        let conversation_id = self.client.state.conversation_id.to_string();

        ApiResponsesOptions {
            reasoning,
            include,
            prompt_cache_key: Some(conversation_id.clone()),
            text,
            store_override: None,
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

    fn get_incremental_items(&self, input_items: &[ResponseItem]) -> Option<Vec<ResponseItem>> {
        // Checks whether the current request input is an incremental append to the previous request.
        // If items in the new request contain all the items from the previous request we build
        // a response.append request otherwise we start with a fresh response.create request.
        let previous_len = self.websocket_last_items.len();
        let can_append = previous_len > 0
            && input_items.starts_with(&self.websocket_last_items)
            && previous_len < input_items.len();
        if can_append {
            Some(input_items[previous_len..].to_vec())
        } else {
            None
        }
    }

    fn refresh_websocket_last_response_id(&mut self) {
        if let Some(mut receiver) = self.websocket_last_response_id_rx.take() {
            match receiver.try_recv() {
                Ok(response_id) if !response_id.is_empty() => {
                    self.websocket_last_response_id = Some(response_id);
                }
                Ok(_) | Err(TryRecvError::Closed) => {
                    self.websocket_last_response_id = None;
                }
                Err(TryRecvError::Empty) => {
                    self.websocket_last_response_id_rx = Some(receiver);
                }
            }
        }
    }

    fn websocket_previous_response_id(&mut self) -> Option<String> {
        self.refresh_websocket_last_response_id();
        self.websocket_last_response_id
            .clone()
            .filter(|id| !id.is_empty())
    }

    fn prepare_websocket_create_request(
        &self,
        model_slug: &str,
        api_prompt: &ApiPrompt,
        options: &ApiResponsesOptions,
        input: Vec<ResponseItem>,
        previous_response_id: Option<String>,
    ) -> ResponsesWsRequest {
        let ApiResponsesOptions {
            reasoning,
            include,
            prompt_cache_key,
            text,
            store_override,
            ..
        } = options;

        let store = store_override.unwrap_or(false);
        let payload = ResponseCreateWsRequest {
            model: model_slug.to_string(),
            instructions: api_prompt.instructions.clone(),
            previous_response_id,
            input,
            tools: api_prompt.tools.clone(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: api_prompt.parallel_tool_calls,
            reasoning: reasoning.clone(),
            store,
            stream: true,
            include: include.clone(),
            prompt_cache_key: prompt_cache_key.clone(),
            text: text.clone(),
        };

        ResponsesWsRequest::ResponseCreate(payload)
    }

    fn prepare_websocket_request(
        &mut self,
        model_slug: &str,
        api_prompt: &ApiPrompt,
        options: &ApiResponsesOptions,
    ) -> ResponsesWsRequest {
        let responses_websockets_v2_enabled = self.client.responses_websockets_v2_enabled();
        let incremental_items = self.get_incremental_items(&api_prompt.input);
        if let Some(append_items) = incremental_items {
            if responses_websockets_v2_enabled
                && let Some(previous_response_id) = self.websocket_previous_response_id()
            {
                return self.prepare_websocket_create_request(
                    model_slug,
                    api_prompt,
                    options,
                    append_items,
                    Some(previous_response_id),
                );
            }

            if !responses_websockets_v2_enabled {
                return ResponsesWsRequest::ResponseAppend(ResponseAppendWsRequest {
                    input: append_items,
                });
            }
        }

        self.prepare_websocket_create_request(
            model_slug,
            api_prompt,
            options,
            api_prompt.input.clone(),
            None,
        )
    }

    /// Returns a websocket connection for this turn, reusing preconnect when possible.
    ///
    /// This method first tries to adopt the session-level preconnect slot, then falls back to a
    /// fresh websocket handshake only when the turn has no live connection. If startup preconnect
    /// is still running, it is awaited first so that task acts as the first connection attempt for
    /// this turn instead of racing a second handshake. If that attempt fails, the normal connect
    /// and stream retry flow continues unchanged.
    async fn websocket_connection(
        &mut self,
        otel_manager: &OtelManager,
        api_provider: codex_api::Provider,
        api_auth: CoreAuthProvider,
        turn_metadata_header: Option<&str>,
        options: &ApiResponsesOptions,
    ) -> std::result::Result<&ApiWebSocketConnection, ApiError> {
        // Prefer the session-level preconnect slot before creating a new websocket.
        if self.connection.is_none() {
            if let Some(preconnected) = self.try_use_preconnected_websocket() {
                self.adopt_preconnected_websocket(preconnected);
            } else {
                self.client.await_preconnect_task().await;
                if let Some(preconnected) = self.try_use_preconnected_websocket() {
                    self.adopt_preconnected_websocket(preconnected);
                }
            }
        }

        let needs_new = match self.connection.as_ref() {
            Some(conn) => conn.is_closed().await,
            None => true,
        };

        if needs_new {
            self.client.clear_preconnect();
            self.websocket_last_items.clear();
            self.websocket_last_response_id = None;
            self.websocket_last_response_id_rx = None;
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

    /// Adopts the session-level preconnect slot for this turn.
    ///
    /// If a turn-local connection already exists, this intentionally does nothing to avoid
    /// replacing an active connection mid-turn.
    fn try_use_preconnected_websocket(&mut self) -> Option<PreconnectedWebSocket> {
        if self.connection.is_some() {
            return None;
        }

        self.client.take_preconnected_websocket()
    }

    /// Moves a preconnected socket into the turn-local connection slot.
    ///
    /// If the preconnect handshake captured sticky-routing turn state, this also seeds the
    /// turn-local state lock so all later requests in the turn replay the same token.
    fn adopt_preconnected_websocket(&mut self, preconnected: PreconnectedWebSocket) {
        let PreconnectedWebSocket {
            connection,
            turn_state,
        } = preconnected;
        if let Some(turn_state) = turn_state {
            let _ = self.turn_state.set(turn_state);
        }
        self.connection = Some(connection);
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
            return Ok(map_response_stream(stream, otel_manager.clone()));
        }

        validate_image_input_compat(prompt, &model_info.slug)?;

        let auth_manager = self.client.state.auth_manager.clone();
        let api_prompt = Self::build_responses_request(prompt)?;

        let mut auth_recovery = auth_manager
            .as_ref()
            .map(super::auth::AuthManager::unauthorized_recovery);
        loop {
            let client_setup = self.client.current_client_setup().await?;
            let transport = ReqwestTransport::new(build_reqwest_client());
            let (request_telemetry, sse_telemetry) = Self::build_streaming_telemetry(otel_manager);
            let compression = self.responses_request_compression(client_setup.auth.as_ref());

            let client = ApiResponsesClient::new(
                transport,
                client_setup.api_provider,
                client_setup.api_auth,
            )
            .with_telemetry(Some(request_telemetry), Some(sse_telemetry));

            let options = self.build_responses_options(
                prompt,
                model_info,
                effort,
                summary,
                turn_metadata_header,
                compression,
            );
            let model_slug =
                canonical_model_slug_for_provider(&self.client.state.provider, &model_info.slug);

            let stream_result = client
                .stream_prompt(model_slug.as_ref(), &api_prompt, options)
                .await;

            match stream_result {
                Ok(stream) => {
                    return Ok(map_response_stream(stream, otel_manager.clone()));
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
    ) -> Result<ResponseStream> {
        validate_image_input_compat(prompt, &model_info.slug)?;

        let auth_manager = self.client.state.auth_manager.clone();
        let api_prompt = Self::build_responses_request(prompt)?;

        let mut auth_recovery = auth_manager
            .as_ref()
            .map(super::auth::AuthManager::unauthorized_recovery);
        loop {
            let client_setup = self.client.current_client_setup().await?;
            let compression = self.responses_request_compression(client_setup.auth.as_ref());

            let options = self.build_responses_options(
                prompt,
                model_info,
                effort,
                summary,
                turn_metadata_header,
                compression,
            );
            let model_slug =
                canonical_model_slug_for_provider(&self.client.state.provider, &model_info.slug);
            let request =
                self.prepare_websocket_request(model_slug.as_ref(), &api_prompt, &options);

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
                Err(ApiError::Transport(
                    unauthorized_transport @ TransportError::Http { status, .. },
                )) if status == StatusCode::UNAUTHORIZED => {
                    handle_unauthorized(unauthorized_transport, &mut auth_recovery).await?;
                    continue;
                }
                Err(err) => return Err(map_api_error(err)),
            }

            let stream_result = self
                .connection
                .as_ref()
                .ok_or_else(|| {
                    map_api_error(ApiError::Stream(
                        "websocket connection is unavailable".to_string(),
                    ))
                })?
                .stream_request(request)
                .await
                .map_err(map_api_error)?;
            self.websocket_last_items = api_prompt.input.clone();
            let (last_response_id_sender, last_response_id_receiver) = oneshot::channel();
            self.websocket_last_response_id_rx = Some(last_response_id_receiver);
            let mut last_response_id_sender = Some(last_response_id_sender);
            let stream_result = stream_result.inspect(move |event| {
                if let Ok(ResponseEvent::Completed { response_id, .. }) = event
                    && !response_id.is_empty()
                    && let Some(sender) = last_response_id_sender.take()
                {
                    let _ = sender.send(response_id.clone());
                }
            });

            return Ok(map_response_stream(stream_result, otel_manager.clone()));
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
                let websocket_enabled =
                    self.client.responses_websocket_enabled() && !self.client.disable_websockets();

                if websocket_enabled {
                    self.stream_responses_websocket(
                        prompt,
                        model_info,
                        otel_manager,
                        effort,
                        summary,
                        turn_metadata_header,
                    )
                    .await
                } else {
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
            }
            WireApi::Gemini => self.stream_gemini(prompt, model_info, effort).await,
        }
    }

    /// Permanently disables WebSockets for this Codex session and resets WebSocket state.
    ///
    /// This is used after exhausting the provider retry budget, to force subsequent requests onto
    /// the HTTP transport. It also clears any warmed websocket preconnect state so future turns
    /// cannot accidentally adopt a stale socket after fallback has been activated.
    ///
    /// Startup preconnect handshakes are intentionally not counted against `stream_max_retries`.
    /// See [`crate::client`] module docs ("Retry-Budget Tradeoff") for rationale and future
    /// alternatives.
    ///
    /// Returns `true` if this call activated fallback, or `false` if fallback was already active.
    pub(crate) fn try_switch_fallback_transport(&mut self, otel_manager: &OtelManager) -> bool {
        let websocket_enabled = self.client.responses_websocket_enabled();
        let activated = self.activate_http_fallback(websocket_enabled);
        if activated {
            warn!("falling back to HTTP");
            otel_manager.counter(
                "codex.transport.fallback_to_http",
                1,
                &[("from_wire_api", "responses_websocket")],
            );

            self.connection = None;
            self.websocket_last_items.clear();
            self.client.clear_preconnect();
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

        let generation_config = Some(GeminiGenerationConfig {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            max_output_tokens: None,
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

        let gemini_api_key = crate::auth::read_gemini_api_key_from_env().or_else(|| {
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
        });

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

/// Adapts the core `Prompt` type into the `codex-api` payload shape.
fn build_api_prompt(prompt: &Prompt, instructions: String, tools_json: Vec<Value>) -> ApiPrompt {
    let input =
        crate::gemini_content::strip_thought_signatures_from_input(&prompt.get_formatted_input());
    ApiPrompt {
        instructions,
        input,
        tools: tools_json,
        parallel_tool_calls: prompt.parallel_tool_calls,
        output_schema: prompt.output_schema.clone(),
    }
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

fn map_response_stream<S>(api_stream: S, otel_manager: OtelManager) -> ResponseStream
where
    S: futures::Stream<Item = std::result::Result<ResponseEvent, ApiError>>
        + Unpin
        + Send
        + 'static,
{
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);

    tokio::spawn(async move {
        let mut logged_error = false;
        let mut api_stream = api_stream;
        while let Some(event) = api_stream.next().await {
            match event {
                Ok(ResponseEvent::Completed {
                    response_id,
                    token_usage,
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
                    if tx_event
                        .send(Ok(ResponseEvent::Completed {
                            response_id,
                            token_usage,
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

    ResponseStream { rx_event }
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
    use super::*;
    use crate::models_manager::model_info::find_model_info_for_slug;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn test_session(provider: ModelProviderInfo) -> ModelClientSession {
        let client = ModelClient::new(
            None,
            ThreadId::new(),
            provider,
            SessionSource::Exec,
            None,
            false,
            false,
            false,
            false,
            None,
        );
        client.new_session()
    }

    fn prompt_with_single_image(image_url: &str) -> Prompt {
        Prompt {
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputImage {
                    image_url: image_url.to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn grok_3_images_are_rejected_with_clear_error() {
        let prompt = prompt_with_single_image("https://example.com/cat.png");

        let err = validate_image_input_compat(&prompt, "grok-3")
            .expect_err("grok-3 should reject image inputs");
        let message = err.to_string();
        assert!(
            message.contains("does not support image inputs"),
            "error should explain the compatibility issue, got: {message}"
        );
    }

    #[test]
    fn grok_data_url_images_remain_allowed() {
        let prompt = prompt_with_single_image("data:image/png;base64,AAAA");

        validate_image_input_compat(&prompt, "grok-4-0709")
            .expect("grok-4 should keep existing data URL image behavior");
    }

    #[test]
    fn grok_4_https_images_remain_allowed() {
        let prompt = prompt_with_single_image("https://example.com/cat.png");

        validate_image_input_compat(&prompt, "grok-4-latest")
            .expect("grok-4 should keep existing HTTPS image behavior");
    }

    #[test]
    fn non_grok_data_url_images_remain_allowed() {
        let prompt = prompt_with_single_image("data:image/png;base64,AAAA");

        validate_image_input_compat(&prompt, "gpt-5-codex")
            .expect("non-grok models should keep existing data URL behavior");
    }

    #[test]
    fn canonical_model_slug_maps_grok_aliases_for_grok_provider() {
        let provider = ModelProviderInfo::create_grok_provider();

        assert_eq!(
            canonical_model_slug_for_provider(&provider, "grok-4.1"),
            "grok-4-latest"
        );
        assert_eq!(
            canonical_model_slug_for_provider(&provider, "xai/grok-4-latest"),
            "grok-4-latest"
        );
        assert_eq!(
            canonical_model_slug_for_provider(&provider, "grok-3-mini"),
            "grok-3-mini"
        );
    }

    #[test]
    fn canonical_model_slug_keeps_original_for_non_grok_provider() {
        let provider = ModelProviderInfo::create_openai_provider();

        assert_eq!(
            canonical_model_slug_for_provider(&provider, "xai/grok-4-latest"),
            "xai/grok-4-latest"
        );
        assert_eq!(
            canonical_model_slug_for_provider(&provider, "grok-4.1"),
            "grok-4.1"
        );
    }

    #[test]
    fn reasoning_effort_is_filtered_for_grok_4_models() {
        assert_eq!(
            sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::Low), "grok-4-latest"),
            None
        );
    }

    #[test]
    fn reasoning_effort_is_kept_for_grok_3_mini_models() {
        assert_eq!(
            sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::Low), "grok-3-mini"),
            Some(ReasoningEffortConfig::Low)
        );
    }

    #[test]
    fn reasoning_effort_is_kept_for_non_grok_models() {
        assert_eq!(
            sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::Low), "gpt-5-codex"),
            Some(ReasoningEffortConfig::Low)
        );
    }

    #[test]
    fn reasoning_payload_omits_when_disabled_without_effort() {
        let reasoning = build_reasoning_payload(true, None, ReasoningSummaryConfig::None);

        assert!(reasoning.is_none());
    }

    #[test]
    fn reasoning_payload_keeps_summary_when_requested() {
        let reasoning = build_reasoning_payload(true, None, ReasoningSummaryConfig::Auto);

        assert_eq!(
            serde_json::to_value(reasoning).unwrap(),
            json!({"summary": "auto"})
        );
    }

    #[test]
    fn reasoning_payload_keeps_effort_when_present() {
        let reasoning = build_reasoning_payload(
            true,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::None,
        );

        assert_eq!(
            serde_json::to_value(reasoning).unwrap(),
            json!({"effort": "low"})
        );
    }

    #[test]
    fn reasoning_payload_is_omitted_when_model_does_not_support_summaries() {
        let reasoning = build_reasoning_payload(
            false,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::Auto,
        );

        assert!(reasoning.is_none());
    }

    #[test]
    fn responses_options_omit_reasoning_when_grok_effort_is_filtered_and_summary_is_disabled() {
        let session = test_session(ModelProviderInfo::create_grok_provider());
        let prompt = Prompt::default();
        let model_info = find_model_info_for_slug("grok-4-latest");
        let options = session.build_responses_options(
            &prompt,
            &model_info,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::None,
            None,
            Compression::None,
        );

        assert!(options.reasoning.is_none());
        assert!(options.include.is_empty());
    }

    #[test]
    fn responses_options_keep_reasoning_summary_for_grok_models() {
        let session = test_session(ModelProviderInfo::create_grok_provider());
        let prompt = Prompt::default();
        let model_info = find_model_info_for_slug("grok-4-latest");
        let options = session.build_responses_options(
            &prompt,
            &model_info,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::Auto,
            None,
            Compression::None,
        );

        assert_eq!(
            serde_json::to_value(options.reasoning).unwrap(),
            json!({"summary": "auto"})
        );
        assert_eq!(
            options.include,
            vec!["reasoning.encrypted_content".to_string()]
        );
    }

    #[test]
    fn responses_options_keep_reasoning_effort_for_non_grok_models() {
        let session = test_session(ModelProviderInfo::create_openai_provider());
        let prompt = Prompt::default();
        let model_info = find_model_info_for_slug("gpt-5-codex");
        let options = session.build_responses_options(
            &prompt,
            &model_info,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::None,
            None,
            Compression::None,
        );

        assert_eq!(
            serde_json::to_value(options.reasoning).unwrap(),
            json!({"effort": "low"})
        );
        assert_eq!(
            options.include,
            vec!["reasoning.encrypted_content".to_string()]
        );
    }

    #[test]
    fn responses_options_omit_reasoning_for_models_without_reasoning_summaries() {
        let session = test_session(ModelProviderInfo::create_gemini_provider());
        let prompt = Prompt::default();
        let model_info = find_model_info_for_slug("gemini-2.5-pro");
        let options = session.build_responses_options(
            &prompt,
            &model_info,
            Some(ReasoningEffortConfig::Low),
            ReasoningSummaryConfig::Auto,
            None,
            Compression::None,
        );

        assert!(options.reasoning.is_none());
        assert!(options.include.is_empty());
    }

    #[tokio::test]
    async fn summarize_memory_traces_fails_fast_for_grok_models() {
        let client = ModelClient::new(
            None,
            ThreadId::new(),
            ModelProviderInfo::create_grok_provider(),
            SessionSource::Exec,
            None,
            false,
            false,
            false,
            false,
            None,
        );
        let model_info = find_model_info_for_slug("grok-4-latest");
        let otel_manager = OtelManager::new(
            ThreadId::new(),
            "grok-4-latest",
            "grok-4-latest",
            None,
            None,
            None,
            "test_originator".to_string(),
            false,
            "test".to_string(),
            SessionSource::Exec,
        );
        let traces = vec![ApiMemoryTrace {
            id: "trace-1".to_string(),
            metadata: codex_api::MemoryTraceMetadata {
                source_path: "trace.jsonl".to_string(),
            },
            items: vec![json!({"role": "user", "content": "hello"})],
        }];

        let err = client
            .summarize_memory_traces(
                traces,
                &model_info,
                Some(ReasoningEffortConfig::Low),
                &otel_manager,
            )
            .await
            .expect_err("grok should fail fast before making memory trace requests");

        assert!(
            err.to_string().contains("Memory trace summarization"),
            "unexpected error message: {err}"
        );
    }

    #[tokio::test]
    async fn summarize_memory_traces_fails_fast_when_provider_is_grok() {
        let client = ModelClient::new(
            None,
            ThreadId::new(),
            ModelProviderInfo::create_grok_provider(),
            SessionSource::Exec,
            None,
            false,
            false,
            false,
            false,
            None,
        );
        let model_info = find_model_info_for_slug("gpt-5-codex");
        let otel_manager = OtelManager::new(
            ThreadId::new(),
            "gpt-5-codex",
            "gpt-5-codex",
            None,
            None,
            None,
            "test_originator".to_string(),
            false,
            "test".to_string(),
            SessionSource::Exec,
        );
        let traces = vec![ApiMemoryTrace {
            id: "trace-1".to_string(),
            metadata: codex_api::MemoryTraceMetadata {
                source_path: "trace.jsonl".to_string(),
            },
            items: vec![json!({"role": "user", "content": "hello"})],
        }];

        let err = client
            .summarize_memory_traces(
                traces,
                &model_info,
                Some(ReasoningEffortConfig::Low),
                &otel_manager,
            )
            .await
            .expect_err("grok provider should fail fast before making memory trace requests");

        assert!(
            err.to_string().contains("Memory trace summarization"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn memory_trace_summarization_is_disabled_for_grok_models() {
        assert!(!model_supports_memory_trace_summarize("grok-4-latest"));
        assert!(!model_supports_memory_trace_summarize("xai/grok-4-latest"));
    }

    #[test]
    fn memory_trace_summarization_stays_enabled_for_non_grok_models() {
        assert!(model_supports_memory_trace_summarize("gpt-5-codex"));
    }

    #[test]
    fn memory_trace_summarization_is_disabled_when_provider_is_grok() {
        let provider = ModelProviderInfo::create_grok_provider();

        assert!(!supports_memory_trace_summarize(&provider, "gpt-5-codex"));
    }
}
