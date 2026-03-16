use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use tokio::sync::OnceCell;

use crate::AuthManager;
use crate::CodexAuth;
use crate::SandboxState;
use crate::agent::AgentControl;
use crate::agent::AgentStatus;
use crate::agent::agent_status_from_event;
use crate::analytics_client::AnalyticsEventsClient;
use crate::analytics_client::AppInvocation;
use crate::analytics_client::InvocationType;
use crate::analytics_client::build_track_events_context;
use crate::apps::render_apps_section;
use crate::commit_attribution::commit_message_trailer_instruction;
use crate::compact;
use crate::compact::InitialContextInjection;
use crate::compact::run_inline_auto_compact_task;
use crate::compact::should_use_remote_compact_task;
use crate::compact_remote::run_inline_remote_auto_compact_task;
use crate::connectors;
use crate::exec_policy::ExecPolicyManager;
use crate::features::FEATURES;
use crate::features::Feature;
use crate::features::Features;
use crate::features::maybe_push_unstable_features_warning;
use crate::model_compat::is_anthropic_model_slug;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::is_grok_model_slug;
use crate::model_compat::is_openai_model_slug;
#[cfg(test)]
use crate::models_manager::collaboration_mode_presets::CollaborationModesConfig;
use crate::models_manager::manager::ModelsManager;
use crate::parse_command::parse_command;
use crate::parse_turn_item;
use crate::realtime_conversation::RealtimeConversationManager;
use crate::realtime_conversation::handle_audio as handle_realtime_conversation_audio;
use crate::realtime_conversation::handle_close as handle_realtime_conversation_close;
use crate::realtime_conversation::handle_start as handle_realtime_conversation_start;
use crate::realtime_conversation::handle_text as handle_realtime_conversation_text;
use crate::rollout::session_index;
use crate::stream_events_utils::HandleOutputCtx;
use crate::stream_events_utils::handle_non_tool_response_item;
use crate::stream_events_utils::handle_output_item_done;
use crate::stream_events_utils::last_assistant_message_from_item;
use crate::stream_events_utils::raw_assistant_output_text_from_item;
use crate::stream_events_utils::record_completed_response_item;
use crate::terminal;
use crate::truncate::TruncationPolicy;
use crate::turn_metadata::TurnMetadataState;
use crate::util::error_or_panic;
use crate::ws_version_from_features;
use async_channel::Receiver;
use async_channel::Sender;
use codex_hooks::HookEvent;
use codex_hooks::HookEventAfterAgent;
use codex_hooks::HookEventMemoryContext;
use codex_hooks::HookPayload;
use codex_hooks::HookResult;
use codex_hooks::Hooks;
use codex_hooks::HooksConfig;
use codex_network_proxy::NetworkProxy;
use codex_network_proxy::NetworkProxyAuditMetadata;
use codex_network_proxy::normalize_host;
use codex_protocol::ThreadId;
use codex_protocol::approvals::ExecPolicyAmendment;
use codex_protocol::approvals::NetworkPolicyAmendment;
use codex_protocol::approvals::NetworkPolicyRuleAction;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::dynamic_tools::DynamicToolResponse;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::items::PlanItem;
use codex_protocol::items::TurnItem;
use codex_protocol::items::UserMessageItem;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::models::format_allow_prefixes;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::HasLegacyEvent;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::MemoryLink;
use codex_protocol::protocol::RawResponseItemEvent;
use codex_protocol::protocol::ReviewRequest;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::TurnContextNetworkItem;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputResponse;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use codex_utils_stream_parser::AssistantTextChunk;
use codex_utils_stream_parser::AssistantTextStreamParser;
use codex_utils_stream_parser::ProposedPlanSegment;
use codex_utils_stream_parser::extract_proposed_plan_text;
use codex_utils_stream_parser::strip_citations;
use futures::future::BoxFuture;
use futures::prelude::*;
use futures::stream::FuturesOrdered;
use rmcp::model::ListResourceTemplatesResult;
use rmcp::model::ListResourcesResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ReadResourceRequestParams;
use rmcp::model::ReadResourceResult;
use rmcp::model::RequestId;
use serde_json;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::debug;
use tracing::error;
use tracing::field;
use tracing::info;
use tracing::info_span;
use tracing::instrument;
use tracing::trace;
use tracing::trace_span;
use tracing::warn;
use uuid::Uuid;

use crate::ModelProviderAccount;
use crate::ModelProviderInfo;
use crate::client::ModelClient;
use crate::client::ModelClientSession;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::compact::collect_user_messages;
use crate::config::Config;
use crate::config::Constrained;
use crate::config::ConstraintResult;
use crate::config::GhostSnapshotConfig;
use crate::config::StartedNetworkProxy;
use crate::config::resolve_web_search_mode_for_turn;
use crate::config::types::McpServerConfig;
use crate::config::types::ShellEnvironmentPolicy;
use crate::context_manager::ContextManager;
use crate::context_manager::TotalTokenUsageBreakdown;
use crate::environment_context::EnvironmentContext;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
#[cfg(test)]
use crate::exec::StreamOutput;
use codex_config::CONFIG_TOML_FILE;

#[derive(Debug, PartialEq)]
pub enum SteerInputError {
    NoActiveTurn(Vec<UserInput>),
    ExpectedTurnMismatch { expected: String, actual: String },
    EmptyInput,
}
use crate::exec_policy::ExecPolicyUpdateError;
use crate::feedback_tags;
use crate::file_watcher::FileWatcher;
use crate::file_watcher::FileWatcherEvent;
use crate::git_info::get_git_repo_root;
use crate::instructions::UserInstructions;
use crate::mcp::CODEX_APPS_MCP_SERVER_NAME;
use crate::mcp::auth::compute_auth_statuses;
use crate::mcp::effective_mcp_servers;
use crate::mcp::maybe_prompt_and_install_mcp_dependencies;
use crate::mcp::with_codex_apps_mcp;
use crate::mcp_connection_manager::McpConnectionManager;
use crate::mcp_connection_manager::codex_apps_tools_cache_key;
use crate::mcp_connection_manager::filter_codex_apps_mcp_tools_only;
use crate::mcp_connection_manager::filter_mcp_tools_by_name;
use crate::mcp_connection_manager::filter_non_codex_apps_mcp_tools_only;
use crate::memories;
use crate::mentions::build_connector_slug_counts;
use crate::mentions::build_skill_name_counts;
use crate::mentions::collect_explicit_app_ids;
use crate::mentions::collect_tool_mentions_from_messages;
use crate::network_policy_decision::execpolicy_network_rule_amendment;
use crate::plugins::PluginsManager;
use crate::project_doc::get_user_instructions;
use crate::protocol::AgentMessageContentDeltaEvent;
use crate::protocol::AgentReasoningSectionBreakEvent;
use crate::protocol::ApplyPatchApprovalRequestEvent;
use crate::protocol::AskForApproval;
use crate::protocol::BackgroundEventEvent;
use crate::protocol::DeprecationNoticeEvent;
use crate::protocol::ErrorEvent;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::protocol::ExecApprovalRequestEvent;
use crate::protocol::McpServerRefreshConfig;
use crate::protocol::ModelRerouteEvent;
use crate::protocol::ModelRerouteReason;
use crate::protocol::NetworkApprovalContext;
use crate::protocol::Op;
use crate::protocol::PlanDeltaEvent;
use crate::protocol::RateLimitSnapshot;
use crate::protocol::ReasoningContentDeltaEvent;
use crate::protocol::ReasoningRawContentDeltaEvent;
use crate::protocol::RequestUserInputEvent;
use crate::protocol::ReviewDecision;
use crate::protocol::SandboxPolicy;
use crate::protocol::SessionConfiguredEvent;
use crate::protocol::SessionNetworkProxyRuntime;
use crate::protocol::SkillDependencies as ProtocolSkillDependencies;
use crate::protocol::SkillErrorInfo;
use crate::protocol::SkillInterface as ProtocolSkillInterface;
use crate::protocol::SkillMetadata as ProtocolSkillMetadata;
use crate::protocol::SkillToolDependency as ProtocolSkillToolDependency;
use crate::protocol::StreamErrorEvent;
use crate::protocol::Submission;
use crate::protocol::TokenCountEvent;
use crate::protocol::TokenUsage;
use crate::protocol::TokenUsageInfo;
use crate::protocol::TurnDiffEvent;
use crate::protocol::WarningEvent;
use crate::rollout::RolloutRecorder;
use crate::rollout::RolloutRecorderParams;
use crate::rollout::map_session_init_error;
use crate::rollout::metadata;
use crate::rollout::policy::EventPersistenceMode;
use crate::shell;
use crate::shell_snapshot::ShellSnapshot;
use crate::skills::SkillError;
use crate::skills::SkillInjections;
use crate::skills::SkillLoadOutcome;
use crate::skills::SkillMetadata;
use crate::skills::SkillsManager;
use crate::skills::build_skill_injections;
use crate::skills::collect_env_var_dependencies;
use crate::skills::collect_explicit_skill_mentions;
use crate::skills::injection::ToolMentionKind;
use crate::skills::injection::app_id_from_path;
use crate::skills::injection::tool_kind_for_path;
use crate::skills::resolve_skill_dependencies_for_turn;
use crate::state::ActiveTurn;
use crate::state::SessionServices;
use crate::state::SessionState;
use crate::state_db;
use crate::tasks::GhostSnapshotTask;
use crate::tasks::RegularTask;
use crate::tasks::ReviewTask;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskContext;
use crate::tools::ToolRouter;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::handlers::SEARCH_TOOL_BM25_TOOL_NAME;
use crate::tools::js_repl::JsReplHandle;
use crate::tools::js_repl::resolve_compatible_node;
use crate::tools::network_approval::NetworkApprovalService;
use crate::tools::network_approval::build_blocked_request_observer;
use crate::tools::network_approval::build_network_policy_decider;
use crate::tools::parallel::ToolCallRuntime;
use crate::tools::sandboxing::ApprovalStore;
use crate::tools::spec::ToolsConfig;
use crate::tools::spec::ToolsConfigParams;
use crate::turn_diff_tracker::TurnDiffTracker;
use crate::unified_exec::UnifiedExecProcessManager;
use crate::util::backoff;
use crate::windows_sandbox::WindowsSandboxLevelExt;
use codex_async_utils::OrCancelExt;
use codex_otel::OtelManager;
use codex_otel::TelemetryAuthMode;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::Personality;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::models::ContentItem;
use codex_protocol::models::DeveloperInstructions;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_readiness::Readiness;
use codex_utils_readiness::ReadinessFlag;
use codex_utils_string::take_bytes_at_char_boundary;

/// The high-level interface to the Codex system.
/// It operates as a queue pair where you send submissions and receive events.
pub struct Codex {
    pub(crate) tx_sub: Sender<Submission>,
    pub(crate) rx_event: Receiver<Event>,
    // Last known status of the agent.
    pub(crate) agent_status: watch::Receiver<AgentStatus>,
    pub(crate) session: Arc<Session>,
}

/// Wrapper returned by [`Codex::spawn`] containing the spawned [`Codex`],
/// the submission id for the initial `ConfigureSession` request and the
/// unique session id.
pub struct CodexSpawnOk {
    pub codex: Codex,
    pub thread_id: ThreadId,
    #[deprecated(note = "use thread_id")]
    pub conversation_id: ThreadId,
}

pub(crate) const INITIAL_SUBMIT_ID: &str = "";
pub(crate) const SUBMISSION_CHANNEL_CAPACITY: usize = 512;
const SMALL_CONTEXT_WINDOW_THRESHOLD: i64 = 16_384;
const SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES: usize = 3_200;
const USER_INSTRUCTIONS_TRUNCATION_NOTICE: &str =
    "\n\n[AGENTS instructions truncated to fit local model context window.]";

fn truncate_user_instructions_for_context(
    user_instructions: &str,
    context_window: Option<i64>,
) -> String {
    let Some(context_window) = context_window else {
        return user_instructions.to_string();
    };
    if context_window > SMALL_CONTEXT_WINDOW_THRESHOLD
        || user_instructions.len() <= SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES
    {
        return user_instructions.to_string();
    }

    let truncated =
        take_bytes_at_char_boundary(user_instructions, SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES)
            .trim_end();
    format!("{truncated}{USER_INSTRUCTIONS_TRUNCATION_NOTICE}")
}
const CYBER_VERIFY_URL: &str = "https://chatgpt.com/cyber";
const CYBER_SAFETY_URL: &str = "https://developers.openai.com/codex/concepts/cyber-safety";

impl Codex {
    /// Spawn a new [`Codex`] and initialize the session.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn spawn(
        mut config: Config,
        auth_manager: Arc<AuthManager>,
        models_manager: Arc<ModelsManager>,
        skills_manager: Arc<SkillsManager>,
        file_watcher: Arc<FileWatcher>,
        conversation_history: InitialHistory,
        session_source: SessionSource,
        agent_control: AgentControl,
        dynamic_tools: Vec<DynamicToolSpec>,
        persist_extended_history: bool,
        metrics_service_name: Option<String>,
    ) -> CodexResult<CodexSpawnOk> {
        let (tx_sub, rx_sub) = async_channel::bounded(SUBMISSION_CHANNEL_CAPACITY);
        let (tx_event, rx_event) = async_channel::unbounded();

        let plugins_manager = PluginsManager::new(config.codex_home.clone());
        let loaded_plugins = plugins_manager.plugins_for_config(&config);
        let loaded_skills = skills_manager.skills_for_config(&config);

        for err in &loaded_skills.errors {
            error!(
                "failed to load skill {}: {}",
                err.path.display(),
                err.message
            );
        }

        if let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { depth, .. }) = session_source
            && depth >= config.agent_max_depth
        {
            config.features.disable(Feature::Collab);
        }

        if config.features.enabled(Feature::JsRepl)
            && let Err(err) = resolve_compatible_node(config.js_repl_node_path.as_deref()).await
        {
            let message = format!(
                "Disabled `js_repl` for this session because the configured Node runtime is unavailable or incompatible. {err}"
            );
            warn!("{message}");
            config.features.disable(Feature::JsRepl);
            config.features.disable(Feature::JsReplToolsOnly);
            config.startup_warnings.push(message);
        }

        let allowed_skills_for_implicit_invocation =
            loaded_skills.allowed_skills_for_implicit_invocation();
        let user_instructions = get_user_instructions(
            &config,
            Some(&allowed_skills_for_implicit_invocation),
            Some(loaded_plugins.capability_summaries()),
        )
        .await;

        let exec_policy = if crate::guardian::is_guardian_subagent_source(&session_source) {
            // Guardian review should rely on built-in shell safety checks rather than
            // caller-provided exec policy rules that could bias the reviewer.
            ExecPolicyManager::default()
        } else {
            ExecPolicyManager::load(&config.config_layer_stack)
                .await
                .map_err(|err| CodexErr::Fatal(format!("failed to load rules: {err}")))?
        };

        let config = Arc::new(config);
        let _ = models_manager
            .list_models(crate::models_manager::manager::RefreshStrategy::OnlineIfUncached)
            .await;
        let model = models_manager
            .get_default_model(
                &config.model,
                crate::models_manager::manager::RefreshStrategy::OnlineIfUncached,
            )
            .await;

        // Resolve base instructions for the session. Priority order:
        // 1. config.base_instructions override
        // 2. conversation history => session_meta.base_instructions
        // 3. base_instructions for current model
        let model_info = models_manager.get_model_info(model.as_str(), &config).await;
        let base_instructions = config
            .base_instructions
            .clone()
            .or_else(|| conversation_history.get_base_instructions().map(|s| s.text))
            .unwrap_or_else(|| model_info.get_model_instructions(config.personality));

        // Respect thread-start tools. When missing (resumed/forked threads), read from the db
        // first, then fall back to rollout-file tools.
        let persisted_tools = if dynamic_tools.is_empty()
            && config.features.enabled(Feature::Sqlite)
        {
            let thread_id = match &conversation_history {
                InitialHistory::Resumed(resumed) => Some(resumed.conversation_id),
                InitialHistory::Forked(_) => conversation_history.forked_from_id(),
                InitialHistory::New => None,
            };
            match thread_id {
                Some(thread_id) => {
                    let state_db_ctx = state_db::get_state_db(&config, None).await;
                    state_db::get_dynamic_tools(state_db_ctx.as_deref(), thread_id, "codex_spawn")
                        .await
                }
                None => None,
            }
        } else {
            None
        };
        let dynamic_tools = if dynamic_tools.is_empty() {
            persisted_tools
                .or_else(|| conversation_history.get_dynamic_tools())
                .unwrap_or_default()
        } else {
            dynamic_tools
        };

        // TODO (aibrahim): Consolidate config.model and config.model_reasoning_effort into config.collaboration_mode
        // to avoid extracting these fields separately and constructing CollaborationMode here.
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: model.clone(),
                reasoning_effort: config.model_reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions,
            personality: config.personality,
            base_instructions,
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name,
            session_source,
            dynamic_tools,
            persist_extended_history,
        };

        // Generate a unique ID for the lifetime of this Codex session.
        let session_source_clone = session_configuration.session_source.clone();
        let (agent_status_tx, agent_status_rx) = watch::channel(AgentStatus::PendingInit);

        let session_init_span = info_span!("session_init");
        let session = Session::new(
            session_configuration,
            config.clone(),
            auth_manager.clone(),
            models_manager.clone(),
            exec_policy,
            tx_event.clone(),
            agent_status_tx.clone(),
            conversation_history,
            session_source_clone,
            skills_manager,
            file_watcher,
            agent_control,
        )
        .instrument(session_init_span)
        .await
        .map_err(|e| {
            error!("Failed to create session: {e:#}");
            map_session_init_error(&e, &config.codex_home)
        })?;
        let thread_id = session.conversation_id;

        // This task will run until Op::Shutdown is received.
        let session_loop_span = info_span!("session_loop", thread_id = %thread_id);
        tokio::spawn(
            submission_loop(Arc::clone(&session), config, rx_sub).instrument(session_loop_span),
        );
        let codex = Codex {
            tx_sub,
            rx_event,
            agent_status: agent_status_rx,
            session,
        };

        #[allow(deprecated)]
        Ok(CodexSpawnOk {
            codex,
            thread_id,
            conversation_id: thread_id,
        })
    }

    /// Submit the `op` wrapped in a `Submission` with a unique ID.
    pub async fn submit(&self, op: Op) -> CodexResult<String> {
        let id = Uuid::now_v7().to_string();
        let sub = Submission { id: id.clone(), op };
        self.submit_with_id(sub).await?;
        Ok(id)
    }

    /// Use sparingly: prefer `submit()` so Codex is responsible for generating
    /// unique IDs for each submission.
    pub async fn submit_with_id(&self, sub: Submission) -> CodexResult<()> {
        self.tx_sub
            .send(sub)
            .await
            .map_err(|_| CodexErr::InternalAgentDied)?;
        Ok(())
    }

    pub async fn next_event(&self) -> CodexResult<Event> {
        let event = self
            .rx_event
            .recv()
            .await
            .map_err(|_| CodexErr::InternalAgentDied)?;
        Ok(event)
    }

    pub async fn steer_input(
        &self,
        input: Vec<UserInput>,
        expected_turn_id: Option<&str>,
    ) -> Result<String, SteerInputError> {
        self.session.steer_input(input, expected_turn_id).await
    }

    pub(crate) async fn agent_status(&self) -> AgentStatus {
        self.agent_status.borrow().clone()
    }

    pub(crate) async fn thread_config_snapshot(&self) -> ThreadConfigSnapshot {
        let state = self.session.state.lock().await;
        state.session_configuration.thread_config_snapshot()
    }

    pub(crate) fn state_db(&self) -> Option<state_db::StateDbHandle> {
        self.session.state_db()
    }

    pub(crate) fn enabled(&self, feature: Feature) -> bool {
        self.session.enabled(feature)
    }
}

/// Context for an initialized model agent
///
/// A session has at most 1 running task at a time, and can be interrupted by user input.
pub(crate) struct Session {
    pub(crate) conversation_id: ThreadId,
    tx_event: Sender<Event>,
    agent_status: watch::Sender<AgentStatus>,
    state: Mutex<SessionState>,
    /// The set of enabled features should be invariant for the lifetime of the
    /// session.
    features: Features,
    pending_mcp_server_refresh_config: Mutex<Option<McpServerRefreshConfig>>,
    pub(crate) conversation: Arc<RealtimeConversationManager>,
    pub(crate) active_turn: Mutex<Option<ActiveTurn>>,
    pub(crate) services: SessionServices,
    js_repl: Arc<JsReplHandle>,
    next_internal_sub_id: AtomicU64,
}

#[derive(Clone, Debug)]
pub(crate) struct TurnSkillsContext {
    pub(crate) outcome: Arc<SkillLoadOutcome>,
    pub(crate) implicit_invocation_seen_skills: Arc<Mutex<HashSet<String>>>,
}
impl TurnSkillsContext {
    pub(crate) fn new(outcome: Arc<SkillLoadOutcome>) -> Self {
        Self {
            outcome,
            implicit_invocation_seen_skills: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

/// The context needed for a single turn of the thread.
#[derive(Debug)]
pub struct TurnContext {
    pub(crate) sub_id: String,
    pub(crate) config: Arc<Config>,
    pub(crate) auth_manager: Option<Arc<AuthManager>>,
    pub(crate) model_info: ModelInfo,
    pub(crate) otel_manager: OtelManager,
    pub(crate) provider: ModelProviderInfo,
    pub(crate) reasoning_effort: Option<ReasoningEffortConfig>,
    pub(crate) reasoning_summary: ReasoningSummaryConfig,
    pub(crate) session_source: SessionSource,
    /// The session's current working directory. All relative paths provided by
    /// the model as well as sandbox policies are resolved against this path
    /// instead of `std::env::current_dir()`.
    pub(crate) cwd: PathBuf,
    pub(crate) developer_instructions: Option<String>,
    pub(crate) compact_prompt: Option<String>,
    pub(crate) user_instructions: Option<String>,
    pub(crate) collaboration_mode: CollaborationMode,
    pub(crate) personality: Option<Personality>,
    pub(crate) approval_policy: Constrained<AskForApproval>,
    pub(crate) sandbox_policy: Constrained<SandboxPolicy>,
    pub(crate) network: Option<NetworkProxy>,
    pub(crate) windows_sandbox_level: WindowsSandboxLevel,
    pub(crate) shell_environment_policy: ShellEnvironmentPolicy,
    pub(crate) tools_config: ToolsConfig,
    pub(crate) features: Features,
    pub(crate) ghost_snapshot: GhostSnapshotConfig,
    pub(crate) final_output_json_schema: Option<Value>,
    pub(crate) codex_linux_sandbox_exe: Option<PathBuf>,
    pub(crate) tool_call_gate: Arc<ReadinessFlag>,
    pub(crate) truncation_policy: TruncationPolicy,
    pub(crate) js_repl: Arc<JsReplHandle>,
    pub(crate) dynamic_tools: Vec<DynamicToolSpec>,
    turn_metadata_header: OnceCell<Option<String>>,
    memory_read_path_source: OnceCell<Option<memories::MemoryReadPathSource>>,
    hook_memory_context: OnceCell<Option<HookEventMemoryContext>>,
    pub(crate) turn_metadata_state: Arc<TurnMetadataState>,
    pub(crate) side_effects_files:
        std::sync::Arc<tokio::sync::Mutex<std::collections::BTreeSet<String>>>,
    pub(crate) turn_skills: TurnSkillsContext,
}
impl TurnContext {
    pub(crate) fn model_context_window(&self) -> Option<i64> {
        let effective_context_window_percent = self.model_info.effective_context_window_percent;
        self.model_info.context_window.map(|context_window| {
            context_window.saturating_mul(effective_context_window_percent) / 100
        })
    }

    pub(crate) async fn with_model(&self, model: String, models_manager: &ModelsManager) -> Self {
        let mut config = (*self.config).clone();
        config.model = Some(model.clone());
        let (provider_id, logical_provider) =
            crate::utility_model::provider_for_model_slug(&config, &model).unwrap_or_else(|| {
                (
                    config.model_provider_id.clone(),
                    config.model_provider.clone(),
                )
            });
        let provider = if providers_match_ignoring_active_account(&self.provider, &logical_provider)
        {
            self.provider.clone()
        } else if let Some(account) =
            normalize_account_pool_in_config_order(provider_id.as_str(), &logical_provider)
                .into_iter()
                .next()
        {
            logical_provider.with_account(&account)
        } else {
            logical_provider.clone()
        };
        config.model_provider_id = provider_id;
        config.model_provider = logical_provider;
        let model_info = models_manager.get_model_info(model.as_str(), &config).await;
        let truncation_policy = model_info.truncation_policy.into();
        let supported_reasoning_levels = model_info
            .supported_reasoning_levels
            .iter()
            .map(|preset| preset.effort)
            .collect::<Vec<_>>();
        let reasoning_effort = if let Some(current_reasoning_effort) = self.reasoning_effort {
            if supported_reasoning_levels.contains(&current_reasoning_effort) {
                Some(current_reasoning_effort)
            } else {
                supported_reasoning_levels
                    .get(supported_reasoning_levels.len().saturating_sub(1) / 2)
                    .copied()
                    .or(model_info.default_reasoning_level)
            }
        } else {
            supported_reasoning_levels
                .get(supported_reasoning_levels.len().saturating_sub(1) / 2)
                .copied()
                .or(model_info.default_reasoning_level)
        };
        config.model_reasoning_effort = reasoning_effort;

        let collaboration_mode =
            self.collaboration_mode
                .with_updates(Some(model.clone()), Some(reasoning_effort), None);
        let features = self.features.clone();
        let tools_config = ToolsConfig::new(&ToolsConfigParams {
            model_info: &model_info,
            features: &features,
            web_search_mode: self.tools_config.web_search_mode,
            is_gemini_wire_api: provider.wire_api == crate::model_provider_info::WireApi::Gemini,
            endpoint_security: config.endpoint_security,
            session_source: self.session_source.clone(),
        })
        .with_allow_login_shell(self.tools_config.allow_login_shell)
        .with_agent_roles(config.agent_roles.clone());

        Self {
            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::BTreeSet::new(),
            )),

            sub_id: self.sub_id.clone(),
            config: Arc::new(config),
            auth_manager: self.auth_manager.clone(),
            model_info: model_info.clone(),
            otel_manager: self
                .otel_manager
                .clone()
                .with_model(model.as_str(), model_info.slug.as_str()),
            provider,
            reasoning_effort,
            reasoning_summary: self.reasoning_summary,
            session_source: self.session_source.clone(),
            cwd: self.cwd.clone(),
            developer_instructions: self.developer_instructions.clone(),
            compact_prompt: self.compact_prompt.clone(),
            user_instructions: self.user_instructions.clone(),
            collaboration_mode,
            personality: self.personality,
            approval_policy: self.approval_policy.clone(),
            sandbox_policy: self.sandbox_policy.clone(),
            network: self.network.clone(),
            windows_sandbox_level: self.windows_sandbox_level,
            shell_environment_policy: self.shell_environment_policy.clone(),
            tools_config,
            features,
            ghost_snapshot: self.ghost_snapshot.clone(),
            final_output_json_schema: self.final_output_json_schema.clone(),
            codex_linux_sandbox_exe: self.codex_linux_sandbox_exe.clone(),
            tool_call_gate: Arc::new(ReadinessFlag::new()),
            truncation_policy,
            js_repl: Arc::clone(&self.js_repl),
            dynamic_tools: self.dynamic_tools.clone(),
            turn_metadata_header: self.turn_metadata_header.clone(),
            memory_read_path_source: self.memory_read_path_source.clone(),
            hook_memory_context: self.hook_memory_context.clone(),
            turn_metadata_state: self.turn_metadata_state.clone(),
            turn_skills: self.turn_skills.clone(),
        }
    }

    pub(crate) fn resolve_path(&self, path: Option<String>) -> PathBuf {
        path.as_ref()
            .map(PathBuf::from)
            .map_or_else(|| self.cwd.clone(), |p| self.cwd.join(p))
    }

    pub(crate) fn compact_prompt(&self) -> &str {
        self.compact_prompt
            .as_deref()
            .unwrap_or(compact::SUMMARIZATION_PROMPT)
    }

    pub(crate) fn to_turn_context_item(&self) -> TurnContextItem {
        TurnContextItem {
            turn_id: Some(self.sub_id.clone()),
            cwd: self.cwd.clone(),
            approval_policy: self.approval_policy.value(),
            sandbox_policy: self.sandbox_policy.get().clone(),
            network: self.turn_context_network_item(),
            model: self.model_info.slug.clone(),
            personality: self.personality,
            collaboration_mode: Some(self.collaboration_mode.clone()),
            effort: self.reasoning_effort,
            summary: self.reasoning_summary,
            user_instructions: self.user_instructions.clone(),
            developer_instructions: self.developer_instructions.clone(),
            final_output_json_schema: self.final_output_json_schema.clone(),
            truncation_policy: Some(self.truncation_policy.into()),
        }
    }

    fn turn_context_network_item(&self) -> Option<TurnContextNetworkItem> {
        let network = self
            .config
            .config_layer_stack
            .requirements()
            .network
            .as_ref()?;
        Some(TurnContextNetworkItem {
            allowed_domains: network.allowed_domains.clone().unwrap_or_default(),
            denied_domains: network.denied_domains.clone().unwrap_or_default(),
        })
    }

    pub(crate) async fn resolve_memory_read_path_source(
        &self,
    ) -> Option<memories::MemoryReadPathSource> {
        self.memory_read_path_source
            .get_or_init(|| async {
                if !self.features.enabled(Feature::MemoryTool) {
                    return None;
                }
                memories::select_memory_read_path_source(&self.config.codex_home, &self.cwd).await
            })
            .await
            .clone()
    }

    pub(crate) async fn resolve_hook_memory_context(&self) -> Option<HookEventMemoryContext> {
        self.hook_memory_context
            .get_or_init(|| async { build_hook_memory_context(self).await })
            .await
            .clone()
    }

    pub(crate) async fn resolve_memory_link(&self) -> Option<MemoryLink> {
        let memory_context = self.resolve_hook_memory_context().await?;
        let scope_version = memory_context.active_memory_scope_version;
        let scope_kind = memory_context.active_scope_kind;
        let summary_sha256 = memory_context.active_memory_summary_sha256;
        let binding_key = memory_context.active_memory_binding_key;

        if scope_version.is_none()
            && scope_kind.is_none()
            && summary_sha256.is_none()
            && binding_key.is_none()
        {
            return None;
        }

        Some(MemoryLink {
            scope_version,
            scope_kind,
            summary_sha256,
            binding_key,
        })
    }
}

#[derive(Clone)]
pub(crate) struct SessionConfiguration {
    /// Provider identifier ("openai", "openrouter", ...).
    provider_id: String,

    /// Provider configuration.
    provider: ModelProviderInfo,

    collaboration_mode: CollaborationMode,
    model_reasoning_summary: ReasoningSummaryConfig,

    /// Developer instructions that supplement the base instructions.
    developer_instructions: Option<String>,

    /// Model instructions that are appended to the base instructions.
    user_instructions: Option<String>,

    /// Personality preference for the model.
    personality: Option<Personality>,

    /// Base instructions for the session.
    base_instructions: String,

    /// Compact prompt override.
    compact_prompt: Option<String>,

    /// When to escalate for approval for execution
    approval_policy: Constrained<AskForApproval>,
    approvals_reviewer: ApprovalsReviewer,
    /// How to sandbox commands executed in the system
    sandbox_policy: Constrained<SandboxPolicy>,
    windows_sandbox_level: WindowsSandboxLevel,

    /// Working directory that should be treated as the *root* of the
    /// session. All relative paths supplied by the model as well as the
    /// execution sandbox are resolved against this directory **instead**
    /// of the process-wide current working directory. CLI front-ends are
    /// expected to expand this to an absolute path before sending the
    /// `ConfigureSession` operation so that the business-logic layer can
    /// operate deterministically.
    cwd: PathBuf,
    /// Directory containing all Codex state for this session.
    codex_home: PathBuf,
    /// Optional user-facing name for the thread, updated during the session.
    thread_name: Option<String>,

    // TODO(pakrym): Remove config from here
    original_config_do_not_use: Arc<Config>,
    /// Optional service name tag for session metrics.
    metrics_service_name: Option<String>,
    /// Source of the session (cli, vscode, exec, mcp, ...)
    session_source: SessionSource,
    dynamic_tools: Vec<DynamicToolSpec>,
    persist_extended_history: bool,
}

impl SessionConfiguration {
    pub(crate) fn codex_home(&self) -> &PathBuf {
        &self.codex_home
    }

    fn thread_config_snapshot(&self) -> ThreadConfigSnapshot {
        ThreadConfigSnapshot {
            model: self.collaboration_mode.model().to_string(),
            model_provider_id: self.provider_id.clone(),
            approval_policy: self.approval_policy.value(),
            approvals_reviewer: self.approvals_reviewer,
            sandbox_policy: self.sandbox_policy.get().clone(),
            cwd: self.cwd.clone(),
            reasoning_effort: self.collaboration_mode.reasoning_effort(),
            personality: self.personality,
            session_source: self.session_source.clone(),
        }
    }

    /// Apply settings updates and return the new configuration plus an
    /// optional provider-switch label when the provider was auto-switched
    /// for a different model family.
    pub(crate) fn apply(
        &self,
        updates: &SessionSettingsUpdate,
    ) -> ConstraintResult<(Self, Option<String>)> {
        let mut next_configuration = self.clone();
        if let Some(collaboration_mode) = updates.collaboration_mode.clone() {
            next_configuration.collaboration_mode = collaboration_mode;
        }
        if let Some(summary) = updates.reasoning_summary {
            next_configuration.model_reasoning_summary = summary;
        }
        if let Some(personality) = updates.personality {
            next_configuration.personality = Some(personality);
        }
        if let Some(approval_policy) = updates.approval_policy {
            next_configuration.approval_policy.set(approval_policy)?;
        }
        if let Some(approvals_reviewer) = updates.approvals_reviewer {
            next_configuration.approvals_reviewer = approvals_reviewer;
        }
        if let Some(sandbox_policy) = updates.sandbox_policy.clone() {
            next_configuration.sandbox_policy.set(sandbox_policy)?;
        }
        if let Some(windows_sandbox_level) = updates.windows_sandbox_level {
            next_configuration.windows_sandbox_level = windows_sandbox_level;
        }
        if let Some(cwd) = updates.cwd.clone() {
            next_configuration.cwd = cwd;
        }
        if let (Some(provider_id), Some(provider)) = (
            updates.model_provider_id.clone(),
            updates.model_provider.clone(),
        ) {
            next_configuration.provider_id = provider_id;
            next_configuration.provider = provider;
            let mut updated_config = (*next_configuration.original_config_do_not_use).clone();
            updated_config.model_provider_id = next_configuration.provider_id.clone();
            updated_config.model_provider = next_configuration.provider.clone();
            next_configuration.original_config_do_not_use = Arc::new(updated_config);
        }

        // Auto-switch provider when the model family changes between
        // known provider families and default OpenAI-compatible models.
        // This ensures that `/model` switches at runtime route requests
        // to the correct API endpoint.
        let new_model = next_configuration.collaboration_mode.model();
        let target_provider_id = provider_id_for_model_family(new_model);
        let original_config = &next_configuration.original_config_do_not_use;
        let provider_is_auto_switched = !providers_match_ignoring_active_account(
            &next_configuration.provider,
            &original_config.user_configured_provider,
        );

        tracing::info!(
            new_model = %new_model,
            target_provider_id = ?target_provider_id,
            current_provider_id = %next_configuration.provider_id,
            current_provider_name = %next_configuration.provider.name,
            current_provider_base_url = ?next_configuration.provider.base_url,
            current_wire_api = ?next_configuration.provider.wire_api,
            current_is_grok = next_configuration.provider.is_grok(),
            provider_is_auto_switched,
            "apply() auto-switch check"
        );

        let mut provider_switch_label: Option<String> = None;

        if updates.collaboration_mode.is_some() {
            if let Some(target_provider_id) = target_provider_id {
                if !provider_matches_builtin_family(
                    &next_configuration.provider,
                    target_provider_id,
                ) {
                    // Use the merged provider map (built-in + user-defined from config.toml)
                    // so that custom providers with account_pool, env_keys, etc. are preserved.
                    let providers = &next_configuration
                        .original_config_do_not_use
                        .model_providers;
                    let old_provider_id = next_configuration.provider_id.clone();
                    if let Some(provider) = providers.get(target_provider_id) {
                        next_configuration.provider_id = target_provider_id.to_string();
                        next_configuration.provider = provider.clone();
                        let preview_provider =
                            normalize_account_pool_in_config_order(target_provider_id, provider)
                                .first()
                                .map(|account| provider.with_account(account))
                                .unwrap_or_else(|| provider.clone());
                        let account_label = account_index_label(&preview_provider);
                        let base_url = preview_provider.base_url.as_deref().unwrap_or("(default)");
                        provider_switch_label = Some(format!(
                            "{old_provider_id} -> {target_provider_id} [{account_label}] @ {base_url} (model: {new_model})"
                        ));
                        tracing::info!(
                            from_provider = %old_provider_id,
                            to_provider = %target_provider_id,
                            account = %account_label,
                            base_url = %base_url,
                            "auto-switching provider for model family"
                        );
                    } else {
                        tracing::warn!(
                            target_provider_id,
                            available_providers = ?providers.keys().collect::<Vec<_>>(),
                            "auto-switch: target provider not found in merged provider map"
                        );
                    }
                }
            } else if is_openai_model_slug(new_model)
                && next_configuration.provider.wire_api
                    != crate::model_provider_info::WireApi::Responses
            {
                let providers = &next_configuration
                    .original_config_do_not_use
                    .model_providers;
                let old_provider_id = next_configuration.provider_id.clone();

                let restored_provider = if original_config.user_configured_provider.wire_api
                    == crate::model_provider_info::WireApi::Responses
                {
                    original_config.user_configured_provider.clone()
                } else if let Some(openai) = providers.get("openai") {
                    openai.clone()
                } else {
                    original_config.user_configured_provider.clone()
                };
                next_configuration.provider_id = resolve_provider_id_for_provider(
                    providers,
                    &restored_provider,
                    &original_config.model_provider_id,
                );
                let preview_provider = normalize_account_pool_in_config_order(
                    next_configuration.provider_id.as_str(),
                    &restored_provider,
                )
                .first()
                .map(|account| restored_provider.with_account(account))
                .unwrap_or_else(|| restored_provider.clone());
                next_configuration.provider = restored_provider;

                let account_label = account_index_label(&preview_provider);
                let base_url = preview_provider.base_url.as_deref().unwrap_or("(default)");
                provider_switch_label = Some(format!(
                    "{} -> {} [{}] @ {} (model: {})",
                    old_provider_id,
                    next_configuration.provider_id,
                    account_label,
                    base_url,
                    new_model
                ));
            } else if provider_is_auto_switched {
                // Switching FROM a family-specific provider back to a default
                // model family: restore the user's explicitly configured provider
                // (before auto-switching).
                let old_provider_id = next_configuration.provider_id.clone();
                let restored_provider = original_config.user_configured_provider.clone();
                next_configuration.provider_id = resolve_provider_id_for_provider(
                    &original_config.model_providers,
                    &restored_provider,
                    &original_config.model_provider_id,
                );
                let preview_provider = normalize_account_pool_in_config_order(
                    next_configuration.provider_id.as_str(),
                    &restored_provider,
                )
                .first()
                .map(|account| restored_provider.with_account(account))
                .unwrap_or_else(|| restored_provider.clone());
                next_configuration.provider = restored_provider;
                let account_label = account_index_label(&preview_provider);
                let base_url = preview_provider.base_url.as_deref().unwrap_or("(default)");
                provider_switch_label = Some(format!(
                    "{} -> {} [{}] @ {} (model: {})",
                    old_provider_id,
                    next_configuration.provider_id,
                    account_label,
                    base_url,
                    new_model
                ));
            }
        } // End if updates.model.is_some()

        Ok((next_configuration, provider_switch_label))
    }
}

fn provider_id_for_model_family(model_slug: &str) -> Option<&'static str> {
    // Check for antigravity prefix first
    if model_slug.starts_with("antigravity/claude-")
        || model_slug.starts_with("antigravity-anthropic/")
    {
        Some(crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID)
    } else if model_slug.starts_with("antigravity/")
        || model_slug.starts_with("antigravity-gemini/")
    {
        // Antigravity non-Claude models use the native Gemini endpoint in this integration.
        Some(crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID)
    } else if is_gemma_model_slug(model_slug) {
        Some(crate::model_provider_info::GEMMA_PROVIDER_ID)
    } else if model_slug.starts_with("gemini-") {
        Some(crate::model_provider_info::GEMINI_PROVIDER_ID)
    } else if is_anthropic_model_slug(model_slug) {
        Some(crate::model_provider_info::ANTHROPIC_PROVIDER_ID)
    } else if is_grok_model_slug(model_slug) {
        Some(crate::model_provider_info::GROK_PROVIDER_ID)
    } else {
        None
    }
}

fn provider_matches_builtin_family(provider: &ModelProviderInfo, provider_id: &str) -> bool {
    match provider_id {
        crate::model_provider_info::GEMINI_PROVIDER_ID => {
            provider.wire_api == crate::model_provider_info::WireApi::Gemini
                && !provider.is_antigravity_gemini()
        }
        crate::model_provider_info::GEMMA_PROVIDER_ID => {
            provider.is_gemma()
                || (provider.wire_api == crate::model_provider_info::WireApi::Gemini
                    && !provider.is_gemini()
                    && !provider.is_antigravity_gemini())
        }
        crate::model_provider_info::ANTHROPIC_PROVIDER_ID => {
            provider.wire_api == crate::model_provider_info::WireApi::Anthropic
                && !provider.is_antigravity_anthropic()
        }
        crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID => {
            provider.is_antigravity_gemini()
        }
        crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID => {
            provider.is_antigravity_anthropic()
        }
        crate::model_provider_info::GROK_PROVIDER_ID => provider.is_grok(),
        _ => false,
    }
}

fn providers_match_ignoring_active_account(
    left: &ModelProviderInfo,
    right: &ModelProviderInfo,
) -> bool {
    let mut normalized_left = left.clone();
    if !normalized_left.account_pool.is_empty() {
        normalized_left.base_url = None;
        normalized_left.env_key = None;
    }
    let mut normalized_right = right.clone();
    if !normalized_right.account_pool.is_empty() {
        normalized_right.base_url = None;
        normalized_right.env_key = None;
    }
    normalized_left == normalized_right
}

fn pick_preferred_provider_id(mut ids: Vec<String>) -> String {
    if ids.len() == 1 {
        return ids.remove(0);
    }

    ids.sort();
    if let Some(openai_id) = ids.iter().find(|id| id.as_str() == "openai") {
        return openai_id.clone();
    }
    ids.remove(0)
}

fn resolve_provider_id_for_provider(
    providers: &HashMap<String, ModelProviderInfo>,
    provider: &ModelProviderInfo,
    fallback_provider_id: &str,
) -> String {
    // Exact match by provider identity after stripping any active pool account.
    if let Some(candidate) = providers.get(fallback_provider_id)
        && providers_match_ignoring_active_account(candidate, provider)
    {
        return fallback_provider_id.to_string();
    }

    let identity_matches = providers
        .iter()
        .filter_map(|(id, candidate)| {
            providers_match_ignoring_active_account(candidate, provider).then_some(id.clone())
        })
        .collect::<Vec<_>>();
    if !identity_matches.is_empty() {
        return pick_preferred_provider_id(identity_matches);
    }

    // Fallback: match by stable identity markers.
    if let Some(candidate) = providers.get(fallback_provider_id)
        && candidate.name == provider.name
        && candidate.wire_api == provider.wire_api
    {
        return fallback_provider_id.to_string();
    }

    let name_matches = providers
        .iter()
        .filter_map(|(id, candidate)| {
            (candidate.name == provider.name && candidate.wire_api == provider.wire_api)
                .then_some(id.clone())
        })
        .collect::<Vec<_>>();
    if !name_matches.is_empty() {
        return pick_preferred_provider_id(name_matches);
    }

    if provider.wire_api == crate::model_provider_info::WireApi::Responses
        && let Some(openai_provider) = providers.get("openai")
        && openai_provider.wire_api == crate::model_provider_info::WireApi::Responses
    {
        return "openai".to_string();
    }

    fallback_provider_id.to_string()
}

fn drop_provider_specific_encrypted_history_items(state: &mut SessionState) -> usize {
    let snapshot = state.clone_history();
    let original = snapshot.raw_items();
    let filtered = original
        .iter()
        .filter(|item| {
            !matches!(
                item,
                ResponseItem::Reasoning {
                    encrypted_content: Some(_),
                    ..
                } | ResponseItem::Compaction { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed_count = original.len().saturating_sub(filtered.len());
    if removed_count > 0 {
        state.replace_history(filtered, None);
    }
    removed_count
}

#[derive(Default, Clone)]
pub(crate) struct SessionSettingsUpdate {
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) approval_policy: Option<AskForApproval>,
    pub(crate) approvals_reviewer: Option<ApprovalsReviewer>,
    pub(crate) sandbox_policy: Option<SandboxPolicy>,
    pub(crate) windows_sandbox_level: Option<WindowsSandboxLevel>,
    pub(crate) model_provider_id: Option<String>,
    pub(crate) model_provider: Option<ModelProviderInfo>,
    pub(crate) collaboration_mode: Option<CollaborationMode>,
    pub(crate) reasoning_summary: Option<ReasoningSummaryConfig>,
    pub(crate) final_output_json_schema: Option<Option<Value>>,
    pub(crate) personality: Option<Personality>,
}

impl Session {
    /// Builds the `x-codex-beta-features` header value for this session.
    ///
    /// `ModelClient` is session-scoped and intentionally does not depend on the full `Config`, so
    /// we precompute the comma-separated list of enabled experimental feature keys at session
    /// creation time and thread it into the client.
    fn build_model_client_beta_features_header(config: &Config) -> Option<String> {
        let beta_features_header = FEATURES
            .iter()
            .filter_map(|spec| {
                if spec.stage.experimental_menu_description().is_some()
                    && config.features.enabled(spec.id)
                {
                    Some(spec.key)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(",");

        if beta_features_header.is_empty() {
            None
        } else {
            Some(beta_features_header)
        }
    }

    async fn start_managed_network_proxy(
        spec: &crate::config::NetworkProxySpec,
        sandbox_policy: &SandboxPolicy,
        network_policy_decider: Option<Arc<dyn codex_network_proxy::NetworkPolicyDecider>>,
        blocked_request_observer: Option<Arc<dyn codex_network_proxy::BlockedRequestObserver>>,
        managed_network_requirements_enabled: bool,
        audit_metadata: NetworkProxyAuditMetadata,
    ) -> anyhow::Result<(StartedNetworkProxy, SessionNetworkProxyRuntime)> {
        let network_proxy = spec
            .start_proxy(
                sandbox_policy,
                network_policy_decider,
                blocked_request_observer,
                managed_network_requirements_enabled,
                audit_metadata,
            )
            .await
            .map_err(|err| anyhow::anyhow!("failed to start managed network proxy: {err}"))?;
        let session_network_proxy = {
            let proxy = network_proxy.proxy();
            SessionNetworkProxyRuntime {
                http_addr: proxy.http_addr().to_string(),
                socks_addr: proxy.socks_addr().to_string(),
                admin_addr: proxy.admin_addr().to_string(),
            }
        };
        Ok((network_proxy, session_network_proxy))
    }

    /// Don't expand the number of mutated arguments on config. We are in the process of getting rid of it.
    pub(crate) fn build_per_turn_config(session_configuration: &SessionConfiguration) -> Config {
        // todo(aibrahim): store this state somewhere else so we don't need to mut config
        let config = session_configuration.original_config_do_not_use.clone();
        let mut per_turn_config = (*config).clone();
        per_turn_config.model = Some(session_configuration.collaboration_mode.model().to_string());
        per_turn_config.model_provider_id = session_configuration.provider_id.clone();
        per_turn_config.model_provider = session_configuration.provider.clone();
        per_turn_config.model_reasoning_effort =
            session_configuration.collaboration_mode.reasoning_effort();
        per_turn_config.model_reasoning_summary = session_configuration.model_reasoning_summary;
        per_turn_config.personality = session_configuration.personality;
        per_turn_config.approvals_reviewer = session_configuration.approvals_reviewer;
        let resolved_web_search_mode = resolve_web_search_mode_for_turn(
            &per_turn_config.web_search_mode,
            session_configuration.sandbox_policy.get(),
        );
        if let Err(err) = per_turn_config
            .web_search_mode
            .set(resolved_web_search_mode)
        {
            let fallback_value = per_turn_config.web_search_mode.value();
            tracing::warn!(
                error = %err,
                ?resolved_web_search_mode,
                ?fallback_value,
                "resolved web_search_mode is disallowed by requirements; keeping constrained value"
            );
        }
        per_turn_config.features = config.features.clone();
        per_turn_config
    }

    pub(crate) async fn codex_home(&self) -> PathBuf {
        let state = self.state.lock().await;
        state.session_configuration.codex_home().clone()
    }

    fn start_file_watcher_listener(self: &Arc<Self>) {
        let mut rx = self.services.file_watcher.subscribe();
        let weak_sess = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(FileWatcherEvent::SkillsChanged { .. }) => {
                        let Some(sess) = weak_sess.upgrade() else {
                            break;
                        };
                        let event = Event {
                            id: sess.next_internal_sub_id(),
                            msg: EventMsg::SkillsUpdateAvailable,
                        };
                        sess.send_event_raw(event).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn make_turn_context(
        auth_manager: Option<Arc<AuthManager>>,
        otel_manager: &OtelManager,
        provider: ModelProviderInfo,
        session_configuration: &SessionConfiguration,
        per_turn_config: Config,
        model_info: ModelInfo,
        network: Option<NetworkProxy>,
        sub_id: String,
        js_repl: Arc<JsReplHandle>,
        skills_outcome: Arc<SkillLoadOutcome>,
    ) -> TurnContext {
        let reasoning_effort = session_configuration.collaboration_mode.reasoning_effort();
        let reasoning_summary = session_configuration.model_reasoning_summary;
        let otel_manager = otel_manager.clone().with_model(
            session_configuration.collaboration_mode.model(),
            model_info.slug.as_str(),
        );
        let session_source = session_configuration.session_source.clone();
        let auth_manager_for_context = auth_manager;
        let provider_for_context = provider;
        let otel_manager_for_context = otel_manager;
        let per_turn_config = Arc::new(per_turn_config);

        let tools_config = ToolsConfig::new(&ToolsConfigParams {
            model_info: &model_info,
            features: &per_turn_config.features,
            web_search_mode: Some(per_turn_config.web_search_mode.value()),
            is_gemini_wire_api: provider_for_context.wire_api
                == crate::model_provider_info::WireApi::Gemini,
            endpoint_security: per_turn_config.endpoint_security,
            session_source: session_source.clone(),
        })
        .with_allow_login_shell(per_turn_config.permissions.allow_login_shell)
        .with_agent_roles(per_turn_config.agent_roles.clone());

        let cwd = session_configuration.cwd.clone();
        let turn_metadata_state = Arc::new(TurnMetadataState::new(
            sub_id.clone(),
            cwd.clone(),
            session_configuration.sandbox_policy.get(),
            session_configuration.windows_sandbox_level,
            per_turn_config
                .features
                .enabled(Feature::UseLinuxSandboxBwrap),
        ));
        TurnContext {
            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(
                std::collections::BTreeSet::new(),
            )),

            sub_id,
            config: per_turn_config.clone(),
            auth_manager: auth_manager_for_context,
            model_info: model_info.clone(),
            otel_manager: otel_manager_for_context,
            provider: provider_for_context,
            reasoning_effort,
            reasoning_summary,
            session_source,
            cwd,
            developer_instructions: session_configuration.developer_instructions.clone(),
            compact_prompt: session_configuration.compact_prompt.clone(),
            user_instructions: session_configuration.user_instructions.clone(),
            collaboration_mode: session_configuration.collaboration_mode.clone(),
            personality: session_configuration.personality,
            approval_policy: session_configuration.approval_policy.clone(),
            sandbox_policy: session_configuration.sandbox_policy.clone(),
            network,
            windows_sandbox_level: session_configuration.windows_sandbox_level,
            shell_environment_policy: per_turn_config.permissions.shell_environment_policy.clone(),
            tools_config,
            features: per_turn_config.features.clone(),
            ghost_snapshot: per_turn_config.ghost_snapshot.clone(),
            final_output_json_schema: None,
            codex_linux_sandbox_exe: per_turn_config.codex_linux_sandbox_exe.clone(),
            tool_call_gate: Arc::new(ReadinessFlag::new()),
            truncation_policy: model_info.truncation_policy.into(),
            js_repl,
            dynamic_tools: session_configuration.dynamic_tools.clone(),
            turn_metadata_header: OnceCell::new(),
            memory_read_path_source: OnceCell::new(),
            hook_memory_context: OnceCell::new(),
            turn_metadata_state,
            turn_skills: TurnSkillsContext::new(skills_outcome),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn new(
        mut session_configuration: SessionConfiguration,
        config: Arc<Config>,
        auth_manager: Arc<AuthManager>,
        models_manager: Arc<ModelsManager>,
        exec_policy: ExecPolicyManager,
        tx_event: Sender<Event>,
        agent_status: watch::Sender<AgentStatus>,
        initial_history: InitialHistory,
        session_source: SessionSource,
        skills_manager: Arc<SkillsManager>,
        file_watcher: Arc<FileWatcher>,
        agent_control: AgentControl,
    ) -> anyhow::Result<Arc<Self>> {
        debug!(
            "Configuring session: model={}; provider={:?}",
            session_configuration.collaboration_mode.model(),
            session_configuration.provider
        );
        if !session_configuration.cwd.is_absolute() {
            return Err(anyhow::anyhow!(
                "cwd is not absolute: {:?}",
                session_configuration.cwd
            ));
        }

        let forked_from_id = initial_history.forked_from_id();

        let (conversation_id, rollout_params) = match &initial_history {
            InitialHistory::New | InitialHistory::Forked(_) => {
                let conversation_id = ThreadId::default();
                (
                    conversation_id,
                    RolloutRecorderParams::new(
                        conversation_id,
                        forked_from_id,
                        session_source,
                        BaseInstructions {
                            text: session_configuration.base_instructions.clone(),
                        },
                        session_configuration.dynamic_tools.clone(),
                        if session_configuration.persist_extended_history {
                            EventPersistenceMode::Extended
                        } else {
                            EventPersistenceMode::Limited
                        },
                    ),
                )
            }
            InitialHistory::Resumed(resumed_history) => (
                resumed_history.conversation_id,
                RolloutRecorderParams::resume(
                    resumed_history.rollout_path.clone(),
                    if session_configuration.persist_extended_history {
                        EventPersistenceMode::Extended
                    } else {
                        EventPersistenceMode::Limited
                    },
                ),
            ),
        };
        let state_builder = match &initial_history {
            InitialHistory::Resumed(resumed) => metadata::builder_from_items(
                resumed.history.as_slice(),
                resumed.rollout_path.as_path(),
            ),
            InitialHistory::New | InitialHistory::Forked(_) => None,
        };

        // Kick off independent async setup tasks in parallel to reduce startup latency.
        //
        // - initialize RolloutRecorder with new or resumed session info
        // - perform default shell discovery
        // - load history metadata
        let rollout_fut = async {
            if config.ephemeral {
                Ok::<_, anyhow::Error>((None, None))
            } else {
                let state_db_ctx = state_db::init_if_enabled(&config, None).await;
                let rollout_recorder = RolloutRecorder::new(
                    &config,
                    rollout_params,
                    state_db_ctx.clone(),
                    state_builder.clone(),
                )
                .await?;
                Ok((Some(rollout_recorder), state_db_ctx))
            }
        };

        let history_meta_fut = crate::message_history::history_metadata(&config);
        let auth_manager_clone = Arc::clone(&auth_manager);
        let config_for_mcp = Arc::clone(&config);
        let auth_and_mcp_fut = async move {
            let auth = auth_manager_clone.auth().await;
            let mcp_servers = effective_mcp_servers(&config_for_mcp, auth.as_ref());
            let auth_statuses = compute_auth_statuses(
                mcp_servers.iter(),
                config_for_mcp.mcp_oauth_credentials_store_mode,
            )
            .await;
            (auth, mcp_servers, auth_statuses)
        };

        // Join all independent futures.
        let (
            rollout_recorder_and_state_db,
            (history_log_id, history_entry_count),
            (auth, mcp_servers, auth_statuses),
        ) = tokio::join!(rollout_fut, history_meta_fut, auth_and_mcp_fut);

        let (rollout_recorder, state_db_ctx) = rollout_recorder_and_state_db.map_err(|e| {
            error!("failed to initialize rollout recorder: {e:#}");
            e
        })?;
        let rollout_path = rollout_recorder
            .as_ref()
            .map(|rec| rec.rollout_path.clone());

        let mut post_session_configured_events = Vec::<Event>::new();

        for usage in config.features.legacy_feature_usages() {
            post_session_configured_events.push(Event {
                id: INITIAL_SUBMIT_ID.to_owned(),
                msg: EventMsg::DeprecationNotice(DeprecationNoticeEvent {
                    summary: usage.summary.clone(),
                    details: usage.details.clone(),
                }),
            });
        }
        if crate::config::uses_deprecated_instructions_file(&config.config_layer_stack) {
            post_session_configured_events.push(Event {
                id: INITIAL_SUBMIT_ID.to_owned(),
                msg: EventMsg::DeprecationNotice(DeprecationNoticeEvent {
                    summary: "`experimental_instructions_file` is deprecated and ignored. Use `model_instructions_file` instead."
                        .to_string(),
                    details: Some(
                        "Move the setting to `model_instructions_file` in config.toml (or under a profile) to load instructions from a file."
                            .to_string(),
                    ),
                }),
            });
        }
        for message in &config.startup_warnings {
            post_session_configured_events.push(Event {
                id: "".to_owned(),
                msg: EventMsg::Warning(WarningEvent {
                    message: message.clone(),
                }),
            });
        }
        maybe_push_unstable_features_warning(&config, &mut post_session_configured_events);
        if config.permissions.approval_policy.value() == AskForApproval::OnFailure {
            post_session_configured_events.push(Event {
                id: "".to_owned(),
                msg: EventMsg::Warning(WarningEvent {
                    message: "`on-failure` approval policy is deprecated and will be removed in a future release. Use `on-request` for interactive approvals or `never` for non-interactive runs.".to_string(),
                }),
            });
        }

        let auth = auth.as_ref();
        let auth_mode = auth.map(CodexAuth::auth_mode).map(TelemetryAuthMode::from);
        let account_id = auth.and_then(CodexAuth::get_account_id);
        let account_email = auth.and_then(CodexAuth::get_account_email);
        let originator = crate::default_client::originator().value;
        let terminal_type = terminal::user_agent();
        let session_model = session_configuration.collaboration_mode.model().to_string();
        let mut otel_manager = OtelManager::new(
            conversation_id,
            session_model.as_str(),
            session_model.as_str(),
            account_id.clone(),
            account_email.clone(),
            auth_mode,
            originator.clone(),
            config.otel.log_user_prompt,
            terminal_type.clone(),
            session_configuration.session_source.clone(),
        );
        if let Some(service_name) = session_configuration.metrics_service_name.as_deref() {
            otel_manager = otel_manager.with_metrics_service_name(service_name);
        }
        let network_proxy_audit_metadata = NetworkProxyAuditMetadata {
            conversation_id: Some(conversation_id.to_string()),
            app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            user_account_id: account_id,
            auth_mode: auth_mode.map(|mode| mode.to_string()),
            originator: Some(originator),
            user_email: account_email,
            terminal_type: Some(terminal_type),
            model: Some(session_model.clone()),
            slug: Some(session_model),
        };
        config.features.emit_metrics(&otel_manager);
        otel_manager.counter(
            "codex.thread.started",
            1,
            &[(
                "is_git",
                if get_git_repo_root(&session_configuration.cwd).is_some() {
                    "true"
                } else {
                    "false"
                },
            )],
        );

        otel_manager.conversation_starts(
            config.model_provider.name.as_str(),
            session_configuration.collaboration_mode.reasoning_effort(),
            config.model_reasoning_summary,
            config.model_context_window,
            config.model_auto_compact_token_limit,
            config.permissions.approval_policy.value(),
            config.permissions.sandbox_policy.get().clone(),
            mcp_servers.keys().map(String::as_str).collect(),
            config.active_profile.clone(),
        );

        let use_zsh_fork_shell = config.features.enabled(Feature::ShellZshFork);
        let mut default_shell = if use_zsh_fork_shell {
            let zsh_path = config.zsh_path.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "zsh fork feature enabled, but `zsh_path` is not configured; set `zsh_path` in config.toml"
                )
            })?;
            let zsh_path = zsh_path.to_path_buf();
            shell::get_shell(shell::ShellType::Zsh, Some(&zsh_path)).ok_or_else(|| {
                anyhow::anyhow!(
                    "zsh fork feature enabled, but zsh_path `{}` is not usable; set `zsh_path` to a valid zsh executable",
                    zsh_path.display()
                )
            })?
        } else {
            shell::default_user_shell()
        };
        // Create the mutable state for the Session.
        let shell_snapshot_tx = if config.features.enabled(Feature::ShellSnapshot) {
            ShellSnapshot::start_snapshotting(
                config.codex_home.clone(),
                conversation_id,
                session_configuration.cwd.clone(),
                &mut default_shell,
                otel_manager.clone(),
            )
        } else {
            let (tx, rx) = watch::channel(None);
            default_shell.shell_snapshot = rx;
            tx
        };
        let thread_name =
            match session_index::find_thread_name_by_id(&config.codex_home, &conversation_id).await
            {
                Ok(name) => name,
                Err(err) => {
                    warn!("Failed to read session index for thread name: {err}");
                    None
                }
            };
        session_configuration.thread_name = thread_name.clone();
        let state = SessionState::new(session_configuration.clone());
        let managed_network_requirements_enabled = config.managed_network_requirements_enabled();
        let network_approval = Arc::new(NetworkApprovalService::default());
        // The managed proxy can call back into core for allowlist-miss decisions.
        let network_policy_decider_session = if managed_network_requirements_enabled {
            config
                .permissions
                .network
                .as_ref()
                .map(|_| Arc::new(RwLock::new(std::sync::Weak::<Session>::new())))
        } else {
            None
        };
        let blocked_request_observer = if managed_network_requirements_enabled {
            config
                .permissions
                .network
                .as_ref()
                .map(|_| build_blocked_request_observer(Arc::clone(&network_approval)))
        } else {
            None
        };
        let network_policy_decider =
            network_policy_decider_session
                .as_ref()
                .map(|network_policy_decider_session| {
                    build_network_policy_decider(
                        Arc::clone(&network_approval),
                        Arc::clone(network_policy_decider_session),
                    )
                });
        let (network_proxy, session_network_proxy) =
            if let Some(spec) = config.permissions.network.as_ref() {
                let (network_proxy, session_network_proxy) = Self::start_managed_network_proxy(
                    spec,
                    config.permissions.sandbox_policy.get(),
                    network_policy_decider.as_ref().map(Arc::clone),
                    blocked_request_observer.as_ref().map(Arc::clone),
                    managed_network_requirements_enabled,
                    network_proxy_audit_metadata,
                )
                .await?;
                (Some(network_proxy), Some(session_network_proxy))
            } else {
                (None, None)
            };

        let services = SessionServices {
            // Initialize the MCP connection manager with an uninitialized
            // instance. It will be replaced with one created via
            // McpConnectionManager::new() once all its constructor args are
            // available. This also ensures `SessionConfigured` is emitted
            // before any MCP-related events. It is reasonable to consider
            // changing this to use Option or OnceCell, though the current
            // setup is straightforward enough and performs well.
            mcp_connection_manager: Arc::new(RwLock::new(McpConnectionManager::new_uninitialized(
                &config.permissions.approval_policy,
            ))),
            mcp_startup_cancellation_token: Mutex::new(CancellationToken::new()),
            unified_exec_manager: UnifiedExecProcessManager::new(
                config.background_terminal_max_timeout,
            ),
            shell_zsh_path: config.zsh_path.clone(),
            main_execve_wrapper_exe: config.main_execve_wrapper_exe.clone(),
            analytics_events_client: AnalyticsEventsClient::new(
                Arc::clone(&config),
                Arc::clone(&auth_manager),
            ),
            hooks: Hooks::new(HooksConfig {
                legacy_notify_argv: config.notify.clone(),
            }),
            rollout: Mutex::new(rollout_recorder),
            user_shell: Arc::new(default_shell),
            shell_snapshot_tx,
            show_raw_agent_reasoning: config.show_raw_agent_reasoning,
            exec_policy,
            auth_manager: Arc::clone(&auth_manager),
            otel_manager,
            models_manager: Arc::clone(&models_manager),
            tool_approvals: Mutex::new(ApprovalStore::default()),
            execve_session_approvals: RwLock::new(HashMap::new()),
            skills_manager,
            file_watcher,
            agent_control,
            network_proxy,
            network_approval: Arc::clone(&network_approval),
            state_db: state_db_ctx.clone(),
            model_client: ModelClient::new(
                Some(Arc::clone(&auth_manager)),
                conversation_id,
                session_configuration.provider.clone(),
                session_configuration.session_source.clone(),
                config.model_verbosity,
                ws_version_from_features(config.as_ref()),
                config.features.enabled(Feature::EnableRequestCompression),
                config.features.enabled(Feature::RuntimeMetrics),
                Self::build_model_client_beta_features_header(config.as_ref()),
            ),
        };
        let js_repl = Arc::new(JsReplHandle::with_node_path(
            config.js_repl_node_path.clone(),
            config.js_repl_node_module_dirs.clone(),
        ));

        let sess = Arc::new(Session {
            conversation_id,
            tx_event: tx_event.clone(),
            agent_status,
            state: Mutex::new(state),
            features: config.features.clone(),
            pending_mcp_server_refresh_config: Mutex::new(None),
            conversation: Arc::new(RealtimeConversationManager::new()),
            active_turn: Mutex::new(None),
            services,
            js_repl,
            next_internal_sub_id: AtomicU64::new(0),
        });
        if let Some(network_policy_decider_session) = network_policy_decider_session {
            let mut guard = network_policy_decider_session.write().await;
            *guard = Arc::downgrade(&sess);
        }
        // Dispatch the SessionConfiguredEvent first and then report any errors.
        // If resuming, include converted initial messages in the payload so UIs can render them immediately.
        let initial_messages = initial_history.get_event_msgs();
        let events = std::iter::once(Event {
            id: INITIAL_SUBMIT_ID.to_owned(),
            msg: EventMsg::SessionConfigured(SessionConfiguredEvent {
                session_id: conversation_id,
                forked_from_id,
                thread_name: session_configuration.thread_name.clone(),
                model: session_configuration.collaboration_mode.model().to_string(),
                model_provider_id: config.model_provider_id.clone(),
                approval_policy: session_configuration.approval_policy.value(),
                approvals_reviewer: session_configuration.approvals_reviewer,
                sandbox_policy: session_configuration.sandbox_policy.get().clone(),
                cwd: session_configuration.cwd.clone(),
                reasoning_effort: session_configuration.collaboration_mode.reasoning_effort(),
                history_log_id,
                history_entry_count,
                initial_messages,
                network_proxy: session_network_proxy,
                rollout_path,
            }),
        })
        .chain(post_session_configured_events.into_iter());
        for event in events {
            sess.send_event_raw(event).await;
        }

        // Start the watcher after SessionConfigured so it cannot emit earlier events.
        sess.start_file_watcher_listener();
        // Construct sandbox_state before MCP startup so it can be sent to each
        // MCP server immediately after it becomes ready (avoiding blocking).
        let sandbox_state = SandboxState {
            sandbox_policy: session_configuration.sandbox_policy.get().clone(),
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            sandbox_cwd: session_configuration.cwd.clone(),
            use_linux_sandbox_bwrap: config.features.enabled(Feature::UseLinuxSandboxBwrap),
        };
        let mut required_mcp_servers: Vec<String> = mcp_servers
            .iter()
            .filter(|(_, server)| server.enabled && server.required)
            .map(|(name, _)| name.clone())
            .collect();
        required_mcp_servers.sort();
        {
            let mut cancel_guard = sess.services.mcp_startup_cancellation_token.lock().await;
            cancel_guard.cancel();
            *cancel_guard = CancellationToken::new();
        }
        let (mcp_connection_manager, cancel_token) = McpConnectionManager::new(
            &mcp_servers,
            config.mcp_oauth_credentials_store_mode,
            auth_statuses.clone(),
            &session_configuration.approval_policy,
            tx_event.clone(),
            sandbox_state,
            config.codex_home.clone(),
            codex_apps_tools_cache_key(auth),
        )
        .await;
        {
            let mut manager_guard = sess.services.mcp_connection_manager.write().await;
            *manager_guard = mcp_connection_manager;
        }
        {
            let mut cancel_guard = sess.services.mcp_startup_cancellation_token.lock().await;
            if cancel_guard.is_cancelled() {
                cancel_token.cancel();
            }
            *cancel_guard = cancel_token;
        }
        if !required_mcp_servers.is_empty() {
            let failures = sess
                .services
                .mcp_connection_manager
                .read()
                .await
                .required_startup_failures(&required_mcp_servers)
                .await;
            if !failures.is_empty() {
                let details = failures
                    .iter()
                    .map(|failure| format!("{}: {}", failure.server, failure.error))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(anyhow::anyhow!(
                    "required MCP servers failed to initialize: {details}"
                ));
            }
        }
        sess.schedule_startup_prewarm(session_configuration.base_instructions.clone())
            .await;

        // record_initial_history can emit events. We record only after the SessionConfiguredEvent is emitted.
        sess.record_initial_history(initial_history).await;

        memories::start_memories_startup_task(
            &sess,
            Arc::clone(&config),
            &session_configuration.session_source,
        );

        Ok(sess)
    }

    pub(crate) fn get_tx_event(&self) -> Sender<Event> {
        self.tx_event.clone()
    }

    pub(crate) fn state_db(&self) -> Option<state_db::StateDbHandle> {
        self.services.state_db.clone()
    }

    /// Ensure all rollout writes are durably flushed.
    pub(crate) async fn flush_rollout(&self) {
        let recorder = {
            let guard = self.services.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.flush().await
        {
            warn!("failed to flush rollout recorder: {e}");
        }
    }

    pub(crate) async fn ensure_rollout_materialized(&self) {
        let recorder = {
            let guard = self.services.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.persist().await
        {
            warn!("failed to materialize rollout recorder: {e}");
        }
    }

    fn next_internal_sub_id(&self) -> String {
        let id = self
            .next_internal_sub_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("auto-compact-{id}")
    }

    pub(crate) async fn route_realtime_text_input(self: &Arc<Self>, text: String) {
        handlers::user_input_or_turn(
            self,
            self.next_internal_sub_id(),
            Op::UserInput {
                items: vec![UserInput::Text {
                    text,
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
            },
        )
        .await;
    }

    pub(crate) async fn get_total_token_usage(&self) -> i64 {
        let state = self.state.lock().await;
        state.get_total_token_usage(state.server_reasoning_included())
    }

    pub(crate) async fn get_total_token_usage_breakdown(&self) -> TotalTokenUsageBreakdown {
        let state = self.state.lock().await;
        state.history.get_total_token_usage_breakdown()
    }

    pub(crate) async fn total_token_usage(&self) -> Option<TokenUsage> {
        let state = self.state.lock().await;
        state.token_info().map(|info| info.total_token_usage)
    }

    pub(crate) async fn get_estimated_token_count(
        &self,
        turn_context: &TurnContext,
    ) -> Option<i64> {
        let state = self.state.lock().await;
        state.history.estimate_token_count(turn_context)
    }

    pub(crate) async fn get_base_instructions(&self) -> BaseInstructions {
        let state = self.state.lock().await;
        BaseInstructions {
            text: state.session_configuration.base_instructions.clone(),
        }
    }

    pub(crate) async fn merge_mcp_tool_selection(&self, tool_names: Vec<String>) -> Vec<String> {
        let mut state = self.state.lock().await;
        state.merge_mcp_tool_selection(tool_names)
    }

    pub(crate) async fn set_mcp_tool_selection(&self, tool_names: Vec<String>) {
        let mut state = self.state.lock().await;
        state.set_mcp_tool_selection(tool_names);
    }

    pub(crate) async fn get_mcp_tool_selection(&self) -> Option<Vec<String>> {
        let state = self.state.lock().await;
        state.get_mcp_tool_selection()
    }

    pub(crate) async fn clear_mcp_tool_selection(&self) {
        let mut state = self.state.lock().await;
        state.clear_mcp_tool_selection();
    }

    pub(crate) async fn set_auto_model_sub_selection(&self, model_sub: Option<String>) {
        let mut state = self.state.lock().await;
        state.set_auto_model_sub_selection(model_sub);
    }

    pub(crate) async fn get_auto_model_sub_selection(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.get_auto_model_sub_selection()
    }

    pub(crate) async fn set_auto_model_sub_calibration_attempted(&self, attempted: bool) {
        let mut state = self.state.lock().await;
        state.set_auto_model_sub_calibration_attempted(attempted);
    }

    pub(crate) async fn get_auto_model_sub_calibration_attempted(&self) -> bool {
        let state = self.state.lock().await;
        state.get_auto_model_sub_calibration_attempted()
    }

    pub(crate) async fn set_last_model_sub_calibration_models(&self, models: Vec<String>) {
        let mut state = self.state.lock().await;
        state.set_last_model_sub_calibration_models(models);
    }

    pub(crate) async fn get_last_model_sub_calibration_models(&self) -> Vec<String> {
        let state = self.state.lock().await;
        state.get_last_model_sub_calibration_models()
    }

    pub(crate) async fn set_last_model_sub_calibration_recommended_for_session(
        &self,
        model: Option<String>,
    ) {
        let mut state = self.state.lock().await;
        state.set_last_model_sub_calibration_recommended_for_session(model);
    }

    pub(crate) async fn get_last_model_sub_calibration_recommended_for_session(
        &self,
    ) -> Option<String> {
        let state = self.state.lock().await;
        state.get_last_model_sub_calibration_recommended_for_session()
    }

    // Merges connector IDs into the session-level explicit connector selection.
    pub(crate) async fn merge_connector_selection(
        &self,
        connector_ids: HashSet<String>,
    ) -> HashSet<String> {
        let mut state = self.state.lock().await;
        state.merge_connector_selection(connector_ids)
    }

    // Returns the connector IDs currently selected for this session.
    pub(crate) async fn get_connector_selection(&self) -> HashSet<String> {
        let state = self.state.lock().await;
        state.get_connector_selection()
    }

    // Clears connector IDs that were accumulated for explicit selection.
    pub(crate) async fn clear_connector_selection(&self) {
        let mut state = self.state.lock().await;
        state.clear_connector_selection();
    }

    async fn record_initial_history(&self, conversation_history: InitialHistory) {
        let turn_context = self.new_default_turn().await;
        self.clear_mcp_tool_selection().await;
        match conversation_history {
            InitialHistory::New => {
                // Build and record initial items (user instructions + environment context)
                // TODO(ccunningham): Defer initial context insertion until the first real turn
                // starts so it reflects the actual first-turn settings (permissions, etc.) and
                // we do not emit model-visible "diff" updates before the first user message.
                let items = self.build_initial_context(&turn_context, None).await;
                self.record_conversation_items(&turn_context, &items).await;
                {
                    let mut state = self.state.lock().await;
                    state.set_reference_context_item(Some(turn_context.to_turn_context_item()));
                }
                self.set_previous_model(None).await;
                // Ensure initial items are visible to immediate readers (e.g., tests, forks).
                self.flush_rollout().await;
            }
            InitialHistory::Resumed(resumed_history) => {
                let rollout_items = resumed_history.history;
                let restored_tool_selection =
                    Self::extract_mcp_tool_selection_from_rollout(&rollout_items);
                let (previous_regular_turn_context_item, crossed_compaction_after_turn) =
                    Self::last_rollout_regular_turn_context_lookup(&rollout_items);
                let previous_model =
                    previous_regular_turn_context_item.map(|ctx| ctx.model.clone());
                let curr = turn_context.model_info.slug.as_str();
                let reference_context_item = if !crossed_compaction_after_turn {
                    previous_regular_turn_context_item.cloned()
                } else {
                    // Keep the baseline empty when compaction may have stripped the referenced
                    // context diffs so the first resumed regular turn fully reinjects context.
                    None
                };
                {
                    let mut state = self.state.lock().await;
                    state.set_reference_context_item(reference_context_item);
                }
                self.set_previous_model(previous_model.clone()).await;

                // If resuming, warn when the last recorded model differs from the current one.
                if let Some(prev) = previous_model.as_deref().filter(|p| *p != curr) {
                    warn!("resuming session with different model: previous={prev}, current={curr}");
                    self.send_event(
                        &turn_context,
                        EventMsg::Warning(WarningEvent {
                            message: format!(
                                "This session was recorded with model `{prev}` but is resuming with `{curr}`. \
                         Consider switching back to `{prev}` as it may affect Codex performance."
                            ),
                        }),
                    )
                    .await;
                }

                // Always add response items to conversation history
                let reconstructed_history = self
                    .reconstruct_history_from_rollout(&turn_context, &rollout_items)
                    .await;
                if !reconstructed_history.is_empty() {
                    self.record_into_history(&reconstructed_history, &turn_context)
                        .await;
                }

                // Seed usage info from the recorded rollout so UIs can show token counts
                // immediately on resume/fork.
                if let Some(info) = Self::last_token_info_from_rollout(&rollout_items) {
                    let mut state = self.state.lock().await;
                    state.set_token_info(Some(info));
                }
                if let Some(selected_tools) = restored_tool_selection {
                    self.set_mcp_tool_selection(selected_tools).await;
                }

                // Defer seeding the session's initial context until the first turn starts so
                // turn/start overrides can be merged before we write to the rollout.
                self.flush_rollout().await;
            }
            InitialHistory::Forked(rollout_items) => {
                let restored_tool_selection =
                    Self::extract_mcp_tool_selection_from_rollout(&rollout_items);
                let (previous_regular_turn_context_item, _) =
                    Self::last_rollout_regular_turn_context_lookup(&rollout_items);
                let previous_model =
                    previous_regular_turn_context_item.map(|ctx| ctx.model.clone());
                self.set_previous_model(previous_model).await;

                // Always add response items to conversation history
                let reconstructed_history = self
                    .reconstruct_history_from_rollout(&turn_context, &rollout_items)
                    .await;
                if !reconstructed_history.is_empty() {
                    self.record_into_history(&reconstructed_history, &turn_context)
                        .await;
                }

                // Seed usage info from the recorded rollout so UIs can show token counts
                // immediately on resume/fork.
                if let Some(info) = Self::last_token_info_from_rollout(&rollout_items) {
                    let mut state = self.state.lock().await;
                    state.set_token_info(Some(info));
                }
                if let Some(selected_tools) = restored_tool_selection {
                    self.set_mcp_tool_selection(selected_tools).await;
                }

                // If persisting, persist all rollout items as-is (recorder filters)
                if !rollout_items.is_empty() {
                    self.persist_rollout_items(&rollout_items).await;
                }

                // Append the current session's initial context after the reconstructed history.
                let initial_context = self.build_initial_context(&turn_context, None).await;
                self.record_conversation_items(&turn_context, &initial_context)
                    .await;
                {
                    let mut state = self.state.lock().await;
                    state.set_reference_context_item(Some(turn_context.to_turn_context_item()));
                }

                // Forked threads should remain file-backed immediately after startup.
                self.ensure_rollout_materialized().await;

                // Flush after seeding history and any persisted rollout copy.
                self.flush_rollout().await;
            }
        }
    }

    /// Returns `(last_turn_context_item, crossed_compaction_after_turn)` from the
    /// rollback-adjusted rollout view.
    ///
    /// This relies on the invariant that only regular turns persist `TurnContextItem`.
    /// `ThreadRolledBack` markers are applied so resume/fork uses the post-rollback history view.
    ///
    /// Returns `(None, false)` when no persisted `TurnContextItem` can be found.
    ///
    /// Older/minimal rollouts may only contain `RolloutItem::TurnContext` entries without turn
    /// lifecycle events. In that case we fall back to the last `TurnContextItem` (plus whether a
    /// later `Compacted` item appears in rollout order).
    // TODO(ccunningham): Simplify this lookup by sharing rollout traversal/rollback application
    // with `reconstruct_history_from_rollout` so resume/fork baseline hydration does not need a
    // second bespoke rollout scan.
    fn last_rollout_regular_turn_context_lookup(
        rollout_items: &[RolloutItem],
    ) -> (Option<&TurnContextItem>, bool) {
        // Reverse scan over rollout items. `ThreadRolledBack(num_turns)` is naturally handled by
        // skipping the next `num_turns` completed turn spans we encounter while walking backward.
        //
        // "Active turn" here means: we have seen `TurnComplete`/`TurnAborted` and are currently
        // scanning backward through that completed turn until its matching `TurnStarted`.
        let mut turns_to_skip_due_to_rollback = 0usize;
        let mut saw_surviving_compaction_after_candidate = false;
        let mut saw_turn_lifecycle_event = false;
        let mut active_turn_id: Option<&str> = None;
        let mut active_turn_saw_user_message = false;
        let mut active_turn_context: Option<&TurnContextItem> = None;
        let mut active_turn_contains_compaction = false;

        for item in rollout_items.iter().rev() {
            match item {
                RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                    // Rollbacks count completed turns, not `TurnContextItem`s. We must continue
                    // ignoring all items inside each skipped turn until we reach its
                    // corresponding `TurnStarted`.
                    let num_turns = usize::try_from(rollback.num_turns).unwrap_or(usize::MAX);
                    turns_to_skip_due_to_rollback =
                        turns_to_skip_due_to_rollback.saturating_add(num_turns);
                }
                RolloutItem::EventMsg(EventMsg::TurnComplete(event)) => {
                    saw_turn_lifecycle_event = true;
                    // Enter the reverse "turn span" for this completed turn.
                    active_turn_id = Some(event.turn_id.as_str());
                    active_turn_saw_user_message = false;
                    active_turn_context = None;
                    active_turn_contains_compaction = false;
                }
                RolloutItem::EventMsg(EventMsg::TurnAborted(event)) => {
                    saw_turn_lifecycle_event = true;
                    // Same reverse-turn handling as `TurnComplete`. Some aborted turns may not
                    // have a turn id; in that case we cannot match `TurnContextItem`s to them.
                    active_turn_id = event.turn_id.as_deref();
                    active_turn_saw_user_message = false;
                    active_turn_context = None;
                    active_turn_contains_compaction = false;
                }
                RolloutItem::EventMsg(EventMsg::UserMessage(_)) => {
                    if active_turn_id.is_some() {
                        active_turn_saw_user_message = true;
                    }
                }
                RolloutItem::EventMsg(EventMsg::TurnStarted(event)) => {
                    saw_turn_lifecycle_event = true;
                    if active_turn_id == Some(event.turn_id.as_str()) {
                        let active_turn_is_rolled_back =
                            active_turn_saw_user_message && turns_to_skip_due_to_rollback > 0;
                        if active_turn_is_rolled_back {
                            // `ThreadRolledBack(num_turns)` counts user turns, so only consume a
                            // skip once we've confirmed this reverse-scanned turn span contains a
                            // user message. Standalone task turns must not consume rollback skips.
                            turns_to_skip_due_to_rollback -= 1;
                        }
                        if !active_turn_is_rolled_back {
                            if let Some(context_item) = active_turn_context {
                                return (
                                    Some(context_item),
                                    saw_surviving_compaction_after_candidate,
                                );
                            }
                            // No `TurnContextItem` in this surviving turn; keep scanning older
                            // turns, but remember if this turn compacted so the eventual
                            // candidate reports "compaction happened after it".
                            if active_turn_contains_compaction {
                                saw_surviving_compaction_after_candidate = true;
                            }
                        }
                        active_turn_id = None;
                        active_turn_saw_user_message = false;
                        active_turn_context = None;
                        active_turn_contains_compaction = false;
                    }
                }
                RolloutItem::TurnContext(ctx) => {
                    // Capture the latest turn context seen in this reverse-scanned turn span. If
                    // the turn later proves to be rolled back, we discard it when we hit the
                    // matching `TurnStarted`. Older rollouts may have lifecycle events but omit
                    // `TurnContextItem.turn_id`; accept those as belonging to the active turn
                    // span for resume/fork hydration.
                    if let Some(active_id) = active_turn_id
                        && ctx
                            .turn_id
                            .as_deref()
                            .is_none_or(|turn_id| turn_id == active_id)
                    {
                        // Reverse scan sees the latest `TurnContextItem` for the turn first.
                        active_turn_context.get_or_insert(ctx);
                    }
                }
                RolloutItem::Compacted(_) => {
                    if active_turn_id.is_some() {
                        // Compaction inside the currently scanned turn is only "after" the
                        // eventual candidate if this turn has no `TurnContextItem` and we keep
                        // scanning into older turns.
                        active_turn_contains_compaction = true;
                    } else {
                        saw_surviving_compaction_after_candidate = true;
                    }
                }
                _ => {}
            }
        }

        // Legacy/minimal rollouts may only persist `TurnContextItem`/`Compacted` without turn
        // lifecycle events. Fall back to the last `TurnContextItem` in rollout order so
        // resume/fork can still hydrate `previous_model` and detect compaction-after-baseline.
        if !saw_turn_lifecycle_event {
            let mut saw_compaction_after_last_turn_context = false;
            for item in rollout_items.iter().rev() {
                match item {
                    RolloutItem::Compacted(_) => {
                        saw_compaction_after_last_turn_context = true;
                    }
                    RolloutItem::TurnContext(ctx) => {
                        return (Some(ctx), saw_compaction_after_last_turn_context);
                    }
                    _ => {}
                }
            }
        }

        (None, false)
    }

    fn last_token_info_from_rollout(rollout_items: &[RolloutItem]) -> Option<TokenUsageInfo> {
        rollout_items.iter().rev().find_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::TokenCount(ev)) => ev.info.clone(),
            _ => None,
        })
    }

    fn extract_mcp_tool_selection_from_rollout(
        rollout_items: &[RolloutItem],
    ) -> Option<Vec<String>> {
        let mut search_call_ids = HashSet::new();
        let mut active_selected_tools: Option<Vec<String>> = None;

        for item in rollout_items {
            let RolloutItem::ResponseItem(response_item) = item else {
                continue;
            };
            match response_item {
                ResponseItem::FunctionCall { name, call_id, .. } => {
                    if name == SEARCH_TOOL_BM25_TOOL_NAME {
                        search_call_ids.insert(call_id.clone());
                    }
                }
                ResponseItem::FunctionCallOutput { call_id, output } => {
                    if !search_call_ids.contains(call_id) {
                        continue;
                    }
                    let Some(content) = output.body.to_text() else {
                        continue;
                    };
                    let Ok(payload) = serde_json::from_str::<Value>(&content) else {
                        continue;
                    };
                    let Some(selected_tools) = payload
                        .get("active_selected_tools")
                        .and_then(Value::as_array)
                    else {
                        continue;
                    };
                    let Some(selected_tools) = selected_tools
                        .iter()
                        .map(|value| value.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    active_selected_tools = Some(selected_tools);
                }
                _ => {}
            }
        }

        active_selected_tools
    }

    async fn previous_model(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.previous_model()
    }

    pub(crate) async fn set_previous_model(&self, previous_model: Option<String>) {
        let mut state = self.state.lock().await;
        state.set_previous_model(previous_model);
    }

    fn maybe_refresh_shell_snapshot_for_cwd(
        &self,
        previous_cwd: &Path,
        next_cwd: &Path,
        codex_home: &Path,
    ) {
        if previous_cwd == next_cwd {
            return;
        }

        if !self.features.enabled(Feature::ShellSnapshot) {
            return;
        }

        ShellSnapshot::refresh_snapshot(
            codex_home.to_path_buf(),
            self.conversation_id,
            next_cwd.to_path_buf(),
            self.services.user_shell.as_ref().clone(),
            self.services.shell_snapshot_tx.clone(),
            self.services.otel_manager.clone(),
        );
    }

    pub(crate) async fn update_settings(
        &self,
        updates: SessionSettingsUpdate,
    ) -> ConstraintResult<()> {
        let (previous_cwd, next_cwd, codex_home, provider_label, dropped_items_count) = {
            let mut state = self.state.lock().await;

            match state.session_configuration.apply(&updates) {
                Ok((updated, provider_label)) => {
                    let previous_cwd = state.session_configuration.cwd.clone();
                    let next_cwd = updated.cwd.clone();
                    let codex_home = updated.codex_home.clone();
                    state.session_configuration = updated;
                    let dropped_items_count = if provider_label.is_some() {
                        drop_provider_specific_encrypted_history_items(&mut state)
                    } else {
                        0
                    };
                    (
                        previous_cwd,
                        next_cwd,
                        codex_home,
                        provider_label,
                        dropped_items_count,
                    )
                }
                Err(err) => {
                    warn!("rejected session settings update: {err}");
                    return Err(err);
                }
            }
        };

        self.maybe_refresh_shell_snapshot_for_cwd(&previous_cwd, &next_cwd, &codex_home);

        if dropped_items_count > 0 {
            tracing::info!(
                dropped_items_count,
                "provider switch: dropped encrypted reasoning items from history"
            );
        }

        if let Some(label) = provider_label {
            self.send_event_raw(Event {
                id: self.next_internal_sub_id(),
                msg: EventMsg::BackgroundEvent(BackgroundEventEvent {
                    message: format!("Provider: {label}"),
                }),
            })
            .await;
        }

        Ok(())
    }

    pub(crate) async fn new_turn_with_sub_id(
        &self,
        sub_id: String,
        updates: SessionSettingsUpdate,
    ) -> ConstraintResult<Arc<TurnContext>> {
        let (
            session_configuration,
            sandbox_policy_changed,
            previous_cwd,
            codex_home,
            provider_label,
            dropped_items_count,
        ) = {
            let mut state = self.state.lock().await;
            match state.session_configuration.clone().apply(&updates) {
                Ok((next, provider_label)) => {
                    let previous_cwd = state.session_configuration.cwd.clone();
                    let sandbox_policy_changed =
                        state.session_configuration.sandbox_policy != next.sandbox_policy;
                    let codex_home = next.codex_home.clone();
                    state.session_configuration = next.clone();
                    let dropped_items_count = if provider_label.is_some() {
                        drop_provider_specific_encrypted_history_items(&mut state)
                    } else {
                        0
                    };
                    (
                        next,
                        sandbox_policy_changed,
                        previous_cwd,
                        codex_home,
                        provider_label,
                        dropped_items_count,
                    )
                }
                Err(err) => {
                    drop(state);
                    self.send_event_raw(Event {
                        id: sub_id.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: err.to_string(),
                            codex_error_info: Some(CodexErrorInfo::BadRequest),
                        }),
                    })
                    .await;
                    return Err(err);
                }
            }
        };

        self.maybe_refresh_shell_snapshot_for_cwd(
            &previous_cwd,
            &session_configuration.cwd,
            &codex_home,
        );

        let turn_context = self
            .new_turn_from_configuration(
                sub_id,
                session_configuration,
                updates.final_output_json_schema,
                sandbox_policy_changed,
            )
            .await;

        if dropped_items_count > 0 {
            tracing::info!(
                dropped_items_count,
                "provider switch: dropped encrypted reasoning items from history"
            );
        }

        if let Some(label) = provider_label {
            let base_url = turn_context
                .provider
                .base_url
                .as_deref()
                .unwrap_or("(default)");
            let label = if turn_context.provider.account_pool.is_empty() {
                format!("{label} @ {base_url}")
            } else {
                format!(
                    "{label} [{}] @ {base_url}",
                    account_index_label(&turn_context.provider)
                )
            };
            self.notify_background_event(&turn_context, format!("Provider: {label}"))
                .await;
        }

        Ok(turn_context)
    }

    async fn new_turn_from_configuration(
        &self,
        sub_id: String,
        session_configuration: SessionConfiguration,
        final_output_json_schema: Option<Option<Value>>,
        sandbox_policy_changed: bool,
    ) -> Arc<TurnContext> {
        let resolved_provider = {
            let mut state = self.state.lock().await;
            resolve_turn_provider_from_pool(
                &mut state,
                &session_configuration.provider_id,
                &session_configuration.provider,
                std::time::Instant::now(),
            )
        };
        let background_message = resolved_provider.background_message.clone();
        // Box this nested async call so startup/resume turn creation does not
        // inline the full future chain onto a small test-thread stack.
        let turn_context = Box::pin(self.new_turn_from_resolved_provider(
            sub_id,
            session_configuration,
            resolved_provider.provider,
            final_output_json_schema,
            sandbox_policy_changed,
        ))
        .await;
        if let Some(message) = background_message {
            self.notify_background_event(&turn_context, message).await;
        }
        turn_context
    }

    async fn new_turn_from_resolved_provider(
        &self,
        sub_id: String,
        session_configuration: SessionConfiguration,
        provider: ModelProviderInfo,
        final_output_json_schema: Option<Option<Value>>,
        sandbox_policy_changed: bool,
    ) -> Arc<TurnContext> {
        let per_turn_config = Self::build_per_turn_config(&session_configuration);
        self.services
            .mcp_connection_manager
            .read()
            .await
            .set_approval_policy(&session_configuration.approval_policy);

        if sandbox_policy_changed {
            let sandbox_state = SandboxState {
                sandbox_policy: per_turn_config.permissions.sandbox_policy.get().clone(),
                codex_linux_sandbox_exe: per_turn_config.codex_linux_sandbox_exe.clone(),
                sandbox_cwd: per_turn_config.cwd.clone(),
                use_linux_sandbox_bwrap: per_turn_config
                    .features
                    .enabled(Feature::UseLinuxSandboxBwrap),
            };
            if let Err(e) = self
                .services
                .mcp_connection_manager
                .read()
                .await
                .notify_sandbox_state_change(&sandbox_state)
                .await
            {
                warn!("Failed to notify sandbox state change to MCP servers: {e:#}");
            }
        }

        let model_info = self
            .services
            .models_manager
            .get_model_info(
                session_configuration.collaboration_mode.model(),
                &per_turn_config,
            )
            .await;
        let skills_outcome = Arc::new(
            self.services
                .skills_manager
                .skills_for_cwd(&session_configuration.cwd, false)
                .await,
        );
        let mut turn_context: TurnContext = Self::make_turn_context(
            Some(Arc::clone(&self.services.auth_manager)),
            &self.services.otel_manager,
            provider,
            &session_configuration,
            per_turn_config,
            model_info,
            self.services
                .network_proxy
                .as_ref()
                .map(StartedNetworkProxy::proxy),
            sub_id,
            Arc::clone(&self.js_repl),
            skills_outcome,
        );

        if let Some(final_schema) = final_output_json_schema {
            turn_context.final_output_json_schema = final_schema;
        }
        let turn_context = Arc::new(turn_context);
        turn_context.turn_metadata_state.spawn_git_enrichment_task();
        turn_context
    }

    pub(crate) async fn maybe_emit_unknown_model_warning_for_turn(&self, tc: &TurnContext) {
        if tc.model_info.used_fallback_model_metadata {
            self.send_event(
                tc,
                EventMsg::Warning(WarningEvent {
                    message: format!(
                        "Model metadata for `{}` not found. Defaulting to fallback metadata; this can degrade performance and cause issues.",
                        tc.model_info.slug
                    ),
                }),
            )
            .await;
        }
    }

    pub(crate) async fn new_default_turn(&self) -> Arc<TurnContext> {
        self.new_default_turn_with_sub_id(self.next_internal_sub_id())
            .await
    }

    pub(crate) async fn take_startup_regular_task(&self) -> Option<RegularTask> {
        let startup_regular_task = {
            let mut state = self.state.lock().await;
            state.take_startup_regular_task()
        };
        let startup_regular_task = startup_regular_task?;
        match startup_regular_task.await {
            Ok(Ok(regular_task)) => Some(regular_task),
            Ok(Err(err)) => {
                warn!("startup websocket prewarm setup failed: {err:#}");
                None
            }
            Err(err) => {
                warn!("startup websocket prewarm setup join failed: {err}");
                None
            }
        }
    }

    async fn schedule_startup_prewarm(self: &Arc<Self>, base_instructions: String) {
        let sess = Arc::clone(self);
        let startup_regular_task: JoinHandle<CodexResult<RegularTask>> =
            tokio::spawn(
                async move { sess.schedule_startup_prewarm_inner(base_instructions).await },
            );
        let mut state = self.state.lock().await;
        state.set_startup_regular_task(startup_regular_task);
    }

    async fn schedule_startup_prewarm_inner(
        self: &Arc<Self>,
        base_instructions: String,
    ) -> CodexResult<RegularTask> {
        let startup_turn_context = self
            .new_default_turn_with_sub_id(INITIAL_SUBMIT_ID.to_owned())
            .await;
        let startup_cancellation_token = CancellationToken::new();
        let startup_router = built_tools(
            self,
            startup_turn_context.as_ref(),
            &[],
            &HashSet::new(),
            None,
            &startup_cancellation_token,
        )
        .await?;
        let startup_prompt = build_prompt(
            Vec::new(),
            startup_router.as_ref(),
            startup_turn_context.as_ref(),
            BaseInstructions {
                text: base_instructions,
            },
            Vec::new(),
            None,
            None,
        );
        let startup_turn_metadata_header = startup_turn_context
            .turn_metadata_state
            .current_header_value();
        RegularTask::with_startup_prewarm(
            self.services.model_client.clone(),
            startup_prompt,
            startup_turn_context,
            startup_turn_metadata_header,
        )
        .await
    }

    pub(crate) async fn get_config(&self) -> std::sync::Arc<Config> {
        let state = self.state.lock().await;
        state
            .session_configuration
            .original_config_do_not_use
            .clone()
    }

    pub(crate) async fn provider(&self) -> ModelProviderInfo {
        let mut state = self.state.lock().await;
        let provider_id = state.session_configuration.provider_id.clone();
        let provider = state.session_configuration.provider.clone();
        resolve_turn_provider_from_pool(
            &mut state,
            &provider_id,
            &provider,
            std::time::Instant::now(),
        )
        .provider
    }

    pub(crate) async fn utility_client_and_model_for_slug(
        &self,
        config: &Config,
        model_slug: &str,
    ) -> Option<(ModelClient, ModelInfo, String)> {
        let (provider_id, logical_provider) =
            crate::utility_model::provider_for_model_slug(config, model_slug)?;
        let model_info = self
            .services
            .models_manager
            .get_model_info(model_slug, config)
            .await;
        let resolved_provider = {
            let mut state = self.state.lock().await;
            resolve_turn_provider_from_pool(
                &mut state,
                &provider_id,
                &logical_provider,
                std::time::Instant::now(),
            )
        };
        let model_client = self
            .services
            .model_client
            .clone_with_provider(resolved_provider.provider);
        Some((model_client, model_info, provider_id))
    }

    async fn entire_summary_client_and_model_for_turn(
        &self,
        turn_context: &TurnContext,
    ) -> (ModelClient, ModelInfo, String, Option<String>) {
        let model_slug =
            crate::entire_summary_generator::model_slug(turn_context.config.as_ref()).to_string();
        let (summary_turn_context, background_message) = self
            .turn_context_with_model_resolved_from_pool(turn_context, model_slug.clone())
            .await;
        let model_client = self
            .services
            .model_client
            .clone_with_provider(summary_turn_context.provider.clone());
        (
            model_client,
            summary_turn_context.model_info,
            model_slug,
            background_message,
        )
    }

    async fn turn_context_with_model_resolved_from_pool(
        &self,
        turn_context: &TurnContext,
        model: String,
    ) -> (TurnContext, Option<String>) {
        let mut next_turn_context = turn_context
            .with_model(model, &self.services.models_manager)
            .await;
        let resolved_provider = {
            let mut state = self.state.lock().await;
            resolve_turn_provider_from_pool(
                &mut state,
                &next_turn_context.config.model_provider_id,
                &next_turn_context.config.model_provider,
                std::time::Instant::now(),
            )
        };
        next_turn_context.provider = resolved_provider.provider;
        (next_turn_context, resolved_provider.background_message)
    }

    pub(crate) async fn reload_user_config_layer(&self) {
        let config_toml_path = {
            let state = self.state.lock().await;
            state
                .session_configuration
                .codex_home
                .join(CONFIG_TOML_FILE)
        };

        let user_config = match std::fs::read_to_string(&config_toml_path) {
            Ok(contents) => match toml::from_str::<toml::Value>(&contents) {
                Ok(config) => config,
                Err(err) => {
                    warn!("failed to parse user config while reloading layer: {err}");
                    return;
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                toml::Value::Table(Default::default())
            }
            Err(err) => {
                warn!("failed to read user config while reloading layer: {err}");
                return;
            }
        };

        let config_toml_path = match AbsolutePathBuf::try_from(config_toml_path) {
            Ok(path) => path,
            Err(err) => {
                warn!("failed to resolve user config path while reloading layer: {err}");
                return;
            }
        };

        let mut state = self.state.lock().await;
        let mut config = (*state.session_configuration.original_config_do_not_use).clone();
        config.config_layer_stack = config
            .config_layer_stack
            .with_user_config(&config_toml_path, user_config);

        let merged_toml = config.config_layer_stack.effective_config();
        if let Ok(config_toml) =
            crate::config::deserialize_config_toml_with_base(merged_toml, &config.codex_home)
        {
            let active_profile = config
                .active_profile
                .clone()
                .or_else(|| config_toml.profile.clone());
            let profile = active_profile
                .as_ref()
                .and_then(|name| config_toml.profiles.get(name));

            let normalized = |value: Option<String>| {
                value.and_then(|value| {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
            };

            config.model_sub = normalized(
                profile
                    .and_then(|entry| entry.model_sub.clone())
                    .or(config_toml.model_sub),
            );
            config.model_sub_responses = normalized(
                profile
                    .and_then(|entry| entry.model_sub_responses.clone())
                    .or(config_toml.model_sub_responses),
            );
        }

        state.session_configuration.original_config_do_not_use = Arc::new(config);
    }

    pub(crate) async fn new_default_turn_with_sub_id(&self, sub_id: String) -> Arc<TurnContext> {
        let session_configuration = {
            let state = self.state.lock().await;
            state.session_configuration.clone()
        };
        Box::pin(self.new_turn_from_configuration(sub_id, session_configuration, None, false)).await
    }

    fn build_settings_update_items(
        &self,
        reference_context_item: Option<&TurnContextItem>,
        previous_user_turn_model: Option<&str>,
        current_context: &TurnContext,
    ) -> Vec<ResponseItem> {
        // TODO: Make context updates a pure diff of persisted previous/current TurnContextItem
        // state so replay/backtracking is deterministic. Runtime inputs that affect model-visible
        // context (shell, exec policy, feature gates, previous-model bridge) should be persisted
        // state or explicit non-state replay events.
        let shell = self.user_shell();
        let exec_policy = self.services.exec_policy.current();
        crate::context_manager::updates::build_settings_update_items(
            reference_context_item,
            previous_user_turn_model,
            current_context,
            shell.as_ref(),
            exec_policy.as_ref(),
            self.features.enabled(Feature::Personality),
        )
    }

    /// Persist the event to rollout and send it to clients.
    pub(crate) async fn send_event(&self, turn_context: &TurnContext, msg: EventMsg) {
        let legacy_source = msg.clone();
        let event = Event {
            id: turn_context.sub_id.clone(),
            msg,
        };
        self.send_event_raw(event).await;
        self.maybe_mirror_event_text_to_realtime(&legacy_source)
            .await;

        let show_raw_agent_reasoning = self.show_raw_agent_reasoning();
        for legacy in legacy_source.as_legacy_events(show_raw_agent_reasoning) {
            let legacy_event = Event {
                id: turn_context.sub_id.clone(),
                msg: legacy,
            };
            self.send_event_raw(legacy_event).await;
        }
    }

    async fn maybe_mirror_event_text_to_realtime(&self, msg: &EventMsg) {
        let Some(text) = realtime_text_for_event(msg) else {
            return;
        };
        if self.conversation.running_state().await.is_none() {
            return;
        }
        if let Err(err) = self.conversation.text_in(text).await {
            debug!("failed to mirror event text to realtime conversation: {err}");
        }
    }

    pub(crate) async fn send_event_raw(&self, event: Event) {
        // Record the last known agent status.
        if let Some(status) = agent_status_from_event(&event.msg) {
            self.agent_status.send_replace(status);
        }
        // Persist the event into rollout (recorder filters as needed)
        let rollout_items = vec![RolloutItem::EventMsg(event.msg.clone())];
        self.persist_rollout_items(&rollout_items).await;
        if let Err(e) = self.tx_event.send(event).await {
            debug!("dropping event because channel is closed: {e}");
        }
    }

    /// Persist the event to the rollout file, flush it, and only then deliver it to clients.
    ///
    /// Most events can be delivered immediately after queueing the rollout write, but some
    /// clients (e.g. app-server thread/rollback) re-read the rollout file synchronously on
    /// receipt of the event and depend on the marker already being visible on disk.
    pub(crate) async fn send_event_raw_flushed(&self, event: Event) {
        // Record the last known agent status.
        if let Some(status) = agent_status_from_event(&event.msg) {
            self.agent_status.send_replace(status);
        }
        self.persist_rollout_items(&[RolloutItem::EventMsg(event.msg.clone())])
            .await;
        self.flush_rollout().await;
        if let Err(e) = self.tx_event.send(event).await {
            debug!("dropping event because channel is closed: {e}");
        }
    }

    pub(crate) async fn emit_turn_item_started(&self, turn_context: &TurnContext, item: &TurnItem) {
        self.send_event(
            turn_context,
            EventMsg::ItemStarted(ItemStartedEvent {
                thread_id: self.conversation_id,
                turn_id: turn_context.sub_id.clone(),
                item: item.clone(),
            }),
        )
        .await;
    }

    pub(crate) async fn emit_turn_item_completed(
        &self,
        turn_context: &TurnContext,
        item: TurnItem,
    ) {
        self.send_event(
            turn_context,
            EventMsg::ItemCompleted(ItemCompletedEvent {
                thread_id: self.conversation_id,
                turn_id: turn_context.sub_id.clone(),
                item,
            }),
        )
        .await;
    }

    /// Adds an execpolicy amendment to both the in-memory and on-disk policies so future
    /// commands can use the newly approved prefix.
    pub(crate) async fn persist_execpolicy_amendment(
        &self,
        amendment: &ExecPolicyAmendment,
    ) -> Result<(), ExecPolicyUpdateError> {
        let codex_home = self
            .state
            .lock()
            .await
            .session_configuration
            .codex_home()
            .clone();

        self.services
            .exec_policy
            .append_amendment_and_update(&codex_home, amendment)
            .await?;

        Ok(())
    }

    pub(crate) async fn turn_context_for_sub_id(&self, sub_id: &str) -> Option<Arc<TurnContext>> {
        let active = self.active_turn.lock().await;
        active
            .as_ref()
            .and_then(|turn| turn.tasks.get(sub_id))
            .map(|task| Arc::clone(&task.turn_context))
    }

    async fn active_turn_context_and_cancellation_token(
        &self,
    ) -> Option<(Arc<TurnContext>, CancellationToken)> {
        let active = self.active_turn.lock().await;
        let (_, task) = active.as_ref()?.tasks.first()?;
        Some((
            Arc::clone(&task.turn_context),
            task.cancellation_token.child_token(),
        ))
    }

    pub(crate) async fn record_execpolicy_amendment_message(
        &self,
        sub_id: &str,
        amendment: &ExecPolicyAmendment,
    ) {
        let Some(prefixes) = format_allow_prefixes(vec![amendment.command.clone()]) else {
            warn!("execpolicy amendment for {sub_id} had no command prefix");
            return;
        };
        let text = format!("Approved command prefix saved:\n{prefixes}");
        let message: ResponseItem = DeveloperInstructions::new(text.clone()).into();

        if let Some(turn_context) = self.turn_context_for_sub_id(sub_id).await {
            self.record_conversation_items(&turn_context, std::slice::from_ref(&message))
                .await;
            return;
        }

        if self
            .inject_response_items(vec![ResponseInputItem::Message {
                role: "developer".to_string(),
                content: vec![ContentItem::InputText { text }],
            }])
            .await
            .is_err()
        {
            warn!("no active turn found to record execpolicy amendment message for {sub_id}");
        }
    }

    pub(crate) async fn persist_network_policy_amendment(
        &self,
        amendment: &NetworkPolicyAmendment,
        network_approval_context: &NetworkApprovalContext,
    ) -> anyhow::Result<()> {
        let host =
            Self::validated_network_policy_amendment_host(amendment, network_approval_context)?;
        let codex_home = self
            .state
            .lock()
            .await
            .session_configuration
            .codex_home()
            .clone();
        let execpolicy_amendment =
            execpolicy_network_rule_amendment(amendment, network_approval_context, &host);

        if let Some(started_network_proxy) = self.services.network_proxy.as_ref() {
            let proxy = started_network_proxy.proxy();
            match amendment.action {
                NetworkPolicyRuleAction::Allow => proxy
                    .add_allowed_domain(&host)
                    .await
                    .map_err(|err| anyhow::anyhow!("failed to update runtime allowlist: {err}"))?,
                NetworkPolicyRuleAction::Deny => proxy
                    .add_denied_domain(&host)
                    .await
                    .map_err(|err| anyhow::anyhow!("failed to update runtime denylist: {err}"))?,
            }
        }

        self.services
            .exec_policy
            .append_network_rule_and_update(
                &codex_home,
                &host,
                execpolicy_amendment.protocol,
                execpolicy_amendment.decision,
                Some(execpolicy_amendment.justification),
            )
            .await
            .map_err(|err| {
                anyhow::anyhow!("failed to persist network policy amendment to execpolicy: {err}")
            })?;

        Ok(())
    }

    fn validated_network_policy_amendment_host(
        amendment: &NetworkPolicyAmendment,
        network_approval_context: &NetworkApprovalContext,
    ) -> anyhow::Result<String> {
        let approved_host = normalize_host(&network_approval_context.host);
        let amendment_host = normalize_host(&amendment.host);
        if amendment_host != approved_host {
            return Err(anyhow::anyhow!(
                "network policy amendment host '{}' does not match approved host '{}'",
                amendment.host,
                network_approval_context.host
            ));
        }
        Ok(approved_host)
    }

    pub(crate) async fn record_network_policy_amendment_message(
        &self,
        sub_id: &str,
        amendment: &NetworkPolicyAmendment,
    ) {
        let (action, list_name) = match amendment.action {
            NetworkPolicyRuleAction::Allow => ("Allowed", "allowlist"),
            NetworkPolicyRuleAction::Deny => ("Denied", "denylist"),
        };
        let text = format!(
            "{action} network rule saved in execpolicy ({list_name}): {}",
            amendment.host
        );
        let message: ResponseItem = DeveloperInstructions::new(text.clone()).into();

        if let Some(turn_context) = self.turn_context_for_sub_id(sub_id).await {
            self.record_conversation_items(&turn_context, std::slice::from_ref(&message))
                .await;
            return;
        }

        if self
            .inject_response_items(vec![ResponseInputItem::Message {
                role: "developer".to_string(),
                content: vec![ContentItem::InputText { text }],
            }])
            .await
            .is_err()
        {
            warn!("no active turn found to record network policy amendment message for {sub_id}");
        }
    }

    /// Emit an exec approval request event and await the user's decision.
    ///
    /// The request is keyed by `call_id` + `approval_id` so matching responses
    /// are delivered to the correct in-flight turn. If the task is aborted,
    /// this returns the default `ReviewDecision` (`Denied`).
    ///
    /// Note that if `available_decisions` is `None`, then the other fields will
    /// be used to derive the available decisions via
    /// [ExecApprovalRequestEvent::default_available_decisions].
    #[allow(clippy::too_many_arguments)]
    pub async fn request_command_approval(
        &self,
        turn_context: &TurnContext,
        call_id: String,
        approval_id: Option<String>,
        command: Vec<String>,
        cwd: PathBuf,
        reason: Option<String>,
        network_approval_context: Option<NetworkApprovalContext>,
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
        additional_permissions: Option<PermissionProfile>,
        available_decisions: Option<Vec<ReviewDecision>>,
    ) -> ReviewDecision {
        //  command-level approvals use `call_id`.
        // `approval_id` is only present for subcommand callbacks (execve intercept)
        let effective_approval_id = approval_id.clone().unwrap_or_else(|| call_id.clone());
        // Add the tx_approve callback to the map before sending the request.
        let (tx_approve, rx_approve) = oneshot::channel();
        let prev_entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.insert_pending_approval(effective_approval_id.clone(), tx_approve)
                }
                None => None,
            }
        };
        if prev_entry.is_some() {
            warn!("Overwriting existing pending approval for call_id: {effective_approval_id}");
        }

        let parsed_cmd = parse_command(&command);
        let proposed_network_policy_amendments = network_approval_context.as_ref().map(|context| {
            vec![
                NetworkPolicyAmendment {
                    host: context.host.clone(),
                    action: NetworkPolicyRuleAction::Allow,
                },
                NetworkPolicyAmendment {
                    host: context.host.clone(),
                    action: NetworkPolicyRuleAction::Deny,
                },
            ]
        });
        let available_decisions = available_decisions.unwrap_or_else(|| {
            ExecApprovalRequestEvent::default_available_decisions(
                network_approval_context.as_ref(),
                proposed_execpolicy_amendment.as_ref(),
                proposed_network_policy_amendments.as_deref(),
                additional_permissions.as_ref(),
            )
        });
        let event = EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            approval_id,
            turn_id: turn_context.sub_id.clone(),
            command,
            cwd,
            reason,
            network_approval_context,
            proposed_execpolicy_amendment,
            proposed_network_policy_amendments,
            additional_permissions,
            available_decisions: Some(available_decisions),
            parsed_cmd,
        });
        self.send_event(turn_context, event).await;
        rx_approve.await.unwrap_or_default()
    }

    pub async fn request_patch_approval(
        &self,
        turn_context: &TurnContext,
        call_id: String,
        changes: HashMap<PathBuf, FileChange>,
        reason: Option<String>,
        grant_root: Option<PathBuf>,
    ) -> oneshot::Receiver<ReviewDecision> {
        // Add the tx_approve callback to the map before sending the request.
        let (tx_approve, rx_approve) = oneshot::channel();
        let approval_id = call_id.clone();
        let prev_entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.insert_pending_approval(approval_id.clone(), tx_approve)
                }
                None => None,
            }
        };
        if prev_entry.is_some() {
            warn!("Overwriting existing pending approval for call_id: {approval_id}");
        }

        let event = EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id,
            turn_id: turn_context.sub_id.clone(),
            changes,
            reason,
            grant_root,
        });
        self.send_event(turn_context, event).await;
        rx_approve
    }

    pub async fn request_user_input(
        &self,
        turn_context: &TurnContext,
        call_id: String,
        args: RequestUserInputArgs,
    ) -> Option<RequestUserInputResponse> {
        let sub_id = turn_context.sub_id.clone();
        let (tx_response, rx_response) = oneshot::channel();
        let event_id = sub_id.clone();
        let prev_entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.insert_pending_user_input(sub_id, tx_response)
                }
                None => None,
            }
        };
        if prev_entry.is_some() {
            warn!("Overwriting existing pending user input for sub_id: {event_id}");
        }

        let event = EventMsg::RequestUserInput(RequestUserInputEvent {
            call_id,
            turn_id: turn_context.sub_id.clone(),
            questions: args.questions,
        });
        self.send_event(turn_context, event).await;
        rx_response.await.ok()
    }

    pub async fn notify_user_input_response(
        &self,
        sub_id: &str,
        response: RequestUserInputResponse,
    ) {
        let entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.remove_pending_user_input(sub_id)
                }
                None => None,
            }
        };
        match entry {
            Some(tx_response) => {
                tx_response.send(response).ok();
            }
            None => {
                warn!("No pending user input found for sub_id: {sub_id}");
            }
        }
    }

    pub async fn notify_dynamic_tool_response(&self, call_id: &str, response: DynamicToolResponse) {
        let entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.remove_pending_dynamic_tool(call_id)
                }
                None => None,
            }
        };
        match entry {
            Some(tx_response) => {
                tx_response.send(response).ok();
            }
            None => {
                warn!("No pending dynamic tool call found for call_id: {call_id}");
            }
        }
    }

    pub async fn notify_approval(&self, approval_id: &str, decision: ReviewDecision) {
        let entry = {
            let mut active = self.active_turn.lock().await;
            match active.as_mut() {
                Some(at) => {
                    let mut ts = at.turn_state.lock().await;
                    ts.remove_pending_approval(approval_id)
                }
                None => None,
            }
        };
        match entry {
            Some(tx_approve) => {
                tx_approve.send(decision).ok();
            }
            None => {
                warn!("No pending approval found for call_id: {approval_id}");
            }
        }
    }

    pub async fn resolve_elicitation(
        &self,
        server_name: String,
        id: RequestId,
        response: ElicitationResponse,
    ) -> anyhow::Result<()> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .resolve_elicitation(server_name, id, response)
            .await
    }

    /// Records input items: always append to conversation history and
    /// persist these response items to rollout.
    pub(crate) async fn record_conversation_items(
        &self,
        turn_context: &TurnContext,
        items: &[ResponseItem],
    ) {
        self.record_into_history(items, turn_context).await;
        self.persist_rollout_response_items(items).await;
        self.send_raw_response_items(turn_context, items).await;
    }

    async fn reconstruct_history_from_rollout(
        &self,
        turn_context: &TurnContext,
        rollout_items: &[RolloutItem],
    ) -> Vec<ResponseItem> {
        let mut history = ContextManager::new();
        for item in rollout_items {
            match item {
                RolloutItem::ResponseItem(response_item) => {
                    history.record_items(
                        std::iter::once(response_item),
                        turn_context.truncation_policy,
                    );
                }
                RolloutItem::Compacted(compacted) => {
                    if let Some(replacement) = &compacted.replacement_history {
                        history.replace(replacement.clone());
                    } else {
                        let user_messages = collect_user_messages(history.raw_items());
                        let rebuilt = compact::build_compacted_history(
                            self.build_initial_context(turn_context, None).await,
                            &user_messages,
                            &compacted.message,
                        );
                        history.replace(rebuilt);
                    }
                }
                RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                    history.drop_last_n_user_turns(rollback.num_turns);
                }
                _ => {}
            }
        }
        history.raw_items().to_vec()
    }

    /// Append ResponseItems to the in-memory conversation history only.
    pub(crate) async fn record_into_history(
        &self,
        items: &[ResponseItem],
        turn_context: &TurnContext,
    ) {
        let mut state = self.state.lock().await;
        state.record_items(items.iter(), turn_context.truncation_policy);
    }

    pub(crate) async fn record_model_warning(&self, message: impl Into<String>, ctx: &TurnContext) {
        self.services
            .otel_manager
            .counter("codex.model_warning", 1, &[]);
        let item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!("Warning: {}", message.into()),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };

        self.record_conversation_items(ctx, &[item]).await;
    }

    fn normalize_model_slug_for_server_model_check(slug: &str) -> &str {
        [
            "openai/",
            "google/",
            "anthropic/",
            "xai/",
            "antigravity/",
            "antigravity-gemini/",
            "antigravity-anthropic/",
        ]
        .iter()
        .find_map(|prefix| slug.strip_prefix(prefix))
        .unwrap_or(slug)
    }

    fn should_warn_on_server_model_mismatch(
        provider_wire_api: crate::model_provider_info::WireApi,
        requested_model: &str,
        server_model: &str,
    ) -> bool {
        if provider_wire_api != crate::model_provider_info::WireApi::Responses {
            return false;
        }

        let requested_model_normalized =
            Self::normalize_model_slug_for_server_model_check(requested_model);
        let server_model_normalized =
            Self::normalize_model_slug_for_server_model_check(server_model);
        !server_model_normalized.eq_ignore_ascii_case(requested_model_normalized)
    }

    async fn maybe_warn_on_server_model_mismatch(
        self: &Arc<Self>,
        turn_context: &Arc<TurnContext>,
        server_model: String,
    ) -> bool {
        let requested_model = turn_context.model_info.slug.clone();
        if !Self::should_warn_on_server_model_mismatch(
            turn_context.provider.wire_api,
            &requested_model,
            &server_model,
        ) {
            info!(
                provider_wire_api = ?turn_context.provider.wire_api,
                "server reported model {server_model} (no mismatch warning needed for requested model {requested_model})"
            );
            return false;
        }

        warn!("server reported model {server_model} while requested model was {requested_model}");

        let warning_message = format!(
            "Your account was flagged for potentially high-risk cyber activity and this request was routed to gpt-5.2 as a fallback. To regain access to gpt-5.3-codex, apply for trusted access: {CYBER_VERIFY_URL} or learn more: {CYBER_SAFETY_URL}"
        );

        self.send_event(
            turn_context,
            EventMsg::ModelReroute(ModelRerouteEvent {
                from_model: requested_model.clone(),
                to_model: server_model.clone(),
                reason: ModelRerouteReason::HighRiskCyberActivity,
            }),
        )
        .await;

        self.send_event(
            turn_context,
            EventMsg::Warning(WarningEvent {
                message: warning_message.clone(),
            }),
        )
        .await;
        self.record_model_warning(warning_message, turn_context)
            .await;
        true
    }

    pub(crate) async fn replace_history(
        &self,
        items: Vec<ResponseItem>,
        reference_context_item: Option<TurnContextItem>,
    ) {
        let mut state = self.state.lock().await;
        state.replace_history(items, reference_context_item);
    }

    async fn persist_rollout_response_items(&self, items: &[ResponseItem]) {
        let rollout_items: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();
        self.persist_rollout_items(&rollout_items).await;
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        self.features.enabled(feature)
    }

    pub(crate) fn features(&self) -> Features {
        self.features.clone()
    }

    pub(crate) async fn collaboration_mode(&self) -> CollaborationMode {
        let state = self.state.lock().await;
        state.session_configuration.collaboration_mode.clone()
    }

    async fn send_raw_response_items(&self, turn_context: &TurnContext, items: &[ResponseItem]) {
        for item in items {
            self.send_event(
                turn_context,
                EventMsg::RawResponseItem(RawResponseItemEvent { item: item.clone() }),
            )
            .await;
        }
    }

    pub(crate) async fn build_initial_context(
        &self,
        turn_context: &TurnContext,
        previous_user_turn_model: Option<&str>,
    ) -> Vec<ResponseItem> {
        let mut developer_sections = Vec::<String>::with_capacity(8);
        let mut contextual_user_sections = Vec::<String>::with_capacity(2);
        let shell = self.user_shell();
        if let Some(model_switch_message) =
            crate::context_manager::updates::build_model_instructions_update_item(
                previous_user_turn_model,
                turn_context,
            )
        {
            developer_sections.push(model_switch_message.into_text());
        }
        developer_sections.push(
            DeveloperInstructions::from_policy(
                turn_context.sandbox_policy.get(),
                turn_context.approval_policy.value(),
                self.services.exec_policy.current().as_ref(),
                &turn_context.cwd,
                turn_context.features.enabled(Feature::RequestPermissions),
            )
            .into_text(),
        );
        let separate_guardian_developer_message = {
            let state = self.state.lock().await;
            crate::guardian::is_guardian_subagent_source(
                &state.session_configuration.session_source,
            )
        };
        if !separate_guardian_developer_message
            && let Some(developer_instructions) = turn_context.developer_instructions.as_deref()
        {
            developer_sections.push(developer_instructions.to_string());
        }
        // Add developer instructions for memories.
        if let Some(memory_prompt) =
            build_memory_tool_developer_instructions(&turn_context.config.codex_home).await
            && turn_context.features.enabled(Feature::MemoryTool)
        {
            developer_sections.push(memory_prompt);
        }
        if turn_context.features.enabled(Feature::MemoryTool)
            && let Some(active_memory_source) = turn_context.resolve_memory_read_path_source().await
        {
            developer_sections.push(
                DeveloperInstructions::new(format!(
                    "Active memory scope version: {}",
                    active_memory_source.memory_scope_version
                ))
                .into_text(),
            );
        }

        if turn_context.config.memories.entire_summary_enabled
            && let Ok(checkpoints) =
                crate::entire_integration::get_recent_entire_checkpoints_with_summaries(
                    turn_context.cwd.as_path(),
                    3, // max checkpoints for main agent
                    Some(&self.services.model_client),
                    Some(&self.services.models_manager),
                    Some(&turn_context.config),
                )
                .await
            && !checkpoints.is_empty()
        {
            let summary = crate::entire_integration::format_checkpoints_summary(&checkpoints);
            let summary = codex_utils_string::take_last_bytes_at_char_boundary(&summary, 3000)
                .trim()
                .to_string();
            if !summary.is_empty() {
                developer_sections.push(
                    DeveloperInstructions::new(format!(
                        "Recent AI Sessions (via Entire):\n{summary}"
                    ))
                    .into_text(),
                );
            }
        }

        // Add developer instructions from collaboration_mode if they exist and are non-empty
        let (collaboration_mode, base_instructions) = {
            let state = self.state.lock().await;
            (
                state.session_configuration.collaboration_mode.clone(),
                state.session_configuration.base_instructions.clone(),
            )
        };
        if let Some(collab_instructions) =
            DeveloperInstructions::from_collaboration_mode(&collaboration_mode)
        {
            developer_sections.push(collab_instructions.into_text());
        }
        if self.features.enabled(Feature::Personality)
            && let Some(personality) = turn_context.personality
        {
            let model_info = turn_context.model_info.clone();
            let has_baked_personality = model_info.supports_personality()
                && base_instructions == model_info.get_model_instructions(Some(personality));
            if !has_baked_personality
                && let Some(personality_message) =
                    crate::context_manager::updates::personality_message_for(
                        &model_info,
                        personality,
                    )
            {
                developer_sections.push(
                    DeveloperInstructions::personality_spec_message(personality_message)
                        .into_text(),
                );
            }
        }
        if turn_context.features.enabled(Feature::Apps) {
            developer_sections.push(render_apps_section());
        }
        if turn_context.features.enabled(Feature::CodexGitCommit)
            && let Some(commit_message_instruction) = commit_message_trailer_instruction(
                turn_context.config.commit_attribution.as_deref(),
            )
        {
            developer_sections.push(commit_message_instruction);
        }
        if let Some(user_instructions) = turn_context.user_instructions.as_deref() {
            let user_instructions = truncate_user_instructions_for_context(
                user_instructions,
                turn_context.model_context_window(),
            );
            contextual_user_sections.push(
                UserInstructions {
                    text: user_instructions,
                    directory: turn_context.cwd.to_string_lossy().into_owned(),
                }
                .serialize_to_text(),
            );
        }
        contextual_user_sections.push(
            EnvironmentContext::from_turn_context(turn_context, shell.as_ref()).serialize_to_xml(),
        );

        let mut items = Vec::with_capacity(2);
        if let Some(developer_message) =
            crate::context_manager::updates::build_developer_update_item(developer_sections)
        {
            items.push(developer_message);
        }
        if let Some(contextual_user_message) =
            crate::context_manager::updates::build_contextual_user_message(contextual_user_sections)
        {
            items.push(contextual_user_message);
        }
        if separate_guardian_developer_message
            && let Some(developer_instructions) = turn_context.developer_instructions.as_deref()
            && let Some(guardian_developer_message) =
                crate::context_manager::updates::build_developer_update_item(vec![
                    developer_instructions.to_string(),
                ])
        {
            items.push(guardian_developer_message);
        }
        items
    }

    pub(crate) async fn persist_rollout_items(&self, items: &[RolloutItem]) {
        let recorder = {
            let guard = self.services.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.record_items(items).await
        {
            error!("failed to record rollout items: {e:#}");
        }
    }

    pub(crate) async fn clone_history(&self) -> ContextManager {
        let state = self.state.lock().await;
        state.clone_history()
    }

    pub(crate) async fn reference_context_item(&self) -> Option<TurnContextItem> {
        let state = self.state.lock().await;
        state.reference_context_item()
    }

    /// Persist the latest turn context snapshot and emit any required model-visible context updates.
    ///
    /// When the reference snapshot is missing, this injects full initial context. Otherwise, it
    /// emits only settings diff items.
    ///
    /// If full context is injected and a model switch occurred, this prepends the
    /// `<model_switch>` developer message so model-specific instructions are not lost.
    ///
    /// Invariant: this is the only runtime path that writes a non-`None`
    /// `reference_context_item`. Non-regular tasks intentionally do not update that
    /// baseline; `reference_context_item` tracks the latest regular model turn.
    pub(crate) async fn record_context_updates_and_set_reference_context_item(
        &self,
        turn_context: &TurnContext,
        previous_user_turn_model: Option<&str>,
    ) {
        let reference_context_item = self.reference_context_item().await;
        let should_inject_full_context = reference_context_item.is_none();
        let context_items = if should_inject_full_context {
            self.build_initial_context(turn_context, previous_user_turn_model)
                .await
        } else {
            // Steady-state path: append only context diffs to minimize token overhead.
            self.build_settings_update_items(
                reference_context_item.as_ref(),
                previous_user_turn_model,
                turn_context,
            )
        };
        if !context_items.is_empty() {
            self.record_conversation_items(turn_context, &context_items)
                .await;
        }

        let mut state = self.state.lock().await;
        state.set_reference_context_item(Some(turn_context.to_turn_context_item()));
    }

    pub(crate) async fn update_token_usage_info(
        &self,
        turn_context: &TurnContext,
        token_usage: Option<&TokenUsage>,
    ) {
        {
            let mut state = self.state.lock().await;
            if let Some(token_usage) = token_usage {
                state
                    .update_token_info_from_usage(token_usage, turn_context.model_context_window());
            }
        }
        self.send_token_count_event(turn_context).await;
    }

    pub(crate) async fn recompute_token_usage(&self, turn_context: &TurnContext) {
        let history = self.clone_history().await;
        let base_instructions = self.get_base_instructions().await;
        let Some(estimated_total_tokens) =
            history.estimate_token_count_with_base_instructions(&base_instructions)
        else {
            return;
        };
        {
            let mut state = self.state.lock().await;
            let mut info = state.token_info().unwrap_or(TokenUsageInfo {
                total_token_usage: TokenUsage::default(),
                last_token_usage: TokenUsage::default(),
                model_context_window: None,
            });

            info.last_token_usage = TokenUsage {
                input_tokens: 0,
                cached_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: estimated_total_tokens.max(0),
            };

            if let Some(model_context_window) = turn_context.model_context_window() {
                info.model_context_window = Some(model_context_window);
            }

            state.set_token_info(Some(info));
        }
        self.send_token_count_event(turn_context).await;
    }

    pub(crate) async fn update_rate_limits(
        &self,
        turn_context: &TurnContext,
        new_rate_limits: RateLimitSnapshot,
    ) {
        {
            let mut state = self.state.lock().await;
            state.set_rate_limits(new_rate_limits);
        }
        self.send_token_count_event(turn_context).await;
    }

    pub(crate) async fn mcp_dependency_prompted(&self) -> HashSet<String> {
        let state = self.state.lock().await;
        state.mcp_dependency_prompted()
    }

    pub(crate) async fn record_mcp_dependency_prompted<I>(&self, names: I)
    where
        I: IntoIterator<Item = String>,
    {
        let mut state = self.state.lock().await;
        state.record_mcp_dependency_prompted(names);
    }

    pub async fn dependency_env(&self) -> HashMap<String, String> {
        let state = self.state.lock().await;
        state.dependency_env()
    }

    pub async fn set_dependency_env(&self, values: HashMap<String, String>) {
        let mut state = self.state.lock().await;
        state.set_dependency_env(values);
    }

    pub(crate) async fn set_server_reasoning_included(&self, included: bool) {
        let mut state = self.state.lock().await;
        state.set_server_reasoning_included(included);
    }

    async fn send_token_count_event(&self, turn_context: &TurnContext) {
        let (info, rate_limits) = {
            let state = self.state.lock().await;
            state.token_info_and_rate_limits()
        };
        let event = EventMsg::TokenCount(TokenCountEvent { info, rate_limits });
        self.send_event(turn_context, event).await;
    }

    pub(crate) async fn set_total_tokens_full(&self, turn_context: &TurnContext) {
        if let Some(context_window) = turn_context.model_context_window() {
            let mut state = self.state.lock().await;
            state.set_token_usage_full(context_window);
        }
        self.send_token_count_event(turn_context).await;
    }

    pub(crate) async fn record_response_item_and_emit_turn_item(
        &self,
        turn_context: &TurnContext,
        response_item: ResponseItem,
    ) {
        // Add to conversation history and persist response item to rollout.
        self.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
            .await;

        // Derive a turn item and emit lifecycle events if applicable.
        if let Some(item) = parse_turn_item(&response_item) {
            self.emit_turn_item_started(turn_context, &item).await;
            self.emit_turn_item_completed(turn_context, item).await;
        }
    }

    pub(crate) async fn record_user_prompt_and_emit_turn_item(
        &self,
        turn_context: &TurnContext,
        input: &[UserInput],
        response_item: ResponseItem,
    ) {
        // Persist the user message to history, but emit the turn item from `UserInput` so
        // UI-only `text_elements` are preserved. `ResponseItem::Message` does not carry
        // those spans, and `record_response_item_and_emit_turn_item` would drop them.
        self.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
            .await;
        let turn_item = TurnItem::UserMessage(UserMessageItem::new(input));
        self.emit_turn_item_started(turn_context, &turn_item).await;
        self.emit_turn_item_completed(turn_context, turn_item).await;
        self.ensure_rollout_materialized().await;
    }

    pub(crate) async fn notify_background_event(
        &self,
        turn_context: &TurnContext,
        message: impl Into<String>,
    ) {
        let event = EventMsg::BackgroundEvent(BackgroundEventEvent {
            message: message.into(),
        });
        self.send_event(turn_context, event).await;
    }

    pub(crate) async fn notify_stream_error(
        &self,
        turn_context: &TurnContext,
        message: impl Into<String>,
        codex_error: CodexErr,
    ) {
        let additional_details = codex_error.to_string();
        let codex_error_info = CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code: codex_error.http_status_code_value(),
        };
        let event = EventMsg::StreamError(StreamErrorEvent {
            message: message.into(),
            codex_error_info: Some(codex_error_info),
            additional_details: Some(additional_details),
        });
        self.send_event(turn_context, event).await;
    }

    async fn maybe_start_ghost_snapshot(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        cancellation_token: CancellationToken,
    ) {
        if !self.enabled(Feature::GhostCommit) {
            return;
        }
        let token = match turn_context.tool_call_gate.subscribe().await {
            Ok(token) => token,
            Err(err) => {
                warn!("failed to subscribe to ghost snapshot readiness: {err}");
                return;
            }
        };

        info!("spawning ghost snapshot task");
        let task = GhostSnapshotTask::new(token);
        Arc::new(task)
            .run(
                Arc::new(SessionTaskContext::new(self.clone())),
                turn_context.clone(),
                Vec::new(),
                cancellation_token,
            )
            .await;
    }

    /// Inject additional user input into the currently active turn.
    ///
    /// Returns the active turn id when accepted.
    pub async fn steer_input(
        &self,
        input: Vec<UserInput>,
        expected_turn_id: Option<&str>,
    ) -> Result<String, SteerInputError> {
        if input.is_empty() {
            return Err(SteerInputError::EmptyInput);
        }

        let mut active = self.active_turn.lock().await;
        let Some(active_turn) = active.as_mut() else {
            return Err(SteerInputError::NoActiveTurn(input));
        };

        let Some((active_turn_id, _)) = active_turn.tasks.first() else {
            return Err(SteerInputError::NoActiveTurn(input));
        };

        if let Some(expected_turn_id) = expected_turn_id
            && expected_turn_id != active_turn_id
        {
            return Err(SteerInputError::ExpectedTurnMismatch {
                expected: expected_turn_id.to_string(),
                actual: active_turn_id.clone(),
            });
        }

        let mut turn_state = active_turn.turn_state.lock().await;
        turn_state.push_pending_input(input.into());
        Ok(active_turn_id.clone())
    }

    /// Returns the input if there was no task running to inject into
    pub async fn inject_response_items(
        &self,
        input: Vec<ResponseInputItem>,
    ) -> Result<(), Vec<ResponseInputItem>> {
        let mut active = self.active_turn.lock().await;
        match active.as_mut() {
            Some(at) => {
                let mut ts = at.turn_state.lock().await;
                for item in input {
                    ts.push_pending_input(item);
                }
                Ok(())
            }
            None => Err(input),
        }
    }

    pub async fn get_pending_input(&self) -> Vec<ResponseInputItem> {
        let mut active = self.active_turn.lock().await;
        match active.as_mut() {
            Some(at) => {
                let mut ts = at.turn_state.lock().await;
                ts.take_pending_input()
            }
            None => Vec::with_capacity(0),
        }
    }

    pub async fn has_pending_input(&self) -> bool {
        let active = self.active_turn.lock().await;
        match active.as_ref() {
            Some(at) => {
                let ts = at.turn_state.lock().await;
                ts.has_pending_input()
            }
            None => false,
        }
    }

    pub async fn list_resources(
        &self,
        server: &str,
        params: Option<PaginatedRequestParams>,
    ) -> anyhow::Result<ListResourcesResult> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .list_resources(server, params)
            .await
    }

    pub async fn list_resource_templates(
        &self,
        server: &str,
        params: Option<PaginatedRequestParams>,
    ) -> anyhow::Result<ListResourceTemplatesResult> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .list_resource_templates(server, params)
            .await
    }

    pub async fn read_resource(
        &self,
        server: &str,
        params: ReadResourceRequestParams,
    ) -> anyhow::Result<ReadResourceResult> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .read_resource(server, params)
            .await
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<serde_json::Value>,
    ) -> anyhow::Result<CallToolResult> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .call_tool(server, tool, arguments)
            .await
    }

    pub(crate) async fn parse_mcp_tool_name(&self, tool_name: &str) -> Option<(String, String)> {
        self.services
            .mcp_connection_manager
            .read()
            .await
            .parse_tool_name(tool_name)
            .await
    }

    pub async fn interrupt_task(self: &Arc<Self>) {
        info!("interrupt received: abort current task, if any");
        let has_active_turn = { self.active_turn.lock().await.is_some() };
        if has_active_turn {
            self.abort_all_tasks(TurnAbortReason::Interrupted).await;
        } else {
            self.cancel_mcp_startup().await;
        }
    }

    pub(crate) fn hooks(&self) -> &Hooks {
        &self.services.hooks
    }

    pub(crate) fn user_shell(&self) -> Arc<shell::Shell> {
        Arc::clone(&self.services.user_shell)
    }

    async fn refresh_mcp_servers_inner(
        &self,
        turn_context: &TurnContext,
        mcp_servers: HashMap<String, McpServerConfig>,
        store_mode: OAuthCredentialsStoreMode,
    ) {
        let auth = self.services.auth_manager.auth().await;
        let config = self.get_config().await;
        let mcp_servers = with_codex_apps_mcp(
            mcp_servers,
            self.features.enabled(Feature::Apps),
            auth.as_ref(),
            config.as_ref(),
        );
        let auth_statuses = compute_auth_statuses(mcp_servers.iter(), store_mode).await;
        let sandbox_state = SandboxState {
            sandbox_policy: turn_context.sandbox_policy.get().clone(),
            codex_linux_sandbox_exe: turn_context.codex_linux_sandbox_exe.clone(),
            sandbox_cwd: turn_context.cwd.clone(),
            use_linux_sandbox_bwrap: turn_context.features.enabled(Feature::UseLinuxSandboxBwrap),
        };
        {
            let mut guard = self.services.mcp_startup_cancellation_token.lock().await;
            guard.cancel();
            *guard = CancellationToken::new();
        }
        let (refreshed_manager, cancel_token) = McpConnectionManager::new(
            &mcp_servers,
            store_mode,
            auth_statuses,
            &turn_context.config.permissions.approval_policy,
            self.get_tx_event(),
            sandbox_state,
            config.codex_home.clone(),
            codex_apps_tools_cache_key(auth.as_ref()),
        )
        .await;
        {
            let mut guard = self.services.mcp_startup_cancellation_token.lock().await;
            if guard.is_cancelled() {
                cancel_token.cancel();
            }
            *guard = cancel_token;
        }

        let mut manager = self.services.mcp_connection_manager.write().await;
        *manager = refreshed_manager;
    }

    async fn refresh_mcp_servers_if_requested(&self, turn_context: &TurnContext) {
        let refresh_config = { self.pending_mcp_server_refresh_config.lock().await.take() };
        let Some(refresh_config) = refresh_config else {
            return;
        };

        let McpServerRefreshConfig {
            mcp_servers,
            mcp_oauth_credentials_store_mode,
        } = refresh_config;

        let mcp_servers =
            match serde_json::from_value::<HashMap<String, McpServerConfig>>(mcp_servers) {
                Ok(servers) => servers,
                Err(err) => {
                    warn!("failed to parse MCP server refresh config: {err}");
                    return;
                }
            };
        let store_mode = match serde_json::from_value::<OAuthCredentialsStoreMode>(
            mcp_oauth_credentials_store_mode,
        ) {
            Ok(mode) => mode,
            Err(err) => {
                warn!("failed to parse MCP OAuth refresh config: {err}");
                return;
            }
        };

        self.refresh_mcp_servers_inner(turn_context, mcp_servers, store_mode)
            .await;
    }

    pub(crate) async fn refresh_mcp_servers_now(
        &self,
        turn_context: &TurnContext,
        mcp_servers: HashMap<String, McpServerConfig>,
        store_mode: OAuthCredentialsStoreMode,
    ) {
        self.refresh_mcp_servers_inner(turn_context, mcp_servers, store_mode)
            .await;
    }

    #[cfg(test)]
    async fn mcp_startup_cancellation_token(&self) -> CancellationToken {
        self.services
            .mcp_startup_cancellation_token
            .lock()
            .await
            .clone()
    }

    fn show_raw_agent_reasoning(&self) -> bool {
        self.services.show_raw_agent_reasoning
    }

    async fn cancel_mcp_startup(&self) {
        self.services
            .mcp_startup_cancellation_token
            .lock()
            .await
            .cancel();
    }
}

async fn submission_loop(sess: Arc<Session>, config: Arc<Config>, rx_sub: Receiver<Submission>) {
    // To break out of this loop, send Op::Shutdown.
    while let Ok(sub) = rx_sub.recv().await {
        debug!(?sub, "Submission");
        match sub.op.clone() {
            Op::Interrupt => {
                handlers::interrupt(&sess).await;
            }
            Op::CleanBackgroundTerminals => {
                handlers::clean_background_terminals(&sess).await;
            }
            Op::RealtimeConversationStart(params) => {
                if let Err(err) =
                    handle_realtime_conversation_start(&sess, sub.id.clone(), params).await
                {
                    sess.send_event_raw(Event {
                        id: sub.id.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: err.to_string(),
                            codex_error_info: Some(CodexErrorInfo::Other),
                        }),
                    })
                    .await;
                }
            }
            Op::RealtimeConversationAudio(params) => {
                handle_realtime_conversation_audio(&sess, sub.id.clone(), params).await;
            }
            Op::RealtimeConversationText(params) => {
                handle_realtime_conversation_text(&sess, sub.id.clone(), params).await;
            }
            Op::RealtimeConversationClose => {
                handle_realtime_conversation_close(&sess, sub.id.clone()).await;
            }
            Op::OverrideTurnContext {
                cwd,
                approval_policy,
                approvals_reviewer,
                sandbox_policy,
                windows_sandbox_level,
                model,
                effort,
                summary,
                collaboration_mode,
                personality,
            } => {
                let collaboration_mode = if let Some(collab_mode) = collaboration_mode {
                    collab_mode
                } else {
                    let state = sess.state.lock().await;
                    state.session_configuration.collaboration_mode.with_updates(
                        model.clone(),
                        effort,
                        None,
                    )
                };
                handlers::override_turn_context(
                    &sess,
                    sub.id.clone(),
                    SessionSettingsUpdate {
                        cwd,
                        approval_policy,
                        approvals_reviewer,
                        sandbox_policy,
                        windows_sandbox_level,
                        collaboration_mode: Some(collaboration_mode),
                        reasoning_summary: summary,
                        personality,
                        ..Default::default()
                    },
                )
                .await;
            }
            Op::UserInput { .. } | Op::UserTurn { .. } => {
                handlers::user_input_or_turn(&sess, sub.id.clone(), sub.op).await;
            }
            Op::ExecApproval {
                id: approval_id,
                turn_id,
                decision,
            } => {
                handlers::exec_approval(&sess, approval_id, turn_id, decision).await;
            }
            Op::PatchApproval { id, decision } => {
                handlers::patch_approval(&sess, id, decision).await;
            }
            Op::UserInputAnswer { id, response } => {
                handlers::request_user_input_response(&sess, id, response).await;
            }
            Op::DynamicToolResponse { id, response } => {
                handlers::dynamic_tool_response(&sess, id, response).await;
            }
            Op::AddToHistory { text } => {
                handlers::add_to_history(&sess, &config, text).await;
            }
            Op::GetHistoryEntryRequest { offset, log_id } => {
                handlers::get_history_entry_request(&sess, &config, sub.id.clone(), offset, log_id)
                    .await;
            }
            Op::ListMcpTools => {
                handlers::list_mcp_tools(&sess, &config, sub.id.clone()).await;
            }
            Op::RefreshMcpServers { config } => {
                handlers::refresh_mcp_servers(&sess, config).await;
            }
            Op::ReloadUserConfig => {
                handlers::reload_user_config(&sess).await;
            }
            Op::ListCustomPrompts => {
                handlers::list_custom_prompts(&sess, sub.id.clone()).await;
            }
            Op::ListSkills { cwds, force_reload } => {
                handlers::list_skills(&sess, sub.id.clone(), cwds, force_reload).await;
            }
            Op::ListRemoteSkills {
                hazelnut_scope,
                product_surface,
                enabled,
            } => {
                handlers::list_remote_skills(
                    &sess,
                    &config,
                    sub.id.clone(),
                    hazelnut_scope,
                    product_surface,
                    enabled,
                )
                .await;
            }
            Op::DownloadRemoteSkill { hazelnut_id } => {
                handlers::export_remote_skill(&sess, &config, sub.id.clone(), hazelnut_id).await;
            }
            Op::Undo => {
                handlers::undo(&sess, sub.id.clone()).await;
            }
            Op::Compact => {
                handlers::compact(&sess, sub.id.clone()).await;
            }
            Op::DropMemories => {
                handlers::drop_memories(&sess, &config, sub.id.clone()).await;
            }
            Op::UpdateMemories => {
                handlers::update_memories(&sess, &config, sub.id.clone()).await;
            }
            Op::ThreadRollback { num_turns } => {
                handlers::thread_rollback(&sess, sub.id.clone(), num_turns).await;
            }
            Op::SetThreadName { name } => {
                handlers::set_thread_name(&sess, sub.id.clone(), name).await;
            }
            Op::RunUserShellCommand { command } => {
                handlers::run_user_shell_command(&sess, sub.id.clone(), command).await;
            }
            Op::ResolveElicitation {
                server_name,
                request_id,
                decision,
                content,
            } => {
                handlers::resolve_elicitation(&sess, server_name, request_id, decision, content)
                    .await;
            }
            Op::Shutdown => {
                if handlers::shutdown(&sess, sub.id.clone()).await {
                    break;
                }
            }
            Op::Review { review_request } => {
                handlers::review(&sess, &config, sub.id.clone(), review_request).await;
            }
            Op::SetReferenceImages { paths } => {
                handlers::set_reference_images(&sess, paths).await;
            }
            Op::ClearReferenceImages => {
                handlers::clear_reference_images(&sess).await;
            }
            Op::SetImageQuality { size } => {
                handlers::set_image_quality(&sess, &size).await;
            }
            Op::SetAspectRatio { ratio } => {
                handlers::set_aspect_ratio(&sess, &ratio).await;
            }
            _ => {} // Ignore unknown ops; enum is non_exhaustive to allow extensions.
        }
    }
    debug!("Agent loop exited");
}

/// Operation handlers
mod handlers {
    use crate::codex::Session;
    use crate::codex::SessionSettingsUpdate;
    use crate::codex::SteerInputError;

    use crate::codex::spawn_review_thread;
    use crate::config::Config;

    use crate::mcp::auth::compute_auth_statuses;
    use crate::mcp::collect_mcp_snapshot_from_manager;
    use crate::mcp::effective_mcp_servers;
    use crate::review_prompts::resolve_review_request;
    use crate::rollout::session_index;
    use crate::tasks::CompactTask;
    use crate::tasks::UndoTask;
    use crate::tasks::UserShellCommandMode;
    use crate::tasks::UserShellCommandTask;
    use crate::tasks::execute_user_shell_command;
    use codex_protocol::custom_prompts::CustomPrompt;
    use codex_protocol::protocol::CodexErrorInfo;
    use codex_protocol::protocol::ErrorEvent;
    use codex_protocol::protocol::Event;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::ListCustomPromptsResponseEvent;
    use codex_protocol::protocol::ListRemoteSkillsResponseEvent;
    use codex_protocol::protocol::ListSkillsResponseEvent;
    use codex_protocol::protocol::McpServerRefreshConfig;
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::RemoteSkillDownloadedEvent;
    use codex_protocol::protocol::RemoteSkillHazelnutScope;
    use codex_protocol::protocol::RemoteSkillProductSurface;
    use codex_protocol::protocol::RemoteSkillSummary;
    use codex_protocol::protocol::ReviewDecision;
    use codex_protocol::protocol::ReviewRequest;
    use codex_protocol::protocol::SkillsListEntry;
    use codex_protocol::protocol::ThreadNameUpdatedEvent;
    use codex_protocol::protocol::ThreadRolledBackEvent;
    use codex_protocol::protocol::TurnAbortReason;
    use codex_protocol::protocol::WarningEvent;
    use codex_protocol::request_user_input::RequestUserInputResponse;

    use crate::context_manager::is_user_turn_boundary;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::dynamic_tools::DynamicToolResponse;
    use codex_protocol::mcp::RequestId as ProtocolRequestId;
    use codex_protocol::user_input::UserInput;
    use codex_rmcp_client::ElicitationAction;
    use codex_rmcp_client::ElicitationResponse;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tracing::info;
    use tracing::warn;

    pub async fn interrupt(sess: &Arc<Session>) {
        sess.interrupt_task().await;
    }

    pub async fn clean_background_terminals(sess: &Arc<Session>) {
        sess.close_unified_exec_processes().await;
    }

    pub async fn override_turn_context(
        sess: &Session,
        sub_id: String,
        updates: SessionSettingsUpdate,
    ) {
        if let Err(err) = sess.update_settings(updates).await {
            sess.send_event_raw(Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: err.to_string(),
                    codex_error_info: Some(CodexErrorInfo::BadRequest),
                }),
            })
            .await;
        }
    }

    pub async fn user_input_or_turn(sess: &Arc<Session>, sub_id: String, op: Op) {
        let (items, updates) = match op {
            Op::UserTurn {
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                effort,
                summary,
                final_output_json_schema,
                items,
                collaboration_mode,
                personality,
            } => {
                let collaboration_mode = collaboration_mode.or_else(|| {
                    Some(CollaborationMode {
                        mode: ModeKind::Default,
                        settings: Settings {
                            model: model.clone(),
                            reasoning_effort: effort,
                            developer_instructions: None,
                        },
                    })
                });
                (
                    items,
                    SessionSettingsUpdate {
                        cwd: Some(cwd),
                        approval_policy: Some(approval_policy),
                        sandbox_policy: Some(sandbox_policy),
                        windows_sandbox_level: None,
                        collaboration_mode,
                        reasoning_summary: Some(summary),
                        final_output_json_schema: Some(final_output_json_schema),
                        personality,
                        ..Default::default()
                    },
                )
            }
            Op::UserInput {
                items,
                final_output_json_schema,
            } => (
                items,
                SessionSettingsUpdate {
                    final_output_json_schema: Some(final_output_json_schema),
                    ..Default::default()
                },
            ),
            _ => unreachable!(),
        };

        let Ok(current_context) = sess.new_turn_with_sub_id(sub_id, updates).await else {
            // new_turn_with_sub_id already emits the error event.
            return;
        };
        sess.maybe_emit_unknown_model_warning_for_turn(current_context.as_ref())
            .await;
        current_context.otel_manager.user_prompt(&items);

        // If the new turn context changes model/provider, do not steer into the currently active
        // task: start a fresh turn so requests use the updated provider endpoint.
        let should_replace_active_turn = sess
            .active_turn_context_and_cancellation_token()
            .await
            .is_some_and(|(active_turn_context, _)| {
                active_turn_context.model_info.slug != current_context.model_info.slug
                    || active_turn_context.provider != current_context.provider
            });

        let items_for_new_turn = if should_replace_active_turn {
            Some(items)
        } else {
            match sess.steer_input(items, None).await {
                Ok(_) => None,
                Err(SteerInputError::NoActiveTurn(items)) => Some(items),
                Err(SteerInputError::ExpectedTurnMismatch { .. } | SteerInputError::EmptyInput) => {
                    None
                }
            }
        };

        if let Some(items) = items_for_new_turn {
            sess.refresh_mcp_servers_if_requested(&current_context)
                .await;
            let regular_task = sess.take_startup_regular_task().await.unwrap_or_default();
            sess.spawn_task(Arc::clone(&current_context), items, regular_task)
                .await;
        }
    }

    pub async fn run_user_shell_command(sess: &Arc<Session>, sub_id: String, command: String) {
        if let Some((turn_context, cancellation_token)) =
            sess.active_turn_context_and_cancellation_token().await
        {
            let session = Arc::clone(sess);
            tokio::spawn(async move {
                execute_user_shell_command(
                    session,
                    turn_context,
                    command,
                    cancellation_token,
                    UserShellCommandMode::ActiveTurnAuxiliary,
                )
                .await;
            });
            return;
        }

        let turn_context = sess.new_default_turn_with_sub_id(sub_id).await;
        sess.spawn_task(
            Arc::clone(&turn_context),
            Vec::new(),
            UserShellCommandTask::new(command),
        )
        .await;
    }

    pub async fn resolve_elicitation(
        sess: &Arc<Session>,
        server_name: String,
        request_id: ProtocolRequestId,
        decision: codex_protocol::approvals::ElicitationAction,
        content: Option<Value>,
    ) {
        let action = match decision {
            codex_protocol::approvals::ElicitationAction::Accept => ElicitationAction::Accept,
            codex_protocol::approvals::ElicitationAction::Decline => ElicitationAction::Decline,
            codex_protocol::approvals::ElicitationAction::Cancel => ElicitationAction::Cancel,
        };
        let content = match action {
            // Preserve the legacy fallback for clients that only send an action.
            ElicitationAction::Accept => Some(content.unwrap_or_else(|| serde_json::json!({}))),
            ElicitationAction::Decline | ElicitationAction::Cancel => None,
        };
        let response = ElicitationResponse { action, content };
        let request_id = match request_id {
            ProtocolRequestId::String(value) => {
                rmcp::model::NumberOrString::String(std::sync::Arc::from(value))
            }
            ProtocolRequestId::Integer(value) => rmcp::model::NumberOrString::Number(value),
        };
        if let Err(err) = sess
            .resolve_elicitation(server_name, request_id, response)
            .await
        {
            warn!(
                error = %err,
                "failed to resolve elicitation request in session"
            );
        }
    }

    /// Propagate a user's exec approval decision to the session.
    /// Also optionally applies an execpolicy amendment.
    pub async fn exec_approval(
        sess: &Arc<Session>,
        approval_id: String,
        turn_id: Option<String>,
        decision: ReviewDecision,
    ) {
        let event_turn_id = turn_id.unwrap_or_else(|| approval_id.clone());
        if let ReviewDecision::ApprovedExecpolicyAmendment {
            proposed_execpolicy_amendment,
        } = &decision
        {
            match sess
                .persist_execpolicy_amendment(proposed_execpolicy_amendment)
                .await
            {
                Ok(()) => {
                    sess.record_execpolicy_amendment_message(
                        &event_turn_id,
                        proposed_execpolicy_amendment,
                    )
                    .await;
                }
                Err(err) => {
                    let message = format!("Failed to apply execpolicy amendment: {err}");
                    tracing::warn!("{message}");
                    let warning = EventMsg::Warning(WarningEvent { message });
                    sess.send_event_raw(Event {
                        id: event_turn_id.clone(),
                        msg: warning,
                    })
                    .await;
                }
            }
        }
        match decision {
            ReviewDecision::Abort => {
                sess.interrupt_task().await;
            }
            other => sess.notify_approval(&approval_id, other).await,
        }
    }

    pub async fn patch_approval(sess: &Arc<Session>, id: String, decision: ReviewDecision) {
        match decision {
            ReviewDecision::Abort => {
                sess.interrupt_task().await;
            }
            other => sess.notify_approval(&id, other).await,
        }
    }

    pub async fn request_user_input_response(
        sess: &Arc<Session>,
        id: String,
        response: RequestUserInputResponse,
    ) {
        sess.notify_user_input_response(&id, response).await;
    }

    pub async fn dynamic_tool_response(
        sess: &Arc<Session>,
        id: String,
        response: DynamicToolResponse,
    ) {
        sess.notify_dynamic_tool_response(&id, response).await;
    }

    pub async fn add_to_history(sess: &Arc<Session>, config: &Arc<Config>, text: String) {
        let id = sess.conversation_id;
        let config = Arc::clone(config);
        tokio::spawn(async move {
            if let Err(e) = crate::message_history::append_entry(&text, &id, &config).await {
                warn!("failed to append to message history: {e}");
            }
        });
    }

    pub async fn get_history_entry_request(
        sess: &Arc<Session>,
        config: &Arc<Config>,
        sub_id: String,
        offset: usize,
        log_id: u64,
    ) {
        let config = Arc::clone(config);
        let sess_clone = Arc::clone(sess);

        tokio::spawn(async move {
            // Run lookup in blocking thread because it does file IO + locking.
            let entry_opt = tokio::task::spawn_blocking(move || {
                crate::message_history::lookup(log_id, offset, &config)
            })
            .await
            .unwrap_or(None);

            let event = Event {
                id: sub_id,
                msg: EventMsg::GetHistoryEntryResponse(
                    crate::protocol::GetHistoryEntryResponseEvent {
                        offset,
                        log_id,
                        entry: entry_opt.map(|e| codex_protocol::message_history::HistoryEntry {
                            conversation_id: e.session_id,
                            ts: e.ts,
                            text: e.text,
                        }),
                    },
                ),
            };

            sess_clone.send_event_raw(event).await;
        });
    }

    pub async fn refresh_mcp_servers(sess: &Arc<Session>, refresh_config: McpServerRefreshConfig) {
        let mut guard = sess.pending_mcp_server_refresh_config.lock().await;
        *guard = Some(refresh_config);
    }

    pub async fn reload_user_config(sess: &Arc<Session>) {
        sess.reload_user_config_layer().await;
    }

    pub async fn list_mcp_tools(sess: &Session, config: &Arc<Config>, sub_id: String) {
        let mcp_connection_manager = sess.services.mcp_connection_manager.read().await;
        let auth = sess.services.auth_manager.auth().await;
        let mcp_servers = effective_mcp_servers(config, auth.as_ref());
        let snapshot = collect_mcp_snapshot_from_manager(
            &mcp_connection_manager,
            compute_auth_statuses(mcp_servers.iter(), config.mcp_oauth_credentials_store_mode)
                .await,
        )
        .await;
        let event = Event {
            id: sub_id,
            msg: EventMsg::McpListToolsResponse(snapshot),
        };
        sess.send_event_raw(event).await;
    }

    pub async fn list_custom_prompts(sess: &Session, sub_id: String) {
        let custom_prompts: Vec<CustomPrompt> =
            if let Some(dir) = crate::custom_prompts::default_prompts_dir() {
                crate::custom_prompts::discover_prompts_in(&dir).await
            } else {
                Vec::new()
            };

        let event = Event {
            id: sub_id,
            msg: EventMsg::ListCustomPromptsResponse(ListCustomPromptsResponseEvent {
                custom_prompts,
            }),
        };
        sess.send_event_raw(event).await;
    }

    pub async fn list_skills(
        sess: &Session,
        sub_id: String,
        cwds: Vec<PathBuf>,
        force_reload: bool,
    ) {
        let cwds = if cwds.is_empty() {
            let state = sess.state.lock().await;
            vec![state.session_configuration.cwd.clone()]
        } else {
            cwds
        };

        let skills_manager = &sess.services.skills_manager;
        let mut skills = Vec::new();
        for cwd in cwds {
            let outcome = skills_manager.skills_for_cwd(&cwd, force_reload).await;
            let errors = super::errors_to_info(&outcome.errors);
            let skills_metadata = super::skills_to_info(&outcome.skills, &outcome.disabled_paths);
            skills.push(SkillsListEntry {
                cwd,
                skills: skills_metadata,
                errors,
            });
        }

        let event = Event {
            id: sub_id,
            msg: EventMsg::ListSkillsResponse(ListSkillsResponseEvent { skills }),
        };
        sess.send_event_raw(event).await;
    }

    pub async fn list_remote_skills(
        sess: &Session,
        config: &Arc<Config>,
        sub_id: String,
        hazelnut_scope: RemoteSkillHazelnutScope,
        product_surface: RemoteSkillProductSurface,
        enabled: Option<bool>,
    ) {
        let auth = sess.services.auth_manager.auth().await;
        let response = crate::skills::remote::list_remote_skills(
            config,
            auth.as_ref(),
            hazelnut_scope,
            product_surface,
            enabled,
        )
        .await
        .map(|skills| {
            skills
                .into_iter()
                .map(|skill| RemoteSkillSummary {
                    id: skill.id,
                    name: skill.name,
                    description: skill.description,
                })
                .collect::<Vec<_>>()
        });

        match response {
            Ok(skills) => {
                let event = Event {
                    id: sub_id,
                    msg: EventMsg::ListRemoteSkillsResponse(ListRemoteSkillsResponseEvent {
                        skills,
                    }),
                };
                sess.send_event_raw(event).await;
            }
            Err(err) => {
                let event = Event {
                    id: sub_id,
                    msg: EventMsg::Error(ErrorEvent {
                        message: format!("failed to list remote skills: {err}"),
                        codex_error_info: Some(CodexErrorInfo::Other),
                    }),
                };
                sess.send_event_raw(event).await;
            }
        }
    }

    pub async fn export_remote_skill(
        sess: &Session,
        config: &Arc<Config>,
        sub_id: String,
        hazelnut_id: String,
    ) {
        let auth = sess.services.auth_manager.auth().await;
        match crate::skills::remote::export_remote_skill(
            config,
            auth.as_ref(),
            hazelnut_id.as_str(),
        )
        .await
        {
            Ok(result) => {
                let id = result.id;
                let event = Event {
                    id: sub_id,
                    msg: EventMsg::RemoteSkillDownloaded(RemoteSkillDownloadedEvent {
                        id: id.clone(),
                        name: id,
                        path: result.path,
                    }),
                };
                sess.send_event_raw(event).await;
            }
            Err(err) => {
                let event = Event {
                    id: sub_id,
                    msg: EventMsg::Error(ErrorEvent {
                        message: format!("failed to export remote skill {hazelnut_id}: {err}"),
                        codex_error_info: Some(CodexErrorInfo::Other),
                    }),
                };
                sess.send_event_raw(event).await;
            }
        }
    }

    pub async fn undo(sess: &Arc<Session>, sub_id: String) {
        let turn_context = sess.new_default_turn_with_sub_id(sub_id).await;
        sess.spawn_task(turn_context, Vec::new(), UndoTask::new())
            .await;
    }

    pub async fn compact(sess: &Arc<Session>, sub_id: String) {
        let turn_context = sess.new_default_turn_with_sub_id(sub_id).await;

        sess.spawn_task(
            Arc::clone(&turn_context),
            vec![UserInput::Text {
                text: turn_context.compact_prompt().to_string(),
                // Compaction prompt is synthesized; no UI element ranges to preserve.
                text_elements: Vec::new(),
            }],
            CompactTask,
        )
        .await;
    }

    pub async fn drop_memories(sess: &Arc<Session>, config: &Arc<Config>, sub_id: String) {
        let mut errors = Vec::new();

        if let Some(state_db) = sess.services.state_db.as_deref() {
            if let Err(err) = state_db.clear_memory_data().await {
                errors.push(format!("failed clearing memory rows from state db: {err}"));
            }
        } else {
            errors.push("state db unavailable; memory rows were not cleared".to_string());
        }

        let memory_root = crate::memories::memory_root(&config.codex_home);
        if let Err(err) = tokio::fs::remove_dir_all(&memory_root).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            errors.push(format!(
                "failed removing memory directory {}: {err}",
                memory_root.display()
            ));
        }

        if errors.is_empty() {
            sess.send_event_raw(Event {
                id: sub_id,
                msg: EventMsg::Warning(WarningEvent {
                    message: format!(
                        "Dropped memories at {} and cleared memory rows from state db.",
                        memory_root.display()
                    ),
                }),
            })
            .await;
            return;
        }

        sess.send_event_raw(Event {
            id: sub_id,
            msg: EventMsg::Error(ErrorEvent {
                message: format!("Memory drop completed with errors: {}", errors.join("; ")),
                codex_error_info: Some(CodexErrorInfo::Other),
            }),
        })
        .await;
    }

    pub async fn update_memories(sess: &Arc<Session>, config: &Arc<Config>, sub_id: String) {
        let session_source = {
            let state = sess.state.lock().await;
            state.session_configuration.session_source.clone()
        };

        crate::memories::start_memories_startup_task(sess, Arc::clone(config), &session_source);

        sess.send_event_raw(Event {
            id: sub_id.clone(),
            msg: EventMsg::Warning(WarningEvent {
                message: "Memory update triggered.".to_string(),
            }),
        })
        .await;
    }

    pub async fn thread_rollback(sess: &Arc<Session>, sub_id: String, num_turns: u32) {
        if num_turns == 0 {
            sess.send_event_raw(Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: "num_turns must be >= 1".to_string(),
                    codex_error_info: Some(CodexErrorInfo::ThreadRollbackFailed),
                }),
            })
            .await;
            return;
        }

        let has_active_turn = { sess.active_turn.lock().await.is_some() };
        if has_active_turn {
            sess.send_event_raw(Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: "Cannot rollback while a turn is in progress.".to_string(),
                    codex_error_info: Some(CodexErrorInfo::ThreadRollbackFailed),
                }),
            })
            .await;
            return;
        }

        let turn_context = sess.new_default_turn_with_sub_id(sub_id).await;

        let mut history = sess.clone_history().await;
        // TODO(ccunningham): Fix rollback/backtracking baseline handling.
        // We clear `reference_context_item` here, but should restore the
        // post-rollback baseline from the surviving history/rollout instead.
        // Truncating history should also invalidate/recompute `previous_model`
        // so the next regular turn replays any dropped model-switch
        // instructions.
        history.drop_last_n_user_turns(num_turns);

        // Replace with the raw items. We don't want to replace with a normalized
        // version of the history.
        sess.replace_history(history.raw_items().to_vec(), None)
            .await;
        sess.recompute_token_usage(turn_context.as_ref()).await;

        sess.send_event_raw_flushed(Event {
            id: turn_context.sub_id.clone(),
            msg: EventMsg::ThreadRolledBack(ThreadRolledBackEvent { num_turns }),
        })
        .await;
    }

    /// Persists the thread name in the session index, updates in-memory state, and emits
    /// a `ThreadNameUpdated` event on success.
    ///
    /// This appends the name to `CODEX_HOME/sessions_index.jsonl` via `session_index::append_thread_name` for the
    /// current `thread_id`, then updates `SessionConfiguration::thread_name`.
    ///
    /// Returns an error event if the name is empty or session persistence is disabled.
    pub async fn set_thread_name(sess: &Arc<Session>, sub_id: String, name: String) {
        let Some(name) = crate::util::normalize_thread_name(&name) else {
            let event = Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: "Thread name cannot be empty.".to_string(),
                    codex_error_info: Some(CodexErrorInfo::BadRequest),
                }),
            };
            sess.send_event_raw(event).await;
            return;
        };

        let persistence_enabled = {
            let rollout = sess.services.rollout.lock().await;
            rollout.is_some()
        };
        if !persistence_enabled {
            let event = Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: "Session persistence is disabled; cannot rename thread.".to_string(),
                    codex_error_info: Some(CodexErrorInfo::Other),
                }),
            };
            sess.send_event_raw(event).await;
            return;
        };

        let codex_home = sess.codex_home().await;
        if let Err(e) =
            session_index::append_thread_name(&codex_home, sess.conversation_id, &name).await
        {
            let event = Event {
                id: sub_id,
                msg: EventMsg::Error(ErrorEvent {
                    message: format!("Failed to set thread name: {e}"),
                    codex_error_info: Some(CodexErrorInfo::Other),
                }),
            };
            sess.send_event_raw(event).await;
            return;
        }

        {
            let mut state = sess.state.lock().await;
            state.session_configuration.thread_name = Some(name.clone());
        }

        sess.send_event_raw(Event {
            id: sub_id,
            msg: EventMsg::ThreadNameUpdated(ThreadNameUpdatedEvent {
                thread_id: sess.conversation_id,
                thread_name: Some(name),
            }),
        })
        .await;
    }

    pub async fn shutdown(sess: &Arc<Session>, sub_id: String) -> bool {
        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;
        let _ = sess.conversation.shutdown().await;
        sess.services
            .unified_exec_manager
            .terminate_all_processes()
            .await;
        info!("Shutting down Codex instance");
        let history = sess.clone_history().await;
        let turn_count = history
            .raw_items()
            .iter()
            .filter(|item| is_user_turn_boundary(item))
            .count();
        sess.services.otel_manager.counter(
            "codex.conversation.turn.count",
            i64::try_from(turn_count).unwrap_or(0),
            &[],
        );

        // Gracefully flush and shutdown rollout recorder on session end so tests
        // that inspect the rollout file do not race with the background writer.
        let recorder_opt = {
            let mut guard = sess.services.rollout.lock().await;
            guard.take()
        };
        if let Some(rec) = recorder_opt
            && let Err(e) = rec.shutdown().await
        {
            warn!("failed to shutdown rollout recorder: {e}");
            let event = Event {
                id: sub_id.clone(),
                msg: EventMsg::Error(ErrorEvent {
                    message: "Failed to shutdown rollout recorder".to_string(),
                    codex_error_info: Some(CodexErrorInfo::Other),
                }),
            };
            sess.send_event_raw(event).await;
        }

        let event = Event {
            id: sub_id,
            msg: EventMsg::ShutdownComplete,
        };
        sess.send_event_raw(event).await;
        true
    }

    pub async fn review(
        sess: &Arc<Session>,
        config: &Arc<Config>,
        sub_id: String,
        review_request: ReviewRequest,
    ) {
        let turn_context = sess.new_default_turn_with_sub_id(sub_id.clone()).await;
        sess.maybe_emit_unknown_model_warning_for_turn(turn_context.as_ref())
            .await;
        sess.refresh_mcp_servers_if_requested(&turn_context).await;
        match resolve_review_request(review_request, turn_context.cwd.as_path()) {
            Ok(resolved) => {
                spawn_review_thread(
                    Arc::clone(sess),
                    Arc::clone(config),
                    turn_context.clone(),
                    sub_id,
                    resolved,
                )
                .await;
            }
            Err(err) => {
                let event = Event {
                    id: sub_id,
                    msg: EventMsg::Error(ErrorEvent {
                        message: err.to_string(),
                        codex_error_info: Some(CodexErrorInfo::Other),
                    }),
                };
                sess.send_event(&turn_context, event.msg).await;
            }
        }
    }

    pub async fn set_reference_images(sess: &Arc<Session>, paths: Vec<std::path::PathBuf>) {
        use codex_protocol::models::ContentItem;
        use codex_protocol::models::ResponseInputItem;
        use codex_protocol::user_input::UserInput;

        let cwd = {
            let state = sess.state.lock().await;
            state.session_configuration.cwd.clone()
        };

        let mut images: Vec<String> = Vec::new();

        for path in paths {
            let absolute = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };

            let input = UserInput::LocalImage { path: absolute };
            let item = ResponseInputItem::from(vec![input]);
            if let ResponseInputItem::Message { content, .. } = item {
                for entry in content {
                    if let ContentItem::InputImage { image_url } = entry
                        && !image_url.trim().is_empty()
                    {
                        images.push(image_url);
                    }
                }
            }
        }

        let mut state = sess.state.lock().await;
        state.set_reference_images(images);
    }

    pub async fn clear_reference_images(sess: &Arc<Session>) {
        let mut state = sess.state.lock().await;
        state.clear_reference_images();
    }

    pub async fn set_image_quality(sess: &Arc<Session>, size: &str) {
        use crate::gemini_types::GeminiImageSize;

        let parsed_size = match size.to_uppercase().as_str() {
            "1K" => Some(GeminiImageSize::Size1K),
            "2K" => Some(GeminiImageSize::Size2K),
            "4K" => Some(GeminiImageSize::Size4K),
            _ => {
                tracing::warn!(
                    "Invalid image quality '{}'. Valid options: 1K, 2K, 4K",
                    size
                );
                return;
            }
        };

        let mut state = sess.state.lock().await;
        state.set_image_size(parsed_size);
    }

    pub async fn set_aspect_ratio(sess: &Arc<Session>, ratio: &str) {
        use crate::gemini_types::GeminiAspectRatio;

        let parsed_ratio = match ratio {
            "1:1" => Some(GeminiAspectRatio::Square),
            "16:9" => Some(GeminiAspectRatio::Landscape),
            "9:16" => Some(GeminiAspectRatio::Portrait),
            "4:3" => Some(GeminiAspectRatio::Standard),
            "3:4" => Some(GeminiAspectRatio::StandardPortrait),
            _ => {
                tracing::warn!(
                    "Invalid aspect ratio '{}'. Valid options: 1:1, 16:9, 9:16, 4:3, 3:4",
                    ratio
                );
                return;
            }
        };

        let mut state = sess.state.lock().await;
        state.set_aspect_ratio(parsed_ratio);
    }
}

/// Spawn a review thread using the given prompt.
async fn spawn_review_thread(
    sess: Arc<Session>,
    config: Arc<Config>,
    parent_turn_context: Arc<TurnContext>,
    sub_id: String,
    resolved: crate::review_prompts::ResolvedReviewRequest,
) {
    let model = config
        .review_model
        .clone()
        .unwrap_or_else(|| parent_turn_context.model_info.slug.clone());
    // For reviews, disable web_search and view_image regardless of global settings.
    let mut review_features = sess.features.clone();
    review_features
        .disable(crate::features::Feature::WebSearchRequest)
        .disable(crate::features::Feature::WebSearchCached);
    let review_web_search_mode = WebSearchMode::Disabled;

    let review_prompt = resolved.prompt.clone();

    // Build per‑turn client with the requested model/family.
    let mut per_turn_config = (*config).clone();
    per_turn_config.model = Some(model.clone());
    per_turn_config.features = review_features.clone();
    if let Err(err) = per_turn_config.web_search_mode.set(review_web_search_mode) {
        let fallback_value = per_turn_config.web_search_mode.value();
        tracing::warn!(
            error = %err,
            ?review_web_search_mode,
            ?fallback_value,
            "review web_search_mode is disallowed by requirements; keeping constrained value"
        );
    }
    let (provider_id, logical_provider) =
        crate::utility_model::provider_for_model_slug(&per_turn_config, &model).unwrap_or_else(
            || {
                (
                    per_turn_config.model_provider_id.clone(),
                    per_turn_config.model_provider.clone(),
                )
            },
        );
    let resolved_provider = {
        let mut state = sess.state.lock().await;
        resolve_turn_provider_from_pool(
            &mut state,
            &provider_id,
            &logical_provider,
            std::time::Instant::now(),
        )
    };
    let background_message = resolved_provider.background_message.clone();
    let provider = resolved_provider.provider;
    per_turn_config.model_provider_id = provider_id;
    per_turn_config.model_provider = logical_provider;
    let review_model_info = sess
        .services
        .models_manager
        .get_model_info(&model, &per_turn_config)
        .await;
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &review_model_info,
        features: &review_features,
        web_search_mode: Some(review_web_search_mode),
        is_gemini_wire_api: provider.wire_api == crate::model_provider_info::WireApi::Gemini,
        endpoint_security: per_turn_config.endpoint_security,
        session_source: parent_turn_context.session_source.clone(),
    })
    .with_allow_login_shell(config.permissions.allow_login_shell)
    .with_agent_roles(config.agent_roles.clone());
    let auth_manager = parent_turn_context.auth_manager.clone();
    let model_info = review_model_info.clone();

    let otel_manager = parent_turn_context
        .otel_manager
        .clone()
        .with_model(model.as_str(), review_model_info.slug.as_str());
    let auth_manager_for_context = auth_manager.clone();
    let provider_for_context = provider.clone();
    let otel_manager_for_context = otel_manager.clone();
    let reasoning_effort = per_turn_config.model_reasoning_effort;
    let reasoning_summary = per_turn_config.model_reasoning_summary;
    let session_source = parent_turn_context.session_source.clone();

    let per_turn_config = Arc::new(per_turn_config);
    let review_turn_id = sub_id.to_string();
    let turn_metadata_state = Arc::new(TurnMetadataState::new(
        review_turn_id.clone(),
        parent_turn_context.cwd.clone(),
        parent_turn_context.sandbox_policy.get(),
        parent_turn_context.windows_sandbox_level,
        parent_turn_context
            .features
            .enabled(Feature::UseLinuxSandboxBwrap),
    ));

    let review_turn_context = TurnContext {
        side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::BTreeSet::new(),
        )),

        sub_id: review_turn_id,
        config: per_turn_config,
        auth_manager: auth_manager_for_context,
        model_info: model_info.clone(),
        otel_manager: otel_manager_for_context,
        provider: provider_for_context,
        reasoning_effort,
        reasoning_summary,
        session_source,
        tools_config,
        features: parent_turn_context.features.clone(),
        ghost_snapshot: parent_turn_context.ghost_snapshot.clone(),
        developer_instructions: None,
        user_instructions: None,
        compact_prompt: parent_turn_context.compact_prompt.clone(),
        collaboration_mode: parent_turn_context.collaboration_mode.clone(),
        personality: parent_turn_context.personality,
        approval_policy: parent_turn_context.approval_policy.clone(),
        sandbox_policy: parent_turn_context.sandbox_policy.clone(),
        network: parent_turn_context.network.clone(),
        windows_sandbox_level: parent_turn_context.windows_sandbox_level,
        shell_environment_policy: parent_turn_context.shell_environment_policy.clone(),
        cwd: parent_turn_context.cwd.clone(),
        final_output_json_schema: None,
        codex_linux_sandbox_exe: parent_turn_context.codex_linux_sandbox_exe.clone(),
        tool_call_gate: Arc::new(ReadinessFlag::new()),
        js_repl: Arc::clone(&sess.js_repl),
        dynamic_tools: parent_turn_context.dynamic_tools.clone(),
        truncation_policy: model_info.truncation_policy.into(),
        turn_metadata_header: parent_turn_context.turn_metadata_header.clone(),
        memory_read_path_source: OnceCell::new(),
        hook_memory_context: OnceCell::new(),
        turn_metadata_state,
        turn_skills: TurnSkillsContext::new(parent_turn_context.turn_skills.outcome.clone()),
    };

    // Seed the child task with the review prompt as the initial user message.
    let input: Vec<UserInput> = vec![UserInput::Text {
        text: review_prompt,
        // Review prompt is synthesized; no UI element ranges to preserve.
        text_elements: Vec::new(),
    }];
    let tc = Arc::new(review_turn_context);
    tc.turn_metadata_state.spawn_git_enrichment_task();
    if let Some(message) = background_message {
        sess.notify_background_event(&tc, message).await;
    }
    // TODO(ccunningham): Review turns currently rely on `spawn_task` for TurnComplete but do not
    // emit a parent TurnStarted. Consider giving review a full parent turn lifecycle
    // (TurnStarted + TurnComplete) for consistency with other standalone tasks.
    sess.spawn_task(tc.clone(), input, ReviewTask::new()).await;

    // Announce entering review mode so UIs can switch modes.
    let review_request = ReviewRequest {
        target: resolved.target,
        user_facing_hint: Some(resolved.user_facing_hint),
    };
    sess.send_event(&tc, EventMsg::EnteredReviewMode(review_request))
        .await;
}

fn skills_to_info(
    skills: &[SkillMetadata],
    disabled_paths: &HashSet<PathBuf>,
) -> Vec<ProtocolSkillMetadata> {
    skills
        .iter()
        .map(|skill| ProtocolSkillMetadata {
            name: skill.name.clone(),
            description: skill.description.clone(),
            short_description: skill.short_description.clone(),
            interface: skill
                .interface
                .clone()
                .map(|interface| ProtocolSkillInterface {
                    display_name: interface.display_name,
                    short_description: interface.short_description,
                    icon_small: interface.icon_small,
                    icon_large: interface.icon_large,
                    brand_color: interface.brand_color,
                    default_prompt: interface.default_prompt,
                }),
            dependencies: skill.dependencies.clone().map(|dependencies| {
                ProtocolSkillDependencies {
                    tools: dependencies
                        .tools
                        .into_iter()
                        .map(|tool| ProtocolSkillToolDependency {
                            r#type: tool.r#type,
                            value: tool.value,
                            description: tool.description,
                            transport: tool.transport,
                            command: tool.command,
                            url: tool.url,
                        })
                        .collect(),
                }
            }),
            path: skill.path_to_skills_md.clone(),
            scope: skill.scope,
            enabled: !disabled_paths.contains(&skill.path_to_skills_md),
        })
        .collect()
}

fn errors_to_info(errors: &[SkillError]) -> Vec<SkillErrorInfo> {
    errors
        .iter()
        .map(|err| SkillErrorInfo {
            path: err.path.clone(),
            message: err.message.clone(),
        })
        .collect()
}

/// Takes a user message as input and runs a loop where, at each sampling request, the model
/// replies with either:
///
/// - requested function calls
/// - an assistant message
///
/// While it is possible for the model to return multiple of these items in a
/// single sampling request, in practice, we generally one item per sampling request:
///
/// - If the model requests a function call, we execute it and send the output
///   back to the model in the next sampling request.
/// - If the model sends only an assistant message, we record it in the
///   conversation history and consider the turn complete.
///
pub(crate) async fn run_turn(
    sess: Arc<Session>,
    mut turn_context: Arc<TurnContext>,
    input: Vec<UserInput>,
    prewarmed_client_session: Option<ModelClientSession>,
    cancellation_token: CancellationToken,
) -> Option<String> {
    if input.is_empty() {
        return None;
    }

    let model_info = turn_context.model_info.clone();
    let auto_compact_limit = model_info.auto_compact_token_limit().unwrap_or(i64::MAX);

    let event = EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_context.sub_id.clone(),
        model_context_window: turn_context.model_context_window(),
        collaboration_mode_kind: turn_context.collaboration_mode.mode,
        memory: turn_context.resolve_memory_link().await,
    });
    sess.send_event(&turn_context, event).await;
    // TODO(ccunningham): Pre-turn compaction runs before context updates and the
    // new user message are recorded. Estimate pending incoming items (context
    // diffs/full reinjection + user input) and trigger compaction preemptively
    // when they would push the thread over the compaction threshold.
    if run_pre_sampling_compact(&sess, &turn_context)
        .await
        .is_err()
    {
        error!("Failed to run pre-sampling compact");
        return None;
    }

    let skills_outcome_arc = Arc::clone(&turn_context.turn_skills.outcome);
    let skills_outcome = Some(skills_outcome_arc.as_ref());

    let previous_model = sess.previous_model().await;
    sess.record_context_updates_and_set_reference_context_item(
        turn_context.as_ref(),
        previous_model.as_deref(),
    )
    .await;

    let available_connectors = if turn_context.config.features.enabled(Feature::Apps) {
        let mcp_tools = match sess
            .services
            .mcp_connection_manager
            .read()
            .await
            .list_all_tools()
            .or_cancel(&cancellation_token)
            .await
        {
            Ok(mcp_tools) => mcp_tools,
            Err(_) => return None,
        };
        connectors::with_app_enabled_state(
            connectors::accessible_connectors_from_mcp_tools(&mcp_tools),
            &turn_context.config,
        )
    } else {
        Vec::new()
    };
    let connector_slug_counts = build_connector_slug_counts(&available_connectors);
    let skill_name_counts_lower = skills_outcome
        .as_ref()
        .map_or_else(HashMap::new, |outcome| {
            build_skill_name_counts(&outcome.skills, &outcome.disabled_paths).1
        });
    let mentioned_skills = skills_outcome.as_ref().map_or_else(Vec::new, |outcome| {
        collect_explicit_skill_mentions(
            &input,
            &outcome.skills,
            &outcome.disabled_paths,
            &connector_slug_counts,
        )
    });
    let config = turn_context.config.clone();
    if config
        .features
        .enabled(Feature::SkillEnvVarDependencyPrompt)
    {
        let env_var_dependencies = collect_env_var_dependencies(&mentioned_skills);
        resolve_skill_dependencies_for_turn(&sess, &turn_context, &env_var_dependencies).await;
    }

    maybe_prompt_and_install_mcp_dependencies(
        sess.as_ref(),
        turn_context.as_ref(),
        &cancellation_token,
        &mentioned_skills,
    )
    .await;

    let otel_manager = turn_context.otel_manager.clone();
    let thread_id = sess.conversation_id.to_string();
    let tracking = build_track_events_context(
        turn_context.model_info.slug.clone(),
        thread_id,
        turn_context.sub_id.clone(),
    );
    let SkillInjections {
        items: skill_items,
        warnings: skill_warnings,
    } = build_skill_injections(
        &mentioned_skills,
        Some(&otel_manager),
        &sess.services.analytics_events_client,
        tracking.clone(),
    )
    .await;

    for message in skill_warnings {
        sess.send_event(&turn_context, EventMsg::Warning(WarningEvent { message }))
            .await;
    }

    let mut explicitly_enabled_connectors = collect_explicit_app_ids(&input);
    explicitly_enabled_connectors.extend(collect_explicit_app_ids_from_skill_items(
        &skill_items,
        &available_connectors,
        &skill_name_counts_lower,
    ));
    let connector_names_by_id = available_connectors
        .iter()
        .map(|connector| (connector.id.as_str(), connector.name.as_str()))
        .collect::<HashMap<&str, &str>>();
    let mentioned_app_invocations = explicitly_enabled_connectors
        .iter()
        .map(|connector_id| AppInvocation {
            connector_id: Some(connector_id.clone()),
            app_name: connector_names_by_id
                .get(connector_id.as_str())
                .map(|name| (*name).to_string()),
            invocation_type: Some(InvocationType::Explicit),
        })
        .collect::<Vec<_>>();
    sess.services
        .analytics_events_client
        .track_app_mentioned(tracking.clone(), mentioned_app_invocations);
    sess.merge_connector_selection(explicitly_enabled_connectors.clone())
        .await;

    let initial_input_for_turn: ResponseInputItem = ResponseInputItem::from(input.clone());
    let response_item: ResponseItem = initial_input_for_turn.clone().into();
    sess.record_user_prompt_and_emit_turn_item(turn_context.as_ref(), &input, response_item)
        .await;
    // Track the previous-model baseline from the regular user-turn path only so
    // standalone tasks (compact/shell/review/undo) cannot suppress future
    // `<model_switch>` injections.
    sess.set_previous_model(Some(turn_context.model_info.slug.clone()))
        .await;

    if !skill_items.is_empty() {
        sess.record_conversation_items(&turn_context, &skill_items)
            .await;
    }

    sess.maybe_start_ghost_snapshot(Arc::clone(&turn_context), cancellation_token.child_token())
        .await;
    let mut last_agent_message: Option<String> = None;
    // Although from the perspective of codex.rs, TurnDiffTracker has the lifecycle of a Task which contains
    // many turns, from the perspective of the user, it is a single turn.
    let turn_diff_tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));
    let mut server_model_warning_emitted_for_turn = false;

    // `ModelClientSession` is turn-scoped and caches WebSocket + sticky routing state, so we reuse
    // one instance across retries within this turn.
    // Startup prewarm is tied to the session-default provider. If the user changes model family
    // before the first turn (e.g. Gemini -> GPT), discard that prewarmed session and create a
    // provider-matched one so requests hit the right base URL/API.
    let mut client_session = match prewarmed_client_session {
        Some(client_session)
            if sess
                .services
                .model_client
                .session_provider_matches(&turn_context.provider) =>
        {
            client_session
        }
        _ => sess
            .services
            .model_client
            .new_session_for_provider(&turn_context.provider),
    };

    loop {
        // Note that pending_input would be something like a message the user
        // submitted through the UI while the model was running. Though the UI
        // may support this, the model might not.
        let pending_response_items = sess
            .get_pending_input()
            .await
            .into_iter()
            .map(ResponseItem::from)
            .collect::<Vec<ResponseItem>>();

        if !pending_response_items.is_empty() {
            for response_item in pending_response_items {
                if let Some(TurnItem::UserMessage(user_message)) = parse_turn_item(&response_item) {
                    // todo(aibrahim): move pending input to be UserInput only to keep TextElements. context: https://github.com/openai/codex/pull/10656#discussion_r2765522480
                    sess.record_user_prompt_and_emit_turn_item(
                        turn_context.as_ref(),
                        &user_message.content,
                        response_item,
                    )
                    .await;
                } else {
                    sess.record_conversation_items(
                        &turn_context,
                        std::slice::from_ref(&response_item),
                    )
                    .await;
                }
            }
        }

        // Construct the input that we will send to the model.
        let sampling_request_input: Vec<ResponseItem> = {
            sess.clone_history()
                .await
                .for_prompt(&turn_context.model_info.input_modalities)
        };

        let sampling_request_input_messages = sampling_request_input
            .iter()
            .filter_map(|item| match parse_turn_item(item) {
                Some(TurnItem::UserMessage(user_message)) => Some(user_message),
                _ => None,
            })
            .map(|user_message| user_message.message())
            .collect::<Vec<String>>();
        let turn_metadata_header = turn_context.turn_metadata_state.current_header_value();
        match run_sampling_request(
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            Arc::clone(&turn_diff_tracker),
            &mut client_session,
            turn_metadata_header.as_deref(),
            sampling_request_input,
            &explicitly_enabled_connectors,
            skills_outcome,
            &mut server_model_warning_emitted_for_turn,
            cancellation_token.child_token(),
        )
        .await
        {
            Ok(outcome) => {
                let SamplingRequestOutcome {
                    result: sampling_request_output,
                    turn_context: updated_turn_context,
                } = outcome;
                turn_context = updated_turn_context;
                let SamplingRequestResult {
                    needs_follow_up,
                    last_agent_message: sampling_request_last_agent_message,
                } = sampling_request_output;
                let total_usage_tokens = sess.get_total_token_usage().await;
                let token_limit_reached = total_usage_tokens >= auto_compact_limit;

                let estimated_token_count =
                    sess.get_estimated_token_count(turn_context.as_ref()).await;

                trace!(
                    turn_id = %turn_context.sub_id,
                    total_usage_tokens,
                    estimated_token_count = ?estimated_token_count,
                    auto_compact_limit,
                    token_limit_reached,
                    needs_follow_up,
                    "post sampling token usage"
                );

                // as long as compaction works well in getting us way below the token limit, we shouldn't worry about being in an infinite loop.
                if token_limit_reached && needs_follow_up {
                    if run_auto_compact(
                        &sess,
                        &turn_context,
                        InitialContextInjection::BeforeLastUserMessage,
                        previous_model.as_deref(),
                    )
                    .await
                    .is_err()
                    {
                        return None;
                    }
                    continue;
                }

                if !needs_follow_up {
                    last_agent_message = sampling_request_last_agent_message;
                    let memory_context = turn_context.resolve_hook_memory_context().await;
                    let memory = turn_context.resolve_memory_link().await;

                    let mut sampling_request_input_messages = sampling_request_input_messages;

                    if turn_context.config.memories.entire_summary_enabled {
                        let user_prompt = sampling_request_input_messages.join("\n");
                        let ai_response = last_agent_message.clone().unwrap_or_default();

                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> =
                            side_effects_guard.iter().cloned().collect();
                        drop(side_effects_guard);

                        let has_files_changed = !files_changed.is_empty();
                        let is_trivial_prompt = sampling_request_input_messages.len() == 1
                            && sampling_request_input_messages[0].len() < 10
                            && !has_files_changed;

                        if !is_trivial_prompt {
                            sess.notify_background_event(
                                &turn_context,
                                "Generating Entire session summary...".to_string(),
                            )
                            .await;
                        }

                        let input = codex_hooks::EntireSummaryInput {
                            thread_id: sess.conversation_id.to_string(),
                            turn_id: turn_context.sub_id.clone(),
                            user_prompt,
                            ai_response,
                            files_changed,
                        };
                        let (
                            summary_model_client,
                            summary_model_info,
                            summary_model_slug,
                            background_message,
                        ) = sess
                            .entire_summary_client_and_model_for_turn(turn_context.as_ref())
                            .await;
                        if let Some(message) = background_message {
                            sess.notify_background_event(turn_context.as_ref(), message)
                                .await;
                        }

                        if let Ok(summary) =
                            crate::entire_summary_generator::generate_entire_summary_with_client_and_model(
                                &input,
                                &summary_model_client,
                                &summary_model_info,
                                &summary_model_slug,
                            )
                            .await
                        {
                            let repo_root = turn_context.cwd.clone();
                            if let Err(e) = codex_hooks::save_summary(
                                &repo_root,
                                &turn_context.sub_id,
                                &summary,
                            )
                            .await
                            {
                                tracing::warn!("Failed to save Entire summary: {}", e);
                            }

                            if summary.is_meaningful {
                                let commit_message = format!(
                                    "{} → {}\n\nMotivation: {}\nApproach: {}\nChallenges: {}\nTradeoffs: {}",
                                    summary.motivation.as_deref().unwrap_or("N/A"),
                                    summary.outcome.as_deref().unwrap_or("N/A"),
                                    summary.motivation.as_deref().unwrap_or("N/A"),
                                    summary.approach.as_deref().unwrap_or("N/A"),
                                    summary.challenges.as_deref().unwrap_or("None"),
                                    summary.tradeoffs.as_deref().unwrap_or("None")
                                );
                                if !sampling_request_input_messages.is_empty() {
                                    sampling_request_input_messages[0] = commit_message;
                                }
                            }
                        }
                    }

                    let memory_scope_version = memory
                        .as_ref()
                        .and_then(|memory| memory.scope_version.clone());
                    let memory_scope_kind =
                        memory.as_ref().and_then(|memory| memory.scope_kind.clone());
                    let memory_summary_sha256 = memory
                        .as_ref()
                        .and_then(|memory| memory.summary_sha256.clone());
                    let memory_binding_key = memory
                        .as_ref()
                        .and_then(|memory| memory.binding_key.clone());
                    let hook_outcomes = sess
                        .hooks()
                        .dispatch(HookPayload {
                            session_id: sess.conversation_id,
                            cwd: turn_context.cwd.clone(),
                            triggered_at: chrono::Utc::now(),
                            hook_event: HookEvent::AfterAgent {
                                event: HookEventAfterAgent {
                                    thread_id: sess.conversation_id,
                                    turn_id: turn_context.sub_id.clone(),
                                    input_messages: sampling_request_input_messages,
                                    last_assistant_message: last_agent_message.clone(),
                                    provider_name: turn_context.provider.name.clone(),
                                    model_slug: turn_context.model_info.slug.clone(),
                                    memory,
                                    memory_scope_version,
                                    memory_scope_kind,
                                    memory_summary_sha256,
                                    memory_binding_key,
                                    memory_context,
                                },
                            },
                        })
                        .await;

                    let mut abort_message = None;
                    for hook_outcome in hook_outcomes {
                        let hook_name = hook_outcome.hook_name;
                        match hook_outcome.result {
                            HookResult::Success => {}
                            HookResult::FailedContinue(error) => {
                                warn!(
                                    turn_id = %turn_context.sub_id,
                                    hook_name = %hook_name,
                                    error = %error,
                                    "after_agent hook failed; continuing"
                                );
                            }
                            HookResult::FailedAbort(error) => {
                                let message = format!(
                                    "after_agent hook '{hook_name}' failed and aborted turn completion: {error}"
                                );
                                warn!(
                                    turn_id = %turn_context.sub_id,
                                    hook_name = %hook_name,
                                    error = %error,
                                    "after_agent hook failed; aborting operation"
                                );
                                if abort_message.is_none() {
                                    abort_message = Some(message);
                                }
                            }
                        }
                    }
                    if let Some(message) = abort_message {
                        sess.send_event(
                            &turn_context,
                            EventMsg::Error(ErrorEvent {
                                message,
                                codex_error_info: None,
                            }),
                        )
                        .await;
                        return None;
                    }
                    break;
                }
                continue;
            }
            Err(CodexErr::TurnAborted) => {
                // Aborted turn is reported via a different event.
                break;
            }
            Err(CodexErr::InvalidImageRequest()) => {
                let mut state = sess.state.lock().await;
                error_or_panic(
                    "Invalid image detected; sanitizing tool output to prevent poisoning",
                );
                if state.history.replace_last_turn_images("Invalid image") {
                    continue;
                }
                let event = EventMsg::Error(ErrorEvent {
                    message: "Invalid image in your last message. Please remove it and try again."
                        .to_string(),
                    codex_error_info: Some(CodexErrorInfo::BadRequest),
                });
                sess.send_event(&turn_context, event).await;
                break;
            }
            Err(e) => {
                info!("Turn error: {e:#}");
                let event = EventMsg::Error(e.to_error_event(None));
                sess.send_event(&turn_context, event).await;
                // let the user continue the conversation
                break;
            }
        }
    }

    last_agent_message
}

pub(crate) async fn build_hook_memory_context(
    turn_context: &TurnContext,
) -> Option<HookEventMemoryContext> {
    if !turn_context.features.enabled(Feature::MemoryTool) {
        return None;
    }

    let cwd_scope_key = memories::memory_scope_key_for_cwd(&turn_context.cwd);
    let cwd_memory_root =
        memories::memory_root_for_cwd(&turn_context.config.codex_home, &turn_context.cwd);
    let cwd_memory_summary_path = memories::memory_summary_file(&cwd_memory_root);
    let user_memory_root = memories::memory_root_for_user(&turn_context.config.codex_home);
    let user_memory_summary_path = memories::memory_summary_file(&user_memory_root);

    let cwd_memory_summary_exists = tokio::fs::try_exists(&cwd_memory_summary_path)
        .await
        .unwrap_or(false);
    let user_memory_summary_exists = tokio::fs::try_exists(&user_memory_summary_path)
        .await
        .unwrap_or(false);
    let active_source = turn_context.resolve_memory_read_path_source().await;
    let active_scope_kind = active_source
        .as_ref()
        .map(|active_source| active_source.scope_kind.to_string());
    let active_memory_root = active_source
        .as_ref()
        .map(|active_source| active_source.memory_root.display().to_string());
    let active_memory_summary_path = active_source
        .as_ref()
        .map(|active_source| active_source.memory_summary_path.display().to_string());
    let active_memory_summary_sha256 = active_source
        .as_ref()
        .map(|active_source| active_source.memory_summary_sha256.clone());
    let active_memory_summary_bytes = active_source
        .as_ref()
        .and_then(|active_source| u64::try_from(active_source.memory_summary.len()).ok());
    let active_memory_scope_version = active_source
        .as_ref()
        .map(|active_source| active_source.memory_scope_version.clone());
    let active_memory_binding_key = active_source
        .as_ref()
        .map(|active_source| active_source.memory_binding_key.clone());

    Some(HookEventMemoryContext {
        cwd_scope_key,
        cwd_memory_root: cwd_memory_root.display().to_string(),
        cwd_memory_summary_path: cwd_memory_summary_path.display().to_string(),
        cwd_memory_summary_exists,
        user_memory_root: user_memory_root.display().to_string(),
        user_memory_summary_path: user_memory_summary_path.display().to_string(),
        user_memory_summary_exists,
        active_scope_kind,
        active_memory_root,
        active_memory_summary_path,
        active_memory_summary_sha256,
        active_memory_summary_bytes,
        active_memory_scope_version,
        active_memory_binding_key,
    })
}

async fn run_pre_sampling_compact(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
) -> CodexResult<()> {
    let total_usage_tokens_before_compaction = sess.get_total_token_usage().await;
    maybe_run_previous_model_inline_compact(
        sess,
        turn_context,
        total_usage_tokens_before_compaction,
    )
    .await?;
    let total_usage_tokens = sess.get_total_token_usage().await;
    let auto_compact_limit = turn_context
        .model_info
        .auto_compact_token_limit()
        .unwrap_or(i64::MAX);
    // Compact if the total usage tokens are greater than the auto compact limit
    if total_usage_tokens >= auto_compact_limit {
        run_auto_compact(
            sess,
            turn_context,
            InitialContextInjection::DoNotInject,
            None,
        )
        .await?;
    }
    Ok(())
}

/// Runs pre-sampling compaction against the previous model when switching to a smaller
/// context-window model.
///
/// Returns `Ok(true)` when compaction ran successfully, `Ok(false)` when compaction was skipped
/// because the model/context-window preconditions were not met, and `Err(_)` only when compaction
/// was attempted and failed.
async fn maybe_run_previous_model_inline_compact(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    total_usage_tokens: i64,
) -> CodexResult<bool> {
    let Some(previous_model) = sess.previous_model().await else {
        return Ok(false);
    };
    let (previous_model_turn_context, background_message) = sess
        .turn_context_with_model_resolved_from_pool(turn_context.as_ref(), previous_model)
        .await;
    let previous_model_turn_context = Arc::new(previous_model_turn_context);
    if let Some(message) = background_message {
        sess.notify_background_event(&previous_model_turn_context, message)
            .await;
    }

    let Some(old_context_window) = previous_model_turn_context.model_context_window() else {
        return Ok(false);
    };
    let Some(new_context_window) = turn_context.model_context_window() else {
        return Ok(false);
    };
    let new_auto_compact_limit = turn_context
        .model_info
        .auto_compact_token_limit()
        .unwrap_or(i64::MAX);
    let should_run = total_usage_tokens > new_auto_compact_limit
        && previous_model_turn_context.model_info.slug != turn_context.model_info.slug
        && old_context_window > new_context_window;
    if should_run {
        run_auto_compact(
            sess,
            &previous_model_turn_context,
            InitialContextInjection::DoNotInject,
            None,
        )
        .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn run_auto_compact(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    initial_context_injection: InitialContextInjection,
    previous_user_turn_model: Option<&str>,
) -> CodexResult<()> {
    if should_use_remote_compact_task(&turn_context.provider) {
        run_inline_remote_auto_compact_task(
            Arc::clone(sess),
            Arc::clone(turn_context),
            initial_context_injection,
            previous_user_turn_model,
        )
        .await?;
    } else {
        run_inline_auto_compact_task(
            Arc::clone(sess),
            Arc::clone(turn_context),
            initial_context_injection,
            previous_user_turn_model,
        )
        .await?;
    }
    Ok(())
}

fn should_switch_provider_account(err: &CodexErr, retries: u64, max_retries: u64) -> bool {
    // Auth / quota errors should immediately try the next pool account.
    if matches!(
        err,
        CodexErr::EnvVar(_)
            | CodexErr::RetryLimit(_)
            | CodexErr::UsageLimitReached(_)
            | CodexErr::InvalidRequest(_)
    ) {
        return true;
    }
    if let Some(status) = err.http_status_code_value()
        && matches!(status, 400 | 401 | 403 | 429)
    {
        return true;
    }
    err.is_retryable() && retries >= max_retries
}

const PROVIDER_POOL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
struct ResolvedTurnProvider {
    provider: ModelProviderInfo,
    background_message: Option<String>,
}

fn normalize_account_pool_in_config_order(
    provider_id: &str,
    provider: &ModelProviderInfo,
) -> Vec<ModelProviderAccount> {
    if provider.account_pool.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    provider
        .account_pool
        .iter()
        .cloned()
        .filter_map(|account| {
            let base_url = account
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let env_key = account
                .env_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let normalized = ModelProviderAccount { base_url, env_key };
            if normalized.base_url.is_none() || normalized.env_key.is_none() {
                warn!(
                    "Skipping account entry for provider {provider_id}: missing base_url or env_key"
                );
                None
            } else if seen.insert(normalized.clone()) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

fn next_account_from_pool(
    provider_id: &str,
    provider: &ModelProviderInfo,
    current_account: Option<&ModelProviderAccount>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
) -> Option<ModelProviderAccount> {
    let pool = normalize_account_pool_in_config_order(provider_id, provider);
    let pool_len = pool.len();
    if pool_len == 0 {
        return None;
    }

    let start_index = current_account
        .and_then(|account| pool.iter().position(|item| item == account))
        .map(|index| (index + 1) % pool_len)
        .unwrap_or(0);

    for offset in 0..pool_len {
        let index = (start_index + offset) % pool_len;
        let account = pool[index].clone();
        if attempted_accounts.insert(account.clone()) {
            return Some(account);
        }
    }

    None
}

/// Return a human-readable label like "key 1/3" indicating which account
/// from the pool is currently active. Falls back to the env_key name when
/// there is no pool.
fn account_index_label(provider: &ModelProviderInfo) -> String {
    if let Some(current) = provider.current_account() {
        let pool = normalize_account_pool_in_config_order("", provider);
        if pool.len() > 1
            && let Some(idx) = pool.iter().position(|a| a == &current)
        {
            return format!("key {}/{}", idx + 1, pool.len());
        }
        current.env_key.unwrap_or_else(|| "<default>".to_string())
    } else {
        "<no account>".to_string()
    }
}

fn resolve_turn_provider_from_pool(
    state: &mut SessionState,
    provider_id: &str,
    provider: &ModelProviderInfo,
    now: std::time::Instant,
) -> ResolvedTurnProvider {
    let pool = normalize_account_pool_in_config_order(provider_id, provider);
    if pool.is_empty() {
        return ResolvedTurnProvider {
            provider: provider.clone(),
            background_message: None,
        };
    }

    let mut cooled_indices = Vec::new();
    for (index, account) in pool.iter().enumerate() {
        if state
            .pool_cooldown_until(provider_id, account, now)
            .is_some()
        {
            cooled_indices.push(index);
            continue;
        }

        let background_message = if pool.len() == 1 {
            None
        } else if cooled_indices.is_empty() {
            Some(format!(
                "Provider pool {provider_id}: trying key {}/{}",
                index + 1,
                pool.len()
            ))
        } else {
            let skipped_keys = if cooled_indices.len() == 1 {
                format!("key {}/{}", cooled_indices[0] + 1, pool.len())
            } else {
                let keys = cooled_indices
                    .iter()
                    .map(|skipped_index| format!("{}/{}", skipped_index + 1, pool.len()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("keys {keys}")
            };
            Some(format!(
                "Provider pool {provider_id}: {skipped_keys} cooling down; trying key {}/{}",
                index + 1,
                pool.len()
            ))
        };

        return ResolvedTurnProvider {
            provider: provider.with_account(account),
            background_message,
        };
    }

    ResolvedTurnProvider {
        provider: provider.with_account(&pool[0]),
        background_message: Some(format!(
            "Provider pool {provider_id}: all keys cooling down; forcing fresh probe from key 1/{}",
            pool.len()
        )),
    }
}

/// Wrapper around `maybe_switch_provider_account` that supports cycling through
/// the account pool multiple rounds. When all accounts in the pool have been
/// attempted in the current round, the `attempted_accounts` set is reset and a
/// new round begins — up to `max_rounds` total rounds.
async fn try_switch_pool_account(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
    pool_switch_count: &mut usize,
    pool_size: usize,
    max_rounds: usize,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
) -> Option<Arc<TurnContext>> {
    // First try within the current round.
    if let Some(ctx) = maybe_switch_provider_account(
        sess,
        turn_context,
        attempted_accounts,
        false,
        err,
        retries,
        max_retries,
    )
    .await
    {
        *pool_switch_count += 1;
        return Some(ctx);
    }

    // Current round exhausted. Check if we can start a new round.
    if !should_switch_provider_account(err, retries, max_retries) {
        return None;
    }
    let completed_rounds = if pool_size > 0 {
        (*pool_switch_count + 1) / pool_size
    } else {
        max_rounds
    };
    if completed_rounds >= max_rounds {
        return None;
    }

    // Reset for the next round and try again.
    attempted_accounts.clear();
    let ctx = maybe_switch_provider_account(
        sess,
        turn_context,
        attempted_accounts,
        true,
        err,
        retries,
        max_retries,
    )
    .await?;
    *pool_switch_count += 1;
    Some(ctx)
}

async fn maybe_switch_provider_account(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
    restart_from_first: bool,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
) -> Option<Arc<TurnContext>> {
    if !should_switch_provider_account(err, retries, max_retries) {
        return None;
    }

    let current_account = turn_context.provider.current_account()?;
    let provider_id = turn_context.config.model_provider_id.clone();
    let now = std::time::Instant::now();
    let mut session_configuration = {
        let mut state = sess.state.lock().await;
        state.mark_pool_account_cooling(
            provider_id.as_str(),
            current_account.clone(),
            now,
            PROVIDER_POOL_COOLDOWN,
        );
        state.session_configuration.clone()
    };
    session_configuration.provider_id = turn_context.config.model_provider_id.clone();
    session_configuration.provider = turn_context.config.model_provider.clone();
    session_configuration.collaboration_mode = turn_context.collaboration_mode.clone();
    session_configuration.model_reasoning_summary = turn_context.reasoning_summary;
    session_configuration.developer_instructions = turn_context.developer_instructions.clone();
    session_configuration.user_instructions = turn_context.user_instructions.clone();
    session_configuration.personality = turn_context.personality;
    session_configuration.compact_prompt = turn_context.compact_prompt.clone();
    session_configuration.approval_policy = turn_context.approval_policy.clone();
    session_configuration.sandbox_policy = turn_context.sandbox_policy.clone();
    session_configuration.windows_sandbox_level = turn_context.windows_sandbox_level;
    session_configuration.cwd = turn_context.cwd.clone();
    session_configuration.original_config_do_not_use = Arc::clone(&turn_context.config);
    session_configuration.session_source = turn_context.session_source.clone();
    session_configuration.dynamic_tools = turn_context.dynamic_tools.clone();
    let next_account = next_account_from_pool(
        provider_id.as_str(),
        &turn_context.provider,
        (!restart_from_first).then_some(&current_account),
        attempted_accounts,
    )?;
    let next_provider = turn_context.provider.with_account(&next_account);
    let updated_context = sess
        .new_turn_from_resolved_provider(
            turn_context.sub_id.clone(),
            session_configuration,
            next_provider.clone(),
            Some(turn_context.final_output_json_schema.clone()),
            false,
        )
        .await;
    let current_label = account_index_label(&turn_context.provider);
    let next_label = account_index_label(&next_provider);
    let cooldown_minutes = PROVIDER_POOL_COOLDOWN.as_secs() / 60;
    let action = if restart_from_first {
        format!("all keys already tried; forcing fresh probe from {next_label}")
    } else {
        format!("switching to {next_label}")
    };
    sess.notify_background_event(
        updated_context.as_ref(),
        format!(
            "Provider pool {provider_id}: {current_label} failed ({err}); cooling for {cooldown_minutes}m, {action}"
        ),
    )
    .await;

    Some(updated_context)
}

fn collect_explicit_app_ids_from_skill_items(
    skill_items: &[ResponseItem],
    connectors: &[connectors::AppInfo],
    skill_name_counts_lower: &HashMap<String, usize>,
) -> HashSet<String> {
    if skill_items.is_empty() || connectors.is_empty() {
        return HashSet::new();
    }

    let skill_messages = skill_items
        .iter()
        .filter_map(|item| match item {
            ResponseItem::Message { content, .. } => {
                content.iter().find_map(|content_item| match content_item {
                    ContentItem::InputText { text } => Some(text.clone()),
                    _ => None,
                })
            }
            _ => None,
        })
        .collect::<Vec<String>>();
    if skill_messages.is_empty() {
        return HashSet::new();
    }

    let mentions = collect_tool_mentions_from_messages(&skill_messages);
    let mention_names_lower = mentions
        .plain_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<HashSet<String>>();
    let mut connector_ids = mentions
        .paths
        .iter()
        .filter(|path| tool_kind_for_path(path) == ToolMentionKind::App)
        .filter_map(|path| app_id_from_path(path).map(str::to_string))
        .collect::<HashSet<String>>();

    let connector_slug_counts = build_connector_slug_counts(connectors);
    for connector in connectors {
        let slug = connectors::connector_mention_slug(connector);
        let connector_count = connector_slug_counts.get(&slug).copied().unwrap_or(0);
        let skill_count = skill_name_counts_lower.get(&slug).copied().unwrap_or(0);
        if connector_count == 1 && skill_count == 0 && mention_names_lower.contains(&slug) {
            connector_ids.insert(connector.id.clone());
        }
    }

    connector_ids
}

fn filter_connectors_for_input(
    connectors: &[connectors::AppInfo],
    input: &[ResponseItem],
    explicitly_enabled_connectors: &HashSet<String>,
    skill_name_counts_lower: &HashMap<String, usize>,
) -> Vec<connectors::AppInfo> {
    let connectors: Vec<connectors::AppInfo> = connectors
        .iter()
        .filter(|connector| connector.is_enabled)
        .cloned()
        .collect::<Vec<_>>();
    if connectors.is_empty() {
        return Vec::new();
    }

    let user_messages = collect_user_messages(input);
    if user_messages.is_empty() && explicitly_enabled_connectors.is_empty() {
        return Vec::new();
    }

    let mentions = collect_tool_mentions_from_messages(&user_messages);
    let mention_names_lower = mentions
        .plain_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<HashSet<String>>();

    let connector_slug_counts = build_connector_slug_counts(&connectors);
    let mut allowed_connector_ids = explicitly_enabled_connectors.clone();
    for path in mentions
        .paths
        .iter()
        .filter(|path| tool_kind_for_path(path) == ToolMentionKind::App)
    {
        if let Some(connector_id) = app_id_from_path(path) {
            allowed_connector_ids.insert(connector_id.to_string());
        }
    }

    connectors
        .into_iter()
        .filter(|connector| {
            connector_inserted_in_messages(
                connector,
                &mention_names_lower,
                &allowed_connector_ids,
                &connector_slug_counts,
                skill_name_counts_lower,
            )
        })
        .collect()
}

fn connector_inserted_in_messages(
    connector: &connectors::AppInfo,
    mention_names_lower: &HashSet<String>,
    allowed_connector_ids: &HashSet<String>,
    connector_slug_counts: &HashMap<String, usize>,
    skill_name_counts_lower: &HashMap<String, usize>,
) -> bool {
    if allowed_connector_ids.contains(&connector.id) {
        return true;
    }

    let mention_slug = connectors::connector_mention_slug(connector);
    let connector_count = connector_slug_counts
        .get(&mention_slug)
        .copied()
        .unwrap_or(0);
    let skill_count = skill_name_counts_lower
        .get(&mention_slug)
        .copied()
        .unwrap_or(0);
    connector_count == 1 && skill_count == 0 && mention_names_lower.contains(&mention_slug)
}

fn filter_codex_apps_mcp_tools(
    mcp_tools: &HashMap<String, crate::mcp_connection_manager::ToolInfo>,
    connectors: &[connectors::AppInfo],
    config: &Config,
) -> HashMap<String, crate::mcp_connection_manager::ToolInfo> {
    let allowed: HashSet<&str> = connectors
        .iter()
        .map(|connector| connector.id.as_str())
        .collect();

    mcp_tools
        .iter()
        .filter(|(_, tool)| {
            if tool.server_name != CODEX_APPS_MCP_SERVER_NAME {
                return true;
            }
            let Some(connector_id) = codex_apps_connector_id(tool) else {
                return false;
            };
            allowed.contains(connector_id) && connectors::codex_app_tool_is_enabled(config, tool)
        })
        .map(|(name, tool)| (name.clone(), tool.clone()))
        .collect()
}

fn codex_apps_connector_id(tool: &crate::mcp_connection_manager::ToolInfo) -> Option<&str> {
    tool.connector_id.as_deref()
}

fn build_prompt(
    input: Vec<ResponseItem>,
    router: &ToolRouter,
    turn_context: &TurnContext,
    base_instructions: BaseInstructions,
    reference_images: Vec<String>,
    image_size: Option<crate::gemini_types::GeminiImageSize>,
    aspect_ratio: Option<crate::gemini_types::GeminiAspectRatio>,
) -> Prompt {
    Prompt {
        input,
        tools: router.specs(),
        parallel_tool_calls: turn_context.model_info.supports_parallel_tool_calls,
        base_instructions,
        personality: turn_context.personality,
        output_schema: turn_context.final_output_json_schema.clone(),
        reference_images,
        image_size,
        aspect_ratio,
    }
}
#[allow(clippy::too_many_arguments)]
#[instrument(level = "trace",
    skip_all,
    fields(
        turn_id = %turn_context.sub_id,
        model = %turn_context.model_info.slug,
        cwd = %turn_context.cwd.display()
    )
)]
async fn run_sampling_request(
    sess: Arc<Session>,
    mut turn_context: Arc<TurnContext>,
    turn_diff_tracker: SharedTurnDiffTracker,
    client_session: &mut ModelClientSession,
    turn_metadata_header: Option<&str>,
    input: Vec<ResponseItem>,
    explicitly_enabled_connectors: &HashSet<String>,
    skills_outcome: Option<&SkillLoadOutcome>,
    server_model_warning_emitted_for_turn: &mut bool,
    cancellation_token: CancellationToken,
) -> CodexResult<SamplingRequestOutcome> {
    let router = built_tools(
        sess.as_ref(),
        turn_context.as_ref(),
        &input,
        explicitly_enabled_connectors,
        skills_outcome,
        &cancellation_token,
    )
    .await?;

    let base_instructions = sess.get_base_instructions().await;

    let (persisted_reference_images, image_size, aspect_ratio) = {
        let state = sess.state.lock().await;
        (
            state.reference_images().to_vec(),
            state.image_size(),
            state.aspect_ratio(),
        )
    };
    let reference_images = if persisted_reference_images.is_empty() {
        derive_reference_images_for_turn(&input)
    } else {
        persisted_reference_images
    };

    let prompt = build_prompt(
        input,
        router.as_ref(),
        turn_context.as_ref(),
        base_instructions,
        reference_images,
        image_size,
        aspect_ratio,
    );

    let mut retries = 0;
    // Track pool cycling: allow up to MAX_POOL_ROUNDS full rounds through the
    // account pool before giving up.
    const MAX_POOL_ROUNDS: usize = 2;
    let mut pool_switch_count: usize = 0;
    let pool_size = normalize_account_pool_in_config_order("", &turn_context.provider)
        .len()
        .max(1);
    let mut attempted_accounts = {
        let mut attempted = HashSet::new();
        if let Some(account) = turn_context.provider.current_account() {
            attempted.insert(account);
        }
        attempted
    };
    loop {
        let err = match try_run_sampling_request(
            Arc::clone(&router),
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            client_session,
            turn_metadata_header,
            Arc::clone(&turn_diff_tracker),
            server_model_warning_emitted_for_turn,
            &prompt,
            cancellation_token.child_token(),
        )
        .await
        {
            Ok(output) => {
                return Ok(SamplingRequestOutcome {
                    result: output,
                    turn_context,
                });
            }
            Err(CodexErr::ContextWindowExceeded) => {
                sess.set_total_tokens_full(&turn_context).await;
                return Err(CodexErr::ContextWindowExceeded);
            }
            Err(CodexErr::UsageLimitReached(e)) => {
                let rate_limits = e.rate_limits.clone();
                if let Some(rate_limits) = rate_limits {
                    sess.update_rate_limits(&turn_context, *rate_limits).await;
                }
                // Try switching to the next account key before giving up.
                let usage_err = CodexErr::UsageLimitReached(e);
                let max_retries = turn_context.provider.stream_max_retries();
                if let Some(updated_context) = try_switch_pool_account(
                    &sess,
                    &turn_context,
                    &mut attempted_accounts,
                    &mut pool_switch_count,
                    pool_size,
                    MAX_POOL_ROUNDS,
                    &usage_err,
                    0, // always eligible for account switch
                    max_retries,
                )
                .await
                {
                    turn_context = updated_context;
                    *client_session = sess
                        .services
                        .model_client
                        .new_session_for_provider(&turn_context.provider);
                    retries = 0;
                    continue;
                }
                return Err(usage_err);
            }
            Err(err) => err,
        };

        let max_retries = turn_context.provider.stream_max_retries();
        if let Some(updated_context) = try_switch_pool_account(
            &sess,
            &turn_context,
            &mut attempted_accounts,
            &mut pool_switch_count,
            pool_size,
            MAX_POOL_ROUNDS,
            &err,
            retries,
            max_retries,
        )
        .await
        {
            turn_context = updated_context;
            *client_session = sess
                .services
                .model_client
                .new_session_for_provider(&turn_context.provider);
            retries = 0;
            continue;
        }

        if !err.is_retryable() {
            return Err(err);
        }

        // Use the configured provider-specific stream retry budget.
        if retries >= max_retries
            && client_session
                .try_switch_fallback_transport(&turn_context.otel_manager, &turn_context.model_info)
        {
            sess.send_event(
                &turn_context,
                EventMsg::Warning(WarningEvent {
                    message: format!("Falling back from WebSockets to HTTPS transport. {err:#}"),
                }),
            )
            .await;
            retries = 0;
            continue;
        }
        if retries < max_retries {
            retries += 1;
            let delay = match &err {
                CodexErr::Stream(_, requested_delay) => {
                    requested_delay.unwrap_or_else(|| backoff(retries))
                }
                _ => backoff(retries),
            };
            warn!(
                "stream disconnected - retrying sampling request ({retries}/{max_retries} in {delay:?})...",
            );

            // In release builds, hide the first websocket retry notification to reduce noisy
            // transient reconnect messages. In debug builds, keep full visibility for diagnosis.
            let report_error = retries > 1
                || cfg!(debug_assertions)
                || sess
                    .services
                    .model_client
                    .active_ws_version(&turn_context.model_info)
                    .is_none();

            if report_error {
                // Surface retry information to any UI/front‑end so the
                // user understands what is happening instead of staring
                // at a seemingly frozen screen.
                sess.notify_stream_error(
                    &turn_context,
                    format!("Reconnecting... {retries}/{max_retries}"),
                    err,
                )
                .await;
            }
            tokio::time::sleep(delay).await;
        } else {
            return Err(err);
        }
    }
}

async fn built_tools(
    sess: &Session,
    turn_context: &TurnContext,
    input: &[ResponseItem],
    explicitly_enabled_connectors: &HashSet<String>,
    skills_outcome: Option<&SkillLoadOutcome>,
    cancellation_token: &CancellationToken,
) -> CodexResult<Arc<ToolRouter>> {
    let mcp_connection_manager = sess.services.mcp_connection_manager.read().await;
    let has_mcp_servers = mcp_connection_manager.has_servers();
    let mut mcp_tools = mcp_connection_manager
        .list_all_tools()
        .or_cancel(cancellation_token)
        .await?;
    drop(mcp_connection_manager);

    let mut effective_explicitly_enabled_connectors = explicitly_enabled_connectors.clone();
    effective_explicitly_enabled_connectors.extend(sess.get_connector_selection().await);

    let connectors = if turn_context.features.enabled(Feature::Apps) {
        Some(connectors::with_app_enabled_state(
            connectors::accessible_connectors_from_mcp_tools(&mcp_tools),
            &turn_context.config,
        ))
    } else {
        None
    };

    let app_tools = connectors.as_ref().map(|connectors| {
        filter_codex_apps_mcp_tools(&mcp_tools, connectors, &turn_context.config)
    });

    if let Some(connectors) = connectors.as_ref() {
        let skill_name_counts_lower = skills_outcome.map_or_else(HashMap::new, |outcome| {
            build_skill_name_counts(&outcome.skills, &outcome.disabled_paths).1
        });

        let explicitly_enabled = filter_connectors_for_input(
            connectors,
            input,
            &effective_explicitly_enabled_connectors,
            &skill_name_counts_lower,
        );

        let mut selected_mcp_tools = filter_non_codex_apps_mcp_tools_only(&mcp_tools);

        if let Some(selected_tools) = sess.get_mcp_tool_selection().await {
            selected_mcp_tools.extend(filter_mcp_tools_by_name(&mcp_tools, &selected_tools));
        }

        selected_mcp_tools.extend(filter_codex_apps_mcp_tools_only(
            &mcp_tools,
            explicitly_enabled.as_ref(),
        ));

        mcp_tools =
            connectors::filter_codex_apps_tools_by_policy(selected_mcp_tools, &turn_context.config);
    }

    Ok(Arc::new(ToolRouter::from_config(
        &turn_context.tools_config,
        has_mcp_servers.then(|| {
            mcp_tools
                .into_iter()
                .map(|(name, tool)| (name, tool.tool))
                .collect()
        }),
        app_tools,
        turn_context.dynamic_tools.as_slice(),
    )))
}

#[derive(Debug)]
struct SamplingRequestOutcome {
    result: SamplingRequestResult,
    turn_context: Arc<TurnContext>,
}

#[derive(Debug)]
struct SamplingRequestResult {
    needs_follow_up: bool,
    last_agent_message: Option<String>,
}

/// Ephemeral per-response state for streaming a single proposed plan.
/// This is intentionally not persisted or stored in session/state since it
/// only exists while a response is actively streaming. The final plan text
/// is extracted from the completed assistant message.
/// Tracks a single proposed plan item across a streaming response.
struct ProposedPlanItemState {
    item_id: String,
    started: bool,
    completed: bool,
}

/// Aggregated state used only while streaming a plan-mode response.
/// Includes per-item parsers, deferred agent message bookkeeping, and the plan item lifecycle.
struct PlanModeStreamState {
    /// Agent message items started by the model but deferred until we see non-plan text.
    pending_agent_message_items: HashMap<String, TurnItem>,
    /// Agent message items whose start notification has been emitted.
    started_agent_message_items: HashSet<String>,
    /// Leading whitespace buffered until we see non-whitespace text for an item.
    leading_whitespace_by_item: HashMap<String, String>,
    /// Tracks plan item lifecycle while streaming plan output.
    plan_item_state: ProposedPlanItemState,
}

impl PlanModeStreamState {
    fn new(turn_id: &str) -> Self {
        Self {
            pending_agent_message_items: HashMap::new(),
            started_agent_message_items: HashSet::new(),
            leading_whitespace_by_item: HashMap::new(),
            plan_item_state: ProposedPlanItemState::new(turn_id),
        }
    }
}

#[derive(Debug, Default)]
struct AssistantMessageStreamParsers {
    plan_mode: bool,
    parsers_by_item: HashMap<String, AssistantTextStreamParser>,
}

type ParsedAssistantTextDelta = AssistantTextChunk;

impl AssistantMessageStreamParsers {
    fn new(plan_mode: bool) -> Self {
        Self {
            plan_mode,
            parsers_by_item: HashMap::new(),
        }
    }

    fn parser_mut(&mut self, item_id: &str) -> &mut AssistantTextStreamParser {
        let plan_mode = self.plan_mode;
        self.parsers_by_item
            .entry(item_id.to_string())
            .or_insert_with(|| AssistantTextStreamParser::new(plan_mode))
    }

    fn seed_item_text(&mut self, item_id: &str, text: &str) -> ParsedAssistantTextDelta {
        if text.is_empty() {
            return ParsedAssistantTextDelta::default();
        }
        self.parser_mut(item_id).push_str(text)
    }

    fn parse_delta(&mut self, item_id: &str, delta: &str) -> ParsedAssistantTextDelta {
        self.parser_mut(item_id).push_str(delta)
    }

    fn finish_item(&mut self, item_id: &str) -> ParsedAssistantTextDelta {
        let Some(mut parser) = self.parsers_by_item.remove(item_id) else {
            return ParsedAssistantTextDelta::default();
        };
        parser.finish()
    }

    fn drain_finished(&mut self) -> Vec<(String, ParsedAssistantTextDelta)> {
        let parsers_by_item = std::mem::take(&mut self.parsers_by_item);
        parsers_by_item
            .into_iter()
            .map(|(item_id, mut parser)| (item_id, parser.finish()))
            .collect()
    }
}

impl ProposedPlanItemState {
    fn new(turn_id: &str) -> Self {
        Self {
            item_id: format!("{turn_id}-plan"),
            started: false,
            completed: false,
        }
    }

    async fn start(&mut self, sess: &Session, turn_context: &TurnContext) {
        if self.started || self.completed {
            return;
        }
        self.started = true;
        let item = TurnItem::Plan(PlanItem {
            id: self.item_id.clone(),
            text: String::new(),
        });
        sess.emit_turn_item_started(turn_context, &item).await;
    }

    async fn push_delta(&mut self, sess: &Session, turn_context: &TurnContext, delta: &str) {
        if self.completed {
            return;
        }
        if delta.is_empty() {
            return;
        }
        let event = PlanDeltaEvent {
            thread_id: sess.conversation_id.to_string(),
            turn_id: turn_context.sub_id.clone(),
            item_id: self.item_id.clone(),
            delta: delta.to_string(),
        };
        sess.send_event(turn_context, EventMsg::PlanDelta(event))
            .await;
    }

    async fn complete_with_text(
        &mut self,
        sess: &Session,
        turn_context: &TurnContext,
        text: String,
    ) {
        if self.completed || !self.started {
            return;
        }
        self.completed = true;
        let item = TurnItem::Plan(PlanItem {
            id: self.item_id.clone(),
            text,
        });
        sess.emit_turn_item_completed(turn_context, item).await;
    }
}

/// In plan mode we defer agent message starts until the parser emits non-plan
/// text. The parser buffers each line until it can rule out a tag prefix, so
/// plan-only outputs never show up as empty assistant messages.
async fn maybe_emit_pending_agent_message_start(
    sess: &Session,
    turn_context: &TurnContext,
    state: &mut PlanModeStreamState,
    item_id: &str,
) {
    if state.started_agent_message_items.contains(item_id) {
        return;
    }
    if let Some(item) = state.pending_agent_message_items.remove(item_id) {
        sess.emit_turn_item_started(turn_context, &item).await;
        state
            .started_agent_message_items
            .insert(item_id.to_string());
    }
}

/// Agent messages are text-only today; concatenate all text entries.
fn agent_message_text(item: &codex_protocol::items::AgentMessageItem) -> String {
    item.content
        .iter()
        .map(|entry| match entry {
            codex_protocol::items::AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect()
}

fn realtime_text_for_event(msg: &EventMsg) -> Option<String> {
    match msg {
        EventMsg::AgentMessage(event) => Some(event.message.clone()),
        EventMsg::ItemCompleted(event) => match &event.item {
            TurnItem::AgentMessage(item) => Some(agent_message_text(item)),
            _ => None,
        },
        EventMsg::Error(_)
        | EventMsg::Warning(_)
        | EventMsg::RealtimeConversationStarted(_)
        | EventMsg::RealtimeConversationRealtime(_)
        | EventMsg::RealtimeConversationClosed(_)
        | EventMsg::ModelReroute(_)
        | EventMsg::ContextCompacted(_)
        | EventMsg::ThreadRolledBack(_)
        | EventMsg::TurnStarted(_)
        | EventMsg::TurnComplete(_)
        | EventMsg::TokenCount(_)
        | EventMsg::UserMessage(_)
        | EventMsg::AgentMessageDelta(_)
        | EventMsg::AgentReasoning(_)
        | EventMsg::AgentReasoningDelta(_)
        | EventMsg::AgentReasoningRawContent(_)
        | EventMsg::AgentReasoningRawContentDelta(_)
        | EventMsg::AgentReasoningSectionBreak(_)
        | EventMsg::SessionConfigured(_)
        | EventMsg::ThreadNameUpdated(_)
        | EventMsg::McpStartupUpdate(_)
        | EventMsg::McpStartupComplete(_)
        | EventMsg::McpToolCallBegin(_)
        | EventMsg::McpToolCallEnd(_)
        | EventMsg::WebSearchBegin(_)
        | EventMsg::WebSearchEnd(_)
        | EventMsg::ExecCommandBegin(_)
        | EventMsg::ExecCommandOutputDelta(_)
        | EventMsg::TerminalInteraction(_)
        | EventMsg::ExecCommandEnd(_)
        | EventMsg::PatchApplyBegin(_)
        | EventMsg::PatchApplyEnd(_)
        | EventMsg::ViewImageToolCall(_)
        | EventMsg::ExecApprovalRequest(_)
        | EventMsg::RequestUserInput(_)
        | EventMsg::DynamicToolCallRequest(_)
        | EventMsg::DynamicToolCallResponse(_)
        | EventMsg::ElicitationRequest(_)
        | EventMsg::ApplyPatchApprovalRequest(_)
        | EventMsg::DeprecationNotice(_)
        | EventMsg::BackgroundEvent(_)
        | EventMsg::UndoStarted(_)
        | EventMsg::UndoCompleted(_)
        | EventMsg::StreamError(_)
        | EventMsg::TurnDiff(_)
        | EventMsg::GetHistoryEntryResponse(_)
        | EventMsg::McpListToolsResponse(_)
        | EventMsg::ListCustomPromptsResponse(_)
        | EventMsg::ListSkillsResponse(_)
        | EventMsg::ListRemoteSkillsResponse(_)
        | EventMsg::RemoteSkillDownloaded(_)
        | EventMsg::SkillsUpdateAvailable
        | EventMsg::PlanUpdate(_)
        | EventMsg::TurnAborted(_)
        | EventMsg::ShutdownComplete
        | EventMsg::EnteredReviewMode(_)
        | EventMsg::ExitedReviewMode(_)
        | EventMsg::RawResponseItem(_)
        | EventMsg::ItemStarted(_)
        | EventMsg::AgentMessageContentDelta(_)
        | EventMsg::PlanDelta(_)
        | EventMsg::ReasoningContentDelta(_)
        | EventMsg::ReasoningRawContentDelta(_)
        | EventMsg::CollabAgentSpawnBegin(_)
        | EventMsg::CollabAgentSpawnEnd(_)
        | EventMsg::CollabAgentInteractionBegin(_)
        | EventMsg::CollabAgentInteractionEnd(_)
        | EventMsg::CollabWaitingBegin(_)
        | EventMsg::CollabWaitingEnd(_)
        | EventMsg::CollabCloseBegin(_)
        | EventMsg::CollabCloseEnd(_)
        | EventMsg::CollabResumeBegin(_)
        | EventMsg::CollabResumeEnd(_)
        | EventMsg::GuardianAssessment(_)
        | EventMsg::FileSystemMutated(_) => None,
    }
}

/// Split the stream into normal assistant text vs. proposed plan content.
/// Normal text becomes AgentMessage deltas; plan content becomes PlanDelta +
/// TurnItem::Plan.
async fn handle_plan_segments(
    sess: &Session,
    turn_context: &TurnContext,
    state: &mut PlanModeStreamState,
    item_id: &str,
    segments: Vec<ProposedPlanSegment>,
) {
    for segment in segments {
        match segment {
            ProposedPlanSegment::Normal(delta) => {
                if delta.is_empty() {
                    continue;
                }
                let has_non_whitespace = delta.chars().any(|ch| !ch.is_whitespace());
                if !has_non_whitespace && !state.started_agent_message_items.contains(item_id) {
                    let entry = state
                        .leading_whitespace_by_item
                        .entry(item_id.to_string())
                        .or_default();
                    entry.push_str(&delta);
                    continue;
                }
                let delta = if !state.started_agent_message_items.contains(item_id) {
                    if let Some(prefix) = state.leading_whitespace_by_item.remove(item_id) {
                        format!("{prefix}{delta}")
                    } else {
                        delta
                    }
                } else {
                    delta
                };
                maybe_emit_pending_agent_message_start(sess, turn_context, state, item_id).await;

                let event = AgentMessageContentDeltaEvent {
                    thread_id: sess.conversation_id.to_string(),
                    turn_id: turn_context.sub_id.clone(),
                    item_id: item_id.to_string(),
                    delta,
                };
                sess.send_event(turn_context, EventMsg::AgentMessageContentDelta(event))
                    .await;
            }
            ProposedPlanSegment::ProposedPlanStart => {
                if !state.plan_item_state.completed {
                    state.plan_item_state.start(sess, turn_context).await;
                }
            }
            ProposedPlanSegment::ProposedPlanDelta(delta) => {
                if !state.plan_item_state.completed {
                    if !state.plan_item_state.started {
                        state.plan_item_state.start(sess, turn_context).await;
                    }
                    state
                        .plan_item_state
                        .push_delta(sess, turn_context, &delta)
                        .await;
                }
            }
            ProposedPlanSegment::ProposedPlanEnd => {}
        }
    }
}

async fn emit_streamed_assistant_text_delta(
    sess: &Session,
    turn_context: &TurnContext,
    plan_mode_state: Option<&mut PlanModeStreamState>,
    item_id: &str,
    parsed: ParsedAssistantTextDelta,
) {
    if parsed.is_empty() {
        return;
    }
    if !parsed.citations.is_empty() {
        // Citation extraction is intentionally local for now; we strip citations from display text
        // but do not yet surface them in protocol events.
        let _citations = parsed.citations;
    }
    if let Some(state) = plan_mode_state {
        if !parsed.plan_segments.is_empty() {
            handle_plan_segments(sess, turn_context, state, item_id, parsed.plan_segments).await;
        }
        return;
    }
    if parsed.visible_text.is_empty() {
        return;
    }
    let event = AgentMessageContentDeltaEvent {
        thread_id: sess.conversation_id.to_string(),
        turn_id: turn_context.sub_id.clone(),
        item_id: item_id.to_string(),
        delta: parsed.visible_text,
    };
    sess.send_event(turn_context, EventMsg::AgentMessageContentDelta(event))
        .await;
}

/// Flush buffered assistant text parser state when an assistant message item ends.
async fn flush_assistant_text_segments_for_item(
    sess: &Session,
    turn_context: &TurnContext,
    plan_mode_state: Option<&mut PlanModeStreamState>,
    parsers: &mut AssistantMessageStreamParsers,
    item_id: &str,
) {
    let parsed = parsers.finish_item(item_id);
    emit_streamed_assistant_text_delta(sess, turn_context, plan_mode_state, item_id, parsed).await;
}

/// Flush any remaining buffered assistant text parser state at response completion.
async fn flush_assistant_text_segments_all(
    sess: &Session,
    turn_context: &TurnContext,
    mut plan_mode_state: Option<&mut PlanModeStreamState>,
    parsers: &mut AssistantMessageStreamParsers,
) {
    for (item_id, parsed) in parsers.drain_finished() {
        emit_streamed_assistant_text_delta(
            sess,
            turn_context,
            plan_mode_state.as_deref_mut(),
            &item_id,
            parsed,
        )
        .await;
    }
}

/// Emit completion for plan items by parsing the finalized assistant message.
async fn maybe_complete_plan_item_from_message(
    sess: &Session,
    turn_context: &TurnContext,
    state: &mut PlanModeStreamState,
    item: &ResponseItem,
) {
    if let ResponseItem::Message { role, content, .. } = item
        && role == "assistant"
    {
        let mut text = String::new();
        for entry in content {
            if let ContentItem::OutputText { text: chunk } = entry {
                text.push_str(chunk);
            }
        }
        if let Some(plan_text) = extract_proposed_plan_text(&text) {
            let (plan_text, _citations) = strip_citations(&plan_text);
            if !state.plan_item_state.started {
                state.plan_item_state.start(sess, turn_context).await;
            }
            state
                .plan_item_state
                .complete_with_text(sess, turn_context, plan_text)
                .await;
        }
    }
}

/// Emit a completed agent message in plan mode, respecting deferred starts.
async fn emit_agent_message_in_plan_mode(
    sess: &Session,
    turn_context: &TurnContext,
    agent_message: codex_protocol::items::AgentMessageItem,
    state: &mut PlanModeStreamState,
) {
    let agent_message_id = agent_message.id.clone();
    let text = agent_message_text(&agent_message);
    if text.trim().is_empty() {
        state.pending_agent_message_items.remove(&agent_message_id);
        state.started_agent_message_items.remove(&agent_message_id);
        return;
    }

    maybe_emit_pending_agent_message_start(sess, turn_context, state, &agent_message_id).await;

    if !state
        .started_agent_message_items
        .contains(&agent_message_id)
    {
        let start_item = state
            .pending_agent_message_items
            .remove(&agent_message_id)
            .unwrap_or_else(|| {
                TurnItem::AgentMessage(codex_protocol::items::AgentMessageItem {
                    id: agent_message_id.clone(),
                    content: Vec::new(),
                    phase: None,
                })
            });
        sess.emit_turn_item_started(turn_context, &start_item).await;
        state
            .started_agent_message_items
            .insert(agent_message_id.clone());
    }

    sess.emit_turn_item_completed(turn_context, TurnItem::AgentMessage(agent_message))
        .await;
    state.started_agent_message_items.remove(&agent_message_id);
}

/// Emit completion for a plan-mode turn item, handling agent messages specially.
async fn emit_turn_item_in_plan_mode(
    sess: &Session,
    turn_context: &TurnContext,
    turn_item: TurnItem,
    previously_active_item: Option<&TurnItem>,
    state: &mut PlanModeStreamState,
) {
    match turn_item {
        TurnItem::AgentMessage(agent_message) => {
            emit_agent_message_in_plan_mode(sess, turn_context, agent_message, state).await;
        }
        _ => {
            if previously_active_item.is_none() {
                sess.emit_turn_item_started(turn_context, &turn_item).await;
            }
            sess.emit_turn_item_completed(turn_context, turn_item).await;
        }
    }
}

/// Handle a completed assistant response item in plan mode, returning true if handled.
async fn handle_assistant_item_done_in_plan_mode(
    sess: &Session,
    turn_context: &TurnContext,
    item: &ResponseItem,
    state: &mut PlanModeStreamState,
    previously_active_item: Option<&TurnItem>,
    last_agent_message: &mut Option<String>,
) -> bool {
    if let ResponseItem::Message { role, .. } = item
        && role == "assistant"
    {
        maybe_complete_plan_item_from_message(sess, turn_context, state, item).await;

        if let Some(turn_item) = handle_non_tool_response_item(item, true) {
            emit_turn_item_in_plan_mode(
                sess,
                turn_context,
                turn_item,
                previously_active_item,
                state,
            )
            .await;
        }

        record_completed_response_item(sess, turn_context, item).await;
        if let Some(agent_message) = last_assistant_message_from_item(item, true) {
            *last_agent_message = Some(agent_message);
        }
        return true;
    }
    false
}

async fn drain_in_flight(
    in_flight: &mut FuturesOrdered<BoxFuture<'static, CodexResult<ResponseInputItem>>>,
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
) -> CodexResult<()> {
    while let Some(res) = in_flight.next().await {
        match res {
            Ok(response_input) => {
                sess.record_conversation_items(&turn_context, &[response_input.into()])
                    .await;
            }
            Err(err) => {
                error_or_panic(format!("in-flight tool future failed during drain: {err}"));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[instrument(level = "trace",
    skip_all,
    fields(
        turn_id = %turn_context.sub_id,
        model = %turn_context.model_info.slug
    )
)]
async fn try_run_sampling_request(
    router: Arc<ToolRouter>,
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    client_session: &mut ModelClientSession,
    turn_metadata_header: Option<&str>,
    turn_diff_tracker: SharedTurnDiffTracker,
    server_model_warning_emitted_for_turn: &mut bool,
    prompt: &Prompt,
    cancellation_token: CancellationToken,
) -> CodexResult<SamplingRequestResult> {
    // Persist one TurnContext marker per sampling request (not just per user turn) so rollout
    // analysis can reconstruct API-turn boundaries. `run_turn` persists model-visible context
    // diffs/full reinjection earlier in the same regular turn before reaching this path.
    let rollout_item = RolloutItem::TurnContext(turn_context.to_turn_context_item());

    feedback_tags!(
        model = turn_context.model_info.slug.clone(),
        approval_policy = turn_context.approval_policy.value(),
        sandbox_policy = turn_context.sandbox_policy.get(),
        effort = turn_context.reasoning_effort,
        auth_mode = sess.services.auth_manager.auth_mode(),
        features = sess.features.enabled_features(),
    );

    sess.persist_rollout_items(&[rollout_item]).await;
    let mut stream = client_session
        .stream(
            prompt,
            &turn_context.model_info,
            &turn_context.otel_manager,
            turn_context.reasoning_effort,
            turn_context.reasoning_summary,
            turn_metadata_header,
        )
        .instrument(trace_span!("stream_request"))
        .or_cancel(&cancellation_token)
        .await??;

    let tool_runtime = ToolCallRuntime::new(
        Arc::clone(&router),
        Arc::clone(&sess),
        Arc::clone(&turn_context),
        Arc::clone(&turn_diff_tracker),
    );
    let mut in_flight: FuturesOrdered<BoxFuture<'static, CodexResult<ResponseInputItem>>> =
        FuturesOrdered::new();
    let mut needs_follow_up = false;
    let mut last_agent_message: Option<String> = None;
    let mut active_item: Option<TurnItem> = None;
    let mut should_emit_turn_diff = false;
    let plan_mode = turn_context.collaboration_mode.mode == ModeKind::Plan;
    let mut assistant_message_stream_parsers = AssistantMessageStreamParsers::new(plan_mode);
    let mut plan_mode_state = plan_mode.then(|| PlanModeStreamState::new(&turn_context.sub_id));
    let receiving_span = trace_span!("receiving_stream");
    let outcome: CodexResult<SamplingRequestResult> = loop {
        let handle_responses = trace_span!(
            parent: &receiving_span,
            "handle_responses",
            otel.name = field::Empty,
            tool_name = field::Empty,
            from = field::Empty,
        );

        let event = match stream
            .next()
            .instrument(trace_span!(parent: &handle_responses, "receiving"))
            .or_cancel(&cancellation_token)
            .await
        {
            Ok(event) => event,
            Err(codex_async_utils::CancelErr::Cancelled) => break Err(CodexErr::TurnAborted),
        };

        let event = match event {
            Some(res) => res?,
            None => {
                break Err(CodexErr::Stream(
                    "stream closed before response.completed".into(),
                    None,
                ));
            }
        };

        sess.services
            .otel_manager
            .record_responses(&handle_responses, &event);

        match event {
            ResponseEvent::Created => {}
            ResponseEvent::OutputItemDone(item) => {
                let previously_active_item = active_item.take();
                if let Some(previous) = previously_active_item.as_ref()
                    && matches!(previous, TurnItem::AgentMessage(_))
                {
                    let item_id = previous.id();
                    flush_assistant_text_segments_for_item(
                        &sess,
                        &turn_context,
                        plan_mode_state.as_mut(),
                        &mut assistant_message_stream_parsers,
                        &item_id,
                    )
                    .await;
                }
                if let Some(state) = plan_mode_state.as_mut()
                    && handle_assistant_item_done_in_plan_mode(
                        &sess,
                        &turn_context,
                        &item,
                        state,
                        previously_active_item.as_ref(),
                        &mut last_agent_message,
                    )
                    .await
                {
                    continue;
                }

                let mut ctx = HandleOutputCtx {
                    sess: sess.clone(),
                    turn_context: turn_context.clone(),
                    tool_runtime: tool_runtime.clone(),
                    cancellation_token: cancellation_token.child_token(),
                };

                let output_result = handle_output_item_done(&mut ctx, item, previously_active_item)
                    .instrument(handle_responses)
                    .await?;
                if let Some(tool_future) = output_result.tool_future {
                    in_flight.push_back(tool_future);
                }
                if let Some(agent_message) = output_result.last_agent_message {
                    last_agent_message = Some(agent_message);
                }
                needs_follow_up |= output_result.needs_follow_up;
            }
            ResponseEvent::OutputItemAdded(item) => {
                if let Some(turn_item) = handle_non_tool_response_item(&item, plan_mode) {
                    let mut turn_item = turn_item;
                    let mut seeded_parsed: Option<ParsedAssistantTextDelta> = None;
                    let mut seeded_item_id: Option<String> = None;
                    if matches!(turn_item, TurnItem::AgentMessage(_))
                        && let Some(raw_text) = raw_assistant_output_text_from_item(&item)
                    {
                        let item_id = turn_item.id();
                        let mut seeded =
                            assistant_message_stream_parsers.seed_item_text(&item_id, &raw_text);
                        if let TurnItem::AgentMessage(agent_message) = &mut turn_item {
                            agent_message.content =
                                vec![codex_protocol::items::AgentMessageContent::Text {
                                    text: if plan_mode {
                                        String::new()
                                    } else {
                                        std::mem::take(&mut seeded.visible_text)
                                    },
                                }];
                        }
                        seeded_parsed = plan_mode.then_some(seeded);
                        seeded_item_id = Some(item_id);
                    }
                    if let Some(state) = plan_mode_state.as_mut()
                        && matches!(turn_item, TurnItem::AgentMessage(_))
                    {
                        let item_id = turn_item.id();
                        state
                            .pending_agent_message_items
                            .insert(item_id, turn_item.clone());
                    } else {
                        sess.emit_turn_item_started(&turn_context, &turn_item).await;
                    }
                    if let (Some(state), Some(item_id), Some(parsed)) = (
                        plan_mode_state.as_mut(),
                        seeded_item_id.as_deref(),
                        seeded_parsed,
                    ) {
                        emit_streamed_assistant_text_delta(
                            &sess,
                            &turn_context,
                            Some(state),
                            item_id,
                            parsed,
                        )
                        .await;
                    }
                    active_item = Some(turn_item);
                }
            }
            ResponseEvent::ServerModel(server_model) => {
                if !*server_model_warning_emitted_for_turn
                    && sess
                        .maybe_warn_on_server_model_mismatch(&turn_context, server_model)
                        .await
                {
                    *server_model_warning_emitted_for_turn = true;
                }
            }
            ResponseEvent::ServerReasoningIncluded(included) => {
                sess.set_server_reasoning_included(included).await;
            }
            ResponseEvent::RateLimits(snapshot) => {
                // Update internal state with latest rate limits, but defer sending until
                // token usage is available to avoid duplicate TokenCount events.
                sess.update_rate_limits(&turn_context, snapshot).await;
            }
            ResponseEvent::ModelsEtag(etag) => {
                // Update internal state with latest models etag
                sess.services.models_manager.refresh_if_new_etag(etag).await;
            }
            ResponseEvent::Completed {
                response_id: _,
                token_usage,
                can_append: _,
            } => {
                flush_assistant_text_segments_all(
                    &sess,
                    &turn_context,
                    plan_mode_state.as_mut(),
                    &mut assistant_message_stream_parsers,
                )
                .await;
                sess.update_token_usage_info(&turn_context, token_usage.as_ref())
                    .await;
                should_emit_turn_diff = true;

                needs_follow_up |= sess.has_pending_input().await;

                break Ok(SamplingRequestResult {
                    needs_follow_up,
                    last_agent_message,
                });
            }
            ResponseEvent::OutputTextDelta(delta) => {
                // In review child threads, suppress assistant text deltas; the
                // UI will show a selection popup from the final ReviewOutput.
                if let Some(active) = active_item.as_ref() {
                    let item_id = active.id();
                    if matches!(active, TurnItem::AgentMessage(_)) {
                        let parsed = assistant_message_stream_parsers.parse_delta(&item_id, &delta);
                        emit_streamed_assistant_text_delta(
                            &sess,
                            &turn_context,
                            plan_mode_state.as_mut(),
                            &item_id,
                            parsed,
                        )
                        .await;
                    } else {
                        let event = AgentMessageContentDeltaEvent {
                            thread_id: sess.conversation_id.to_string(),
                            turn_id: turn_context.sub_id.clone(),
                            item_id,
                            delta,
                        };
                        sess.send_event(&turn_context, EventMsg::AgentMessageContentDelta(event))
                            .await;
                    }
                } else {
                    error_or_panic("OutputTextDelta without active item".to_string());
                }
            }
            ResponseEvent::ReasoningSummaryDelta {
                delta,
                summary_index,
            } => {
                if let Some(active) = active_item.as_ref() {
                    let event = ReasoningContentDeltaEvent {
                        thread_id: sess.conversation_id.to_string(),
                        turn_id: turn_context.sub_id.clone(),
                        item_id: active.id(),
                        delta,
                        summary_index,
                    };
                    sess.send_event(&turn_context, EventMsg::ReasoningContentDelta(event))
                        .await;
                } else {
                    error_or_panic("ReasoningSummaryDelta without active item".to_string());
                }
            }
            ResponseEvent::ReasoningSummaryPartAdded { summary_index } => {
                if let Some(active) = active_item.as_ref() {
                    let event =
                        EventMsg::AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent {
                            item_id: active.id(),
                            summary_index,
                        });
                    sess.send_event(&turn_context, event).await;
                } else {
                    error_or_panic("ReasoningSummaryPartAdded without active item".to_string());
                }
            }
            ResponseEvent::ReasoningContentDelta {
                delta,
                content_index,
            } => {
                if let Some(active) = active_item.as_ref() {
                    let event = ReasoningRawContentDeltaEvent {
                        thread_id: sess.conversation_id.to_string(),
                        turn_id: turn_context.sub_id.clone(),
                        item_id: active.id(),
                        delta,
                        content_index,
                    };
                    sess.send_event(&turn_context, EventMsg::ReasoningRawContentDelta(event))
                        .await;
                } else {
                    error_or_panic("ReasoningRawContentDelta without active item".to_string());
                }
            }
        }
    };

    flush_assistant_text_segments_all(
        &sess,
        &turn_context,
        plan_mode_state.as_mut(),
        &mut assistant_message_stream_parsers,
    )
    .await;

    drain_in_flight(&mut in_flight, sess.clone(), turn_context.clone()).await?;

    if should_emit_turn_diff {
        let unified_diff = {
            let mut tracker = turn_diff_tracker.lock().await;
            tracker.get_unified_diff()
        };
        if let Ok(Some(unified_diff)) = unified_diff {
            let msg = EventMsg::TurnDiff(TurnDiffEvent { unified_diff });
            sess.clone().send_event(&turn_context, msg).await;
        }
    }

    outcome
}

pub(super) fn get_last_assistant_message_from_turn(responses: &[ResponseItem]) -> Option<String> {
    for item in responses.iter().rev() {
        if let Some(message) = last_assistant_message_from_item(item, false) {
            return Some(message);
        }
    }
    None
}

/// When no persisted reference images are set, derive them from the
/// conversation history so image-capable models still receive context.
///
/// Priority:
/// 1. Explicit images attached to the last user message in this turn.
/// 2. The most recent image from an assistant message (e.g. a generated image).
/// 3. Empty — no images to attach.
fn derive_reference_images_for_turn(input: &[ResponseItem]) -> Vec<String> {
    // Prefer explicit images attached to the last user message in this turn.
    let last_user_index = input
        .iter()
        .rposition(|item| matches!(item, ResponseItem::Message { role, .. } if role == "user"));

    if let Some(index) = last_user_index
        && let ResponseItem::Message { content, .. } = &input[index]
    {
        let mut urls: Vec<String> = Vec::new();
        for entry in content {
            if let ContentItem::InputImage { image_url } = entry
                && !image_url.trim().is_empty()
            {
                urls.push(image_url.clone());
            }
        }
        if !urls.is_empty() {
            return urls;
        }
    }

    // Otherwise, fall back to the last assistant message that carried an image.
    for item in input.iter().rev() {
        if let ResponseItem::Message { role, content, .. } = item {
            if role != "assistant" {
                continue;
            }
            for entry in content.iter().rev() {
                if let ContentItem::InputImage { image_url } = entry
                    && !image_url.trim().is_empty()
                {
                    return vec![image_url.clone()];
                }
            }
        }
    }

    Vec::new()
}

use crate::memories::prompts::build_memory_tool_developer_instructions;
#[cfg(test)]
pub(crate) use tests::make_session_and_context;
#[cfg(test)]
pub(crate) use tests::make_session_and_context_with_dynamic_tools_and_rx;
#[cfg(test)]
pub(crate) use tests::make_session_and_context_with_rx;
#[cfg(test)]
pub(crate) use tests::make_session_configuration_for_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CodexAuth;
    use crate::config::ConfigBuilder;
    use crate::config::test_config;
    use crate::config_loader::ConfigLayerStack;
    use crate::config_loader::ConfigLayerStackOrdering;
    use crate::config_loader::NetworkConstraints;
    use crate::config_loader::RequirementSource;
    use crate::config_loader::Sourced;
    use crate::exec::ExecToolCallOutput;
    use crate::function_tool::FunctionCallError;
    use crate::mcp_connection_manager::ToolInfo;
    use crate::models_manager::model_info;
    use crate::shell::default_user_shell;
    use crate::tools::format_exec_output_str;

    use codex_protocol::ThreadId;
    use codex_protocol::models::FunctionCallOutputBody;
    use codex_protocol::models::FunctionCallOutputPayload;

    use crate::protocol::CompactedItem;
    use crate::protocol::CreditsSnapshot;
    use crate::protocol::InitialHistory;
    use crate::protocol::NetworkApprovalProtocol;
    use crate::protocol::RateLimitSnapshot;
    use crate::protocol::RateLimitWindow;
    use crate::protocol::ResumedHistory;
    use crate::protocol::TokenCountEvent;
    use crate::protocol::TokenUsage;
    use crate::protocol::TokenUsageInfo;
    use crate::state::TaskKind;
    use crate::tasks::SessionTask;
    use crate::tasks::SessionTaskContext;
    use crate::tools::ToolRouter;
    use crate::tools::context::ToolInvocation;
    use crate::tools::context::ToolOutput;
    use crate::tools::context::ToolPayload;
    use crate::tools::handlers::ShellHandler;
    use crate::tools::handlers::UnifiedExecHandler;
    use crate::tools::registry::ToolHandler;
    use crate::tools::router::ToolCallSource;
    use crate::turn_diff_tracker::TurnDiffTracker;
    use codex_app_server_protocol::AppInfo;
    use codex_otel::TelemetryAuthMode;
    use codex_protocol::models::BaseInstructions;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseInputItem;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::openai_models::InputModality;
    use codex_protocol::openai_models::ModelsResponse;
    use std::path::Path;
    use std::time::Duration;
    use tokio::time::sleep;

    use codex_protocol::mcp::CallToolResult as McpCallToolResult;
    use pretty_assertions::assert_eq;
    use rmcp::model::JsonObject;
    use rmcp::model::Tool;
    use serde::Deserialize;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;

    struct InstructionsTestCase {
        slug: &'static str,
        expects_apply_patch_instructions: bool,
    }

    fn user_message(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }
    }

    #[test]
    fn truncate_user_instructions_for_small_context_window() {
        let input = "a".repeat(SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES + 128);
        let expected = format!(
            "{}{}",
            "a".repeat(SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES),
            USER_INSTRUCTIONS_TRUNCATION_NOTICE
        );
        assert_eq!(
            truncate_user_instructions_for_context(&input, Some(8_192)),
            expected
        );
    }

    #[test]
    fn truncate_user_instructions_preserves_full_text_for_large_context_window() {
        let input = "a".repeat(SMALL_CONTEXT_MAX_USER_INSTRUCTIONS_BYTES + 128);
        assert_eq!(
            truncate_user_instructions_for_context(&input, Some(32_768)),
            input
        );
    }

    fn skill_message(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }
    }

    fn make_connector(id: &str, name: &str) -> AppInfo {
        AppInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            logo_url: None,
            logo_url_dark: None,
            distribution_channel: None,
            branding: None,
            app_metadata: None,
            labels: None,
            install_url: None,
            is_accessible: true,
            is_enabled: true,
        }
    }

    fn collaboration_mode_for_model(model: &str) -> CollaborationMode {
        CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: model.to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        }
    }

    #[test]
    fn normalize_account_pool_in_config_order_dedupes_invalid_entries() {
        let mut provider = ModelProviderInfo::create_openai_provider();
        provider.base_url = Some("https://a.example/v1".to_string());
        provider.env_key = Some("KEY_A".to_string());
        provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://a.example/v1".to_string()),
                env_key: Some("KEY_A".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://b.example/v1".to_string()),
                env_key: Some("KEY_B".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://c.example/v1".to_string()),
                env_key: Some("KEY_C".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://b.example/v1".to_string()),
                env_key: Some("KEY_B".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("".to_string()),
                env_key: Some("KEY_SKIP".to_string()),
            },
        ];

        let normalized = normalize_account_pool_in_config_order("openai", &provider);
        assert_eq!(
            normalized,
            vec![
                ModelProviderAccount {
                    base_url: Some("https://a.example/v1".to_string()),
                    env_key: Some("KEY_A".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://b.example/v1".to_string()),
                    env_key: Some("KEY_B".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://c.example/v1".to_string()),
                    env_key: Some("KEY_C".to_string()),
                },
            ]
        );
    }

    #[test]
    fn account_index_label_uses_configured_account_order() {
        let mut provider = ModelProviderInfo::create_openai_provider();
        provider.base_url = Some("https://b.example/v1".to_string());
        provider.env_key = Some("KEY_B".to_string());
        provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://a.example/v1".to_string()),
                env_key: Some("KEY_A".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://b.example/v1".to_string()),
                env_key: Some("KEY_B".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://c.example/v1".to_string()),
                env_key: Some("KEY_C".to_string()),
            },
        ];

        assert_eq!(account_index_label(&provider), "key 2/3");
    }

    #[test]
    fn next_account_from_pool_wraps_without_repeating_in_turn() {
        let mut provider = ModelProviderInfo::create_openai_provider();
        provider.base_url = Some("https://a.example/v1".to_string());
        provider.env_key = Some("KEY_A".to_string());
        provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://a.example/v1".to_string()),
                env_key: Some("KEY_A".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://b.example/v1".to_string()),
                env_key: Some("KEY_B".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://c.example/v1".to_string()),
                env_key: Some("KEY_C".to_string()),
            },
        ];

        let mut attempted = std::collections::HashSet::new();
        attempted.insert(
            provider
                .current_account()
                .expect("current account should be available"),
        );

        let first = next_account_from_pool(
            "openai",
            &provider,
            provider.current_account().as_ref(),
            &mut attempted,
        )
        .expect("second account should be selected first");
        assert_eq!(first.env_key.as_deref(), Some("KEY_B"));

        let second = next_account_from_pool("openai", &provider, Some(&first), &mut attempted)
            .expect("third account should be selected next");
        assert_eq!(second.env_key.as_deref(), Some("KEY_C"));

        let third = next_account_from_pool("openai", &provider, Some(&second), &mut attempted);
        assert_eq!(third, None);
    }

    #[tokio::test]
    async fn resolve_turn_provider_from_pool_skips_cooled_accounts_in_config_order() {
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://backup.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_3".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };
        let session_configuration = SessionConfiguration {
            provider_id: "openai".to_string(),
            provider: provider.clone(),
            ..make_session_configuration_for_tests().await
        };
        let mut state = SessionState::new(session_configuration);
        let now = std::time::Instant::now();
        state.mark_pool_account_cooling(
            "openai",
            provider.account_pool[0].clone(),
            now,
            PROVIDER_POOL_COOLDOWN,
        );

        let resolved = resolve_turn_provider_from_pool(&mut state, "openai", &provider, now);
        assert_eq!(
            resolved.provider.env_key.as_deref(),
            Some("OPENAI_API_KEY_POOL_2")
        );
        assert_eq!(
            resolved.background_message,
            Some("Provider pool openai: key 1/3 cooling down; trying key 2/3".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_turn_provider_from_pool_retries_preferred_key_after_cooldown_expires() {
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };
        let session_configuration = SessionConfiguration {
            provider_id: "openai".to_string(),
            provider: provider.clone(),
            ..make_session_configuration_for_tests().await
        };
        let mut state = SessionState::new(session_configuration);
        let now = std::time::Instant::now();
        state.mark_pool_account_cooling(
            "openai",
            provider.account_pool[0].clone(),
            now,
            PROVIDER_POOL_COOLDOWN,
        );

        let resolved = resolve_turn_provider_from_pool(
            &mut state,
            "openai",
            &provider,
            now + PROVIDER_POOL_COOLDOWN + Duration::from_secs(1),
        );
        assert_eq!(
            resolved.provider.env_key.as_deref(),
            Some("OPENAI_API_KEY_POOL_1")
        );
        assert_eq!(
            resolved.background_message,
            Some("Provider pool openai: trying key 1/2".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_turn_provider_from_pool_forces_probe_when_all_accounts_are_cooling() {
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };
        let session_configuration = SessionConfiguration {
            provider_id: "openai".to_string(),
            provider: provider.clone(),
            ..make_session_configuration_for_tests().await
        };
        let mut state = SessionState::new(session_configuration);
        let now = std::time::Instant::now();
        for account in provider.account_pool.clone() {
            state.mark_pool_account_cooling("openai", account, now, PROVIDER_POOL_COOLDOWN);
        }

        let resolved = resolve_turn_provider_from_pool(&mut state, "openai", &provider, now);
        assert_eq!(
            resolved.provider.env_key.as_deref(),
            Some("OPENAI_API_KEY_POOL_1")
        );
        assert_eq!(
            resolved.background_message,
            Some(
                "Provider pool openai: all keys cooling down; forcing fresh probe from key 1/2"
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn review_thread_resolves_review_model_provider_from_pool_instead_of_parent_provider() {
        let (sess, turn_context, rx) = make_session_and_context_with_rx().await;
        let parent_turn_context = Arc::new(
            turn_context
                .with_model("claude-opus-4-6".to_string(), &sess.services.models_manager)
                .await,
        );
        assert!(parent_turn_context.provider.is_anthropic());

        let mut review_config = (*parent_turn_context.config).clone();
        let mut openai_provider = review_config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        openai_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://preferred.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://fallback.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
            },
        ];
        review_config
            .model_providers
            .insert("openai".to_string(), openai_provider.clone());
        review_config.user_configured_provider = openai_provider.clone();
        review_config.review_model = Some("gpt-5.1-codex-mini".to_string());
        let review_config = Arc::new(review_config);

        let now = std::time::Instant::now();
        {
            let mut state = sess.state.lock().await;
            state.mark_pool_account_cooling(
                "openai",
                openai_provider.account_pool[0].clone(),
                now,
                PROVIDER_POOL_COOLDOWN,
            );
        }

        spawn_review_thread(
            Arc::clone(&sess),
            Arc::clone(&review_config),
            Arc::clone(&parent_turn_context),
            "review-sub".to_string(),
            crate::review_prompts::ResolvedReviewRequest {
                target: codex_protocol::protocol::ReviewTarget::Custom {
                    instructions: "Check these changes".to_string(),
                },
                prompt: "Check these changes".to_string(),
                user_facing_hint: "Check these changes".to_string(),
            },
        )
        .await;

        let review_turn_context = tokio::time::timeout(StdDuration::from_secs(2), async {
            loop {
                if let Some(ctx) = sess.turn_context_for_sub_id("review-sub").await {
                    break ctx;
                }
                sleep(StdDuration::from_millis(10)).await;
            }
        })
        .await
        .expect("review task should become active");

        assert_eq!(
            review_turn_context.config.model.as_deref(),
            Some("gpt-5.1-codex-mini")
        );
        assert_eq!(review_turn_context.config.model_provider_id, "openai");
        assert!(review_turn_context.provider.is_openai());
        assert_eq!(
            review_turn_context.provider.env_key.as_deref(),
            Some("OPENAI_API_KEY_POOL_2")
        );
        assert_eq!(
            review_turn_context.provider.base_url.as_deref(),
            Some("https://fallback.example/v1")
        );
        assert_eq!(
            review_turn_context.config.model_provider.account_pool,
            openai_provider.account_pool
        );

        let expected_message =
            "Provider pool openai: key 1/2 cooling down; trying key 2/2".to_string();
        let mut saw_expected_background = false;
        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let event = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for review events")
                .expect("event");
            match event.msg {
                EventMsg::BackgroundEvent(ev) if ev.message == expected_message => {
                    saw_expected_background = true;
                }
                EventMsg::EnteredReviewMode(_) => break,
                _ => {}
            }
        }
        assert!(
            saw_expected_background,
            "expected review turn to surface pool failover background message"
        );

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;
    }

    #[test]
    fn server_model_warning_ignores_non_responses_prefix_only_differences() {
        assert!(
            !Session::should_warn_on_server_model_mismatch(
                crate::model_provider_info::WireApi::Anthropic,
                "antigravity/claude-sonnet-4-6",
                "claude-sonnet-4-6",
            ),
            "anthropic wire API should not trigger cyber fallback warning",
        );
        assert!(
            !Session::should_warn_on_server_model_mismatch(
                crate::model_provider_info::WireApi::Gemini,
                "antigravity/gemini-3.1-pro-preview",
                "gemini-3.1-pro-preview",
            ),
            "gemini wire API should not trigger cyber fallback warning",
        );
    }

    #[test]
    fn server_model_warning_normalizes_known_namespace_prefixes_for_responses() {
        assert!(
            !Session::should_warn_on_server_model_mismatch(
                crate::model_provider_info::WireApi::Responses,
                "openai/gpt-5.3-codex",
                "gpt-5.3-codex",
            ),
            "namespaced and bare slugs should match",
        );
        assert!(
            Session::should_warn_on_server_model_mismatch(
                crate::model_provider_info::WireApi::Responses,
                "gpt-5.3-codex",
                "gpt-5.2",
            ),
            "true fallback downgrade should still warn",
        );
    }

    #[tokio::test]
    async fn provider_switch_history_sanitizer_drops_encrypted_reasoning_items() {
        let session_configuration = make_session_configuration_for_tests().await;
        let mut state = SessionState::new(session_configuration);
        state.replace_history(
            vec![
                ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: "existing assistant output".to_string(),
                    }],
                    end_turn: None,
                    phase: None,
                    thought_signature: None,
                },
                ResponseItem::Reasoning {
                    id: "r-1".to_string(),
                    summary: Vec::new(),
                    content: None,
                    encrypted_content: Some("enc-blob-1".to_string()),
                },
                ResponseItem::Compaction {
                    encrypted_content: "enc-blob-2".to_string(),
                },
                ResponseItem::Reasoning {
                    id: "r-2".to_string(),
                    summary: Vec::new(),
                    content: None,
                    encrypted_content: None,
                },
                user_message("user turn"),
            ],
            None,
        );

        let removed = drop_provider_specific_encrypted_history_items(&mut state);
        assert_eq!(removed, 2);

        let filtered = state.clone_history().for_prompt(&[InputModality::Text]);
        assert_eq!(
            filtered,
            vec![
                ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![ContentItem::OutputText {
                        text: "existing assistant output".to_string(),
                    }],
                    end_turn: None,
                    phase: None,
                    thought_signature: None,
                },
                ResponseItem::Reasoning {
                    id: "r-2".to_string(),
                    summary: Vec::new(),
                    content: None,
                    encrypted_content: None,
                },
                user_message("user turn"),
            ]
        );
    }

    #[tokio::test]
    async fn new_default_turn_selects_first_pool_account_from_logical_provider() {
        let (session, _) = make_session_and_context().await;
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };

        {
            let mut state = session.state.lock().await;
            state.session_configuration.provider_id = "openai".to_string();
            state.session_configuration.provider = provider.clone();

            let mut config = (*state.session_configuration.original_config_do_not_use).clone();
            config.model_provider_id = "openai".to_string();
            config.model_provider = provider.clone();
            config.user_configured_provider = provider;
            state.session_configuration.original_config_do_not_use = Arc::new(config);
        }

        let turn_context = session
            .new_default_turn_with_sub_id("pool-turn".to_string())
            .await;

        assert_eq!(
            turn_context.provider.base_url.as_deref(),
            Some("https://preferred.example/v1")
        );
        assert_eq!(
            turn_context.provider.env_key.as_deref(),
            Some("OPENAI_API_KEY_POOL_1")
        );
    }

    #[tokio::test]
    async fn session_provider_resolves_first_pool_account_from_logical_provider() {
        let (session, _) = make_session_and_context().await;
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };

        {
            let mut state = session.state.lock().await;
            state.session_configuration.provider_id = "openai".to_string();
            state.session_configuration.provider = provider.clone();
        }

        let resolved = session.provider().await;

        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://preferred.example/v1")
        );
        assert_eq!(resolved.env_key.as_deref(), Some("OPENAI_API_KEY_POOL_1"));
    }

    #[tokio::test]
    async fn maybe_switch_provider_account_preserves_turn_local_provider_configuration() {
        let (session, turn_context) = make_session_and_context().await;
        let session = Arc::new(session);
        let mut anthropic_turn_context = turn_context
            .with_model(
                "claude-opus-4-6".to_string(),
                &session.services.models_manager,
            )
            .await;
        let mut config = (*anthropic_turn_context.config).clone();
        let mut anthropic_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();
        anthropic_provider.base_url = None;
        anthropic_provider.env_key = None;
        anthropic_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://preferred.anthropic.example".to_string()),
                env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://fallback.anthropic.example".to_string()),
                env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
            },
        ];
        config.model_providers.insert(
            crate::model_provider_info::ANTHROPIC_PROVIDER_ID.to_string(),
            anthropic_provider.clone(),
        );
        config.model_provider_id = crate::model_provider_info::ANTHROPIC_PROVIDER_ID.to_string();
        config.model_provider = anthropic_provider.clone();
        anthropic_turn_context.config = Arc::new(config);
        anthropic_turn_context.provider =
            anthropic_provider.with_account(&anthropic_provider.account_pool[0]);
        let anthropic_turn_context = Arc::new(anthropic_turn_context);

        let mut attempted_accounts = HashSet::from([anthropic_provider.account_pool[0].clone()]);
        let switched = maybe_switch_provider_account(
            &session,
            &anthropic_turn_context,
            &mut attempted_accounts,
            false,
            &CodexErr::UsageLimitReached(crate::error::UsageLimitReachedError {
                plan_type: None,
                resets_at: None,
                rate_limits: None,
                promo_message: None,
            }),
            0,
            0,
        )
        .await
        .expect("turn should switch to fallback account");

        assert_eq!(switched.config.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(
            switched.config.model_provider_id,
            crate::model_provider_info::ANTHROPIC_PROVIDER_ID
        );
        assert_eq!(
            switched.provider.current_account(),
            Some(anthropic_provider.account_pool[1].clone())
        );

        let now = std::time::Instant::now();
        let mut state = session.state.lock().await;
        assert!(
            state
                .pool_cooldown_until(
                    crate::model_provider_info::ANTHROPIC_PROVIDER_ID,
                    &anthropic_provider.account_pool[0],
                    now
                )
                .is_some()
        );
        assert_eq!(
            state.pool_cooldown_until(
                crate::model_provider_info::ANTHROPIC_PROVIDER_ID,
                &anthropic_provider.account_pool[0],
                now + Duration::from_secs(61)
            ),
            None
        );
        assert_eq!(
            state.pool_cooldown_until("openai", &anthropic_provider.account_pool[0], now),
            None
        );
    }

    #[tokio::test]
    async fn utility_client_and_model_for_slug_respects_session_pool_cooldown() {
        let (session, _) = make_session_and_context().await;
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };

        let config = {
            let mut state = session.state.lock().await;
            state.session_configuration.provider_id = "openai".to_string();
            state.session_configuration.provider = provider.clone();
            let now = std::time::Instant::now();
            state.mark_pool_account_cooling(
                "openai",
                provider.account_pool[0].clone(),
                now,
                PROVIDER_POOL_COOLDOWN,
            );

            let mut config = (*state.session_configuration.original_config_do_not_use).clone();
            config.model_provider_id = "openai".to_string();
            config.model_provider = provider.clone();
            config.user_configured_provider = provider.clone();
            state.session_configuration.original_config_do_not_use = Arc::new(config.clone());
            config
        };

        let (model_client, _model_info, provider_id) = session
            .utility_client_and_model_for_slug(&config, "gpt-5.1-codex-mini")
            .await
            .expect("utility provider should resolve");

        assert_eq!(provider_id, "openai");
        assert_eq!(
            model_client.provider_for_test().current_account(),
            Some(provider.account_pool[1].clone())
        );
    }

    #[tokio::test]
    async fn entire_summary_client_and_model_for_turn_respects_session_pool_cooldown() {
        let (session, mut turn_context) = make_session_and_context().await;
        let provider = ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..ModelProviderInfo::create_openai_provider()
        };

        let config = {
            let mut state = session.state.lock().await;
            state.session_configuration.provider_id = "openai".to_string();
            state.session_configuration.provider = provider.clone();
            let now = std::time::Instant::now();
            state.mark_pool_account_cooling(
                "openai",
                provider.account_pool[0].clone(),
                now,
                PROVIDER_POOL_COOLDOWN,
            );

            let mut config = (*state.session_configuration.original_config_do_not_use).clone();
            config.model_provider_id = "openai".to_string();
            config.model_provider = provider.clone();
            config.user_configured_provider = provider.clone();
            config.memories.entire_summary_model = Some("gpt-5.1-codex-mini".to_string());
            config
                .model_providers
                .insert("openai".to_string(), provider.clone());
            state.session_configuration.original_config_do_not_use = Arc::new(config.clone());
            config
        };
        turn_context.config = Arc::new(config);

        let (model_client, model_info, model_slug, background_message) = session
            .entire_summary_client_and_model_for_turn(&turn_context)
            .await;

        assert_eq!(model_slug, "gpt-5.1-codex-mini");
        assert_eq!(model_info.slug, "gpt-5.1-codex-mini");
        assert_eq!(
            model_client.provider_for_test().current_account(),
            Some(provider.account_pool[1].clone())
        );
        assert_eq!(
            background_message.as_deref(),
            Some("Provider pool openai: key 1/2 cooling down; trying key 2/2")
        );
    }

    #[tokio::test]
    async fn apply_switches_to_grok_provider_for_namespaced_grok_model() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        assert!(session_configuration.provider.is_openai());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("xai/grok-4-latest")),
                ..Default::default()
            })
            .expect("model switch to grok should be valid");

        assert!(next.provider.is_grok());
        assert_eq!(next.provider.env_key.as_deref(), Some("XAI_API_KEY"));
    }

    #[tokio::test]
    async fn apply_switches_to_gemma_provider_for_namespaced_gemma_model() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        assert!(session_configuration.provider.is_openai());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("google/gemma-3n")),
                ..Default::default()
            })
            .expect("model switch to gemma should be valid");

        assert!(next.provider.is_gemma());
    }

    #[tokio::test]
    async fn apply_switches_to_anthropic_provider_for_claude_model() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        assert!(session_configuration.provider.is_openai());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("claude-opus-4-6")),
                ..Default::default()
            })
            .expect("model switch to claude should be valid");

        assert!(next.provider.is_anthropic());
        assert_eq!(next.provider.env_key.as_deref(), Some("ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn apply_switches_to_anthropic_provider_for_namespaced_claude_model() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        assert!(session_configuration.provider.is_openai());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model(
                    "anthropic/claude-sonnet-4-6",
                )),
                ..Default::default()
            })
            .expect("model switch to namespaced claude should be valid");

        assert!(next.provider.is_anthropic());
        assert_eq!(next.provider.env_key.as_deref(), Some("ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn apply_switches_from_gemini_provider_to_grok_provider() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let (gemini, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemini-2.5-pro")),
                ..Default::default()
            })
            .expect("model switch to gemini should be valid");
        assert!(gemini.provider.is_gemini());

        let (grok, _) = gemini
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("grok-4-latest")),
                ..Default::default()
            })
            .expect("model switch from gemini to grok should be valid");

        assert!(grok.provider.is_grok());
    }

    #[tokio::test]
    async fn apply_switches_to_gemma_provider_for_gemma_model() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        assert!(session_configuration.provider.is_openai());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemma-3n")),
                ..Default::default()
            })
            .expect("model switch to gemma should be valid");

        assert!(next.provider.is_gemma());
        let expected_base_url = std::env::var("GEMMA_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "http://localhost:5001/v1beta".to_string());
        assert_eq!(
            next.provider.base_url.as_deref(),
            Some(expected_base_url.as_str())
        );
    }

    #[tokio::test]
    async fn apply_switches_from_gemini_provider_to_gemma_provider() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let (gemini, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemini-2.5-pro")),
                ..Default::default()
            })
            .expect("model switch to gemini should be valid");
        assert!(gemini.provider.is_gemini());

        let (gemma, _) = gemini
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemma-3n")),
                ..Default::default()
            })
            .expect("model switch from gemini to gemma should be valid");

        assert!(gemma.provider.is_gemma());
    }

    #[tokio::test]
    async fn apply_keeps_custom_gemini_provider_for_gemini_models() {
        let (session, _) = make_session_and_context().await;
        let mut session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let mut custom_gemini_provider = session_configuration.provider.clone();
        custom_gemini_provider.name = "Gemini Proxy".to_string();
        custom_gemini_provider.base_url = Some("https://example.com/gemini".to_string());
        custom_gemini_provider.env_key = Some("GEMINI_API_KEY".to_string());
        custom_gemini_provider.wire_api = crate::model_provider_info::WireApi::Gemini;
        custom_gemini_provider.requires_openai_auth = false;
        custom_gemini_provider.supports_websockets = false;
        session_configuration.provider = custom_gemini_provider.clone();

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemini-2.5-pro")),
                ..Default::default()
            })
            .expect("model switch to gemini should be valid");

        assert_eq!(next.provider, custom_gemini_provider);
    }

    #[tokio::test]
    async fn apply_keeps_custom_gemini_provider_for_gemma_models() {
        let (session, _) = make_session_and_context().await;
        let mut session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let mut custom_gemini_provider = session_configuration.provider.clone();
        custom_gemini_provider.name = "Gemini Proxy".to_string();
        custom_gemini_provider.base_url = Some("http://localhost:5001/v1beta".to_string());
        custom_gemini_provider.env_key = None;
        custom_gemini_provider.wire_api = crate::model_provider_info::WireApi::Gemini;
        custom_gemini_provider.requires_openai_auth = false;
        custom_gemini_provider.supports_websockets = false;
        session_configuration.provider = custom_gemini_provider.clone();

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemma-3n")),
                ..Default::default()
            })
            .expect("model switch to gemma should be valid");

        assert_eq!(next.provider, custom_gemini_provider);
    }

    #[tokio::test]
    async fn apply_keeps_custom_anthropic_provider_for_claude_models() {
        let (session, _) = make_session_and_context().await;
        let mut session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let mut custom_anthropic_provider = session_configuration.provider.clone();
        custom_anthropic_provider.name = "Anthropic Proxy".to_string();
        custom_anthropic_provider.base_url = Some("https://example.com/anthropic".to_string());
        custom_anthropic_provider.env_key = Some("ANTHROPIC_API_KEY".to_string());
        custom_anthropic_provider.wire_api = crate::model_provider_info::WireApi::Anthropic;
        custom_anthropic_provider.requires_openai_auth = false;
        custom_anthropic_provider.supports_websockets = false;
        session_configuration.provider = custom_anthropic_provider.clone();

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("claude-opus-4-6")),
                ..Default::default()
            })
            .expect("model switch to claude should be valid");

        assert_eq!(next.provider, custom_anthropic_provider);
    }

    #[tokio::test]
    async fn apply_restores_user_provider_when_switching_away_from_grok() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let expected_user_provider = session_configuration
            .original_config_do_not_use
            .user_configured_provider
            .clone();

        let (grok, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("grok-4-latest")),
                ..Default::default()
            })
            .expect("model switch to grok should be valid");
        assert!(grok.provider.is_grok());

        let (restored, _) = grok
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5-codex")),
                ..Default::default()
            })
            .expect("model switch back to default family should be valid");

        assert_eq!(restored.provider, expected_user_provider);
    }

    #[tokio::test]
    async fn apply_restores_user_provider_when_switching_away_from_gemma() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let expected_user_provider = session_configuration
            .original_config_do_not_use
            .user_configured_provider
            .clone();

        let (gemma, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gemma-3n")),
                ..Default::default()
            })
            .expect("model switch to gemma should be valid");
        assert!(gemma.provider.is_gemma());

        let (restored, _) = gemma
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5-codex")),
                ..Default::default()
            })
            .expect("model switch back to default family should be valid");

        assert_eq!(restored.provider, expected_user_provider);
    }

    #[tokio::test]
    async fn apply_restores_user_provider_when_switching_away_from_claude() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let expected_user_provider = session_configuration
            .original_config_do_not_use
            .user_configured_provider
            .clone();

        let (claude, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("claude-opus-4-6")),
                ..Default::default()
            })
            .expect("model switch to claude should be valid");
        assert!(claude.provider.is_anthropic());

        let (restored, _) = claude
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5-codex")),
                ..Default::default()
            })
            .expect("model switch back to default family should be valid");

        assert_eq!(restored.provider, expected_user_provider);
    }

    #[tokio::test]
    async fn apply_switches_to_openai_provider_for_gpt_model_when_started_on_claude() {
        let mut config = crate::config::test_config();
        config.model = Some("claude-opus-4-6".to_string());
        config.model_provider_id = "anthropic".to_string();
        config.model_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();
        config.user_configured_provider = config.model_provider.clone();
        let config = std::sync::Arc::new(config);

        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let reasoning_effort = config.model_reasoning_effort;
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };

        let session_configuration = SessionConfiguration {
            metrics_service_name: None,
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: std::sync::Arc::clone(&config),
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        };

        assert!(session_configuration.provider.is_anthropic());

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5.3-codex")),
                ..Default::default()
            })
            .expect("model switch to gpt should be valid");

        assert!(next.provider.is_openai());
        assert_eq!(next.provider_id, "openai");
    }

    #[tokio::test]
    async fn apply_switches_to_antigravity_gemini_provider_for_non_claude_antigravity_models() {
        let (session, _) = make_session_and_context().await;
        let session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let (next, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model(
                    "antigravity/gpt-oss-120b-medium",
                )),
                ..Default::default()
            })
            .expect("model switch to antigravity non-claude model should be valid");

        assert_eq!(
            next.provider_id,
            crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID
        );
        assert_eq!(
            next.provider.wire_api,
            crate::model_provider_info::WireApi::Gemini
        );
    }

    #[tokio::test]
    async fn apply_restores_user_provider_id_when_startup_model_auto_switched_provider_id() {
        let (session, _) = make_session_and_context().await;
        let mut session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let mut config = (*session_configuration.original_config_do_not_use).clone();
        let gemini_provider = config
            .model_providers
            .get(crate::model_provider_info::GEMINI_PROVIDER_ID)
            .expect("gemini provider should exist")
            .clone();
        let openai_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();

        // Simulate startup state where model family auto-switch changed
        // model_provider_id to `gemini` while preserving the user's original
        // configured provider (`openai`) for later restoration.
        config.model_provider_id = crate::model_provider_info::GEMINI_PROVIDER_ID.to_string();
        config.model_provider = gemini_provider.clone();
        config.user_configured_provider = openai_provider.clone();
        session_configuration.original_config_do_not_use = Arc::new(config);
        session_configuration.provider_id =
            crate::model_provider_info::GEMINI_PROVIDER_ID.to_string();
        session_configuration.provider = gemini_provider;

        let (restored, label) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5-codex")),
                ..Default::default()
            })
            .expect("switch back to default model family should restore user provider");

        assert_eq!(restored.provider_id, "openai");
        assert_eq!(restored.provider, openai_provider);
        let label = label.expect("provider switch label should be present");
        assert!(label.starts_with("gemini -> openai "));
    }

    #[test]
    fn resolve_provider_id_for_provider_matches_normalized_pool_provider() {
        let mut providers = HashMap::new();

        let mut codex_provider = ModelProviderInfo::create_openai_provider();
        codex_provider.name = "codex".to_string();
        codex_provider.base_url = None;
        codex_provider.env_key = None;
        codex_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://code.ppchat.vip/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://code.ppchat.vip/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
            },
        ];
        providers.insert("codex".to_string(), codex_provider.clone());
        providers.insert(
            crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID.to_string(),
            ModelProviderInfo::create_antigravity_anthropic_provider(),
        );

        let selected_codex_provider = codex_provider.with_account(&codex_provider.account_pool[1]);

        let resolved = resolve_provider_id_for_provider(
            &providers,
            &selected_codex_provider,
            crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID,
        );
        assert_eq!(resolved, "codex");
    }

    #[tokio::test]
    async fn apply_restores_custom_responses_provider_after_anthropic_auto_switch() {
        let (session, _) = make_session_and_context().await;
        let mut session_configuration = {
            let state = session.state.lock().await;
            state.session_configuration.clone()
        };

        let mut config = (*session_configuration.original_config_do_not_use).clone();

        let mut codex_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        codex_provider.name = "codex".to_string();
        codex_provider.base_url = None;
        codex_provider.env_key = None;
        codex_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://code.ppchat.vip/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://code.ppchat.vip/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
            },
        ];
        config
            .model_providers
            .insert("codex".to_string(), codex_provider.clone());

        let user_configured_provider = codex_provider;

        let mut antigravity_provider = config
            .model_providers
            .get(crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID)
            .expect("antigravity anthropic provider should exist")
            .clone();
        antigravity_provider.base_url = Some("http://localhost:8317/v1beta".to_string());
        antigravity_provider.env_key = Some("ANTIGRAVITY_API_KEY_POOL_2".to_string());
        antigravity_provider.account_pool = vec![ModelProviderAccount {
            base_url: Some("http://localhost:8317".to_string()),
            env_key: Some("ANTIGRAVITY_API_KEY_POOL_1".to_string()),
        }];

        config.model_provider_id =
            crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID.to_string();
        config.model_provider = antigravity_provider.clone();
        config.user_configured_provider = user_configured_provider;

        session_configuration.original_config_do_not_use = Arc::new(config);
        session_configuration.provider_id =
            crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID.to_string();
        session_configuration.provider = antigravity_provider;

        let (restored, _) = session_configuration
            .apply(&SessionSettingsUpdate {
                collaboration_mode: Some(collaboration_mode_for_model("gpt-5.3-codex")),
                ..Default::default()
            })
            .expect("switch back to gpt model should restore responses provider");

        assert_eq!(restored.provider_id, "codex");
        assert_eq!(
            restored.provider.wire_api,
            crate::model_provider_info::WireApi::Responses
        );
        assert_eq!(restored.provider.base_url, None);
        assert_eq!(restored.provider.env_key, None);
        assert_eq!(restored.provider.account_pool.len(), 2);
    }
    #[test]
    fn assistant_message_stream_parsers_can_be_seeded_from_output_item_added_text() {
        let mut parsers = AssistantMessageStreamParsers::new(false);
        let item_id = "msg-1";

        let seeded = parsers.seed_item_text(item_id, "hello <oai-mem-citation>doc");
        let parsed = parsers.parse_delta(item_id, "1</oai-mem-citation> world");
        let tail = parsers.finish_item(item_id);

        assert_eq!(seeded.visible_text, "hello ");
        assert_eq!(seeded.citations, Vec::<String>::new());
        assert_eq!(parsed.visible_text, " world");
        assert_eq!(parsed.citations, vec!["doc1".to_string()]);
        assert_eq!(tail.visible_text, "");
        assert_eq!(tail.citations, Vec::<String>::new());
    }

    #[test]
    fn assistant_message_stream_parsers_seed_buffered_prefix_stays_out_of_finish_tail() {
        let mut parsers = AssistantMessageStreamParsers::new(false);
        let item_id = "msg-1";

        let seeded = parsers.seed_item_text(item_id, "hello <oai-mem-");
        let parsed = parsers.parse_delta(item_id, "citation>doc</oai-mem-citation> world");
        let tail = parsers.finish_item(item_id);

        assert_eq!(seeded.visible_text, "hello ");
        assert_eq!(seeded.citations, Vec::<String>::new());
        assert_eq!(parsed.visible_text, " world");
        assert_eq!(parsed.citations, vec!["doc".to_string()]);
        assert_eq!(tail.visible_text, "");
        assert_eq!(tail.citations, Vec::<String>::new());
    }

    #[test]
    fn assistant_message_stream_parsers_seed_plan_parser_across_added_and_delta_boundaries() {
        let mut parsers = AssistantMessageStreamParsers::new(true);
        let item_id = "msg-1";

        let seeded = parsers.seed_item_text(item_id, "Intro\n<proposed");
        let parsed = parsers.parse_delta(item_id, "_plan>\n- step\n</proposed_plan>\nOutro");
        let tail = parsers.finish_item(item_id);

        assert_eq!(seeded.visible_text, "Intro\n");
        assert_eq!(
            seeded.plan_segments,
            vec![ProposedPlanSegment::Normal("Intro\n".to_string())]
        );
        assert_eq!(parsed.visible_text, "Outro");
        assert_eq!(
            parsed.plan_segments,
            vec![
                ProposedPlanSegment::ProposedPlanStart,
                ProposedPlanSegment::ProposedPlanDelta("- step\n".to_string()),
                ProposedPlanSegment::ProposedPlanEnd,
                ProposedPlanSegment::Normal("Outro".to_string()),
            ]
        );
        assert_eq!(tail.visible_text, "");
        assert!(tail.plan_segments.is_empty());
    }

    fn make_mcp_tool(
        server_name: &str,
        tool_name: &str,
        connector_id: Option<&str>,
        connector_name: Option<&str>,
    ) -> ToolInfo {
        ToolInfo {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            tool: Tool {
                name: tool_name.to_string().into(),
                title: None,
                description: Some(format!("Test tool: {tool_name}").into()),
                input_schema: Arc::new(JsonObject::default()),
                output_schema: None,
                annotations: None,
                execution: None,
                icons: None,
                meta: None,
            },
            connector_id: connector_id.map(str::to_string),
            connector_name: connector_name.map(str::to_string),
        }
    }

    fn function_call_rollout_item(name: &str, call_id: &str) -> RolloutItem {
        RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            name: name.to_string(),
            arguments: "{}".to_string(),
            call_id: call_id.to_string(),
            thought_signature: None,
        })
    }

    fn function_call_output_rollout_item(call_id: &str, output: &str) -> RolloutItem {
        RolloutItem::ResponseItem(ResponseItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: FunctionCallOutputPayload::from_text(output.to_string()),
        })
    }

    #[test]
    fn validated_network_policy_amendment_host_allows_normalized_match() {
        let amendment = NetworkPolicyAmendment {
            host: "ExAmPlE.Com.:443".to_string(),
            action: NetworkPolicyRuleAction::Allow,
        };
        let context = NetworkApprovalContext {
            host: "example.com".to_string(),
            protocol: NetworkApprovalProtocol::Https,
        };

        let host = Session::validated_network_policy_amendment_host(&amendment, &context)
            .expect("normalized hosts should match");

        assert_eq!(host, "example.com");
    }

    #[test]
    fn validated_network_policy_amendment_host_rejects_mismatch() {
        let amendment = NetworkPolicyAmendment {
            host: "evil.example.com".to_string(),
            action: NetworkPolicyRuleAction::Deny,
        };
        let context = NetworkApprovalContext {
            host: "api.example.com".to_string(),
            protocol: NetworkApprovalProtocol::Https,
        };

        let err = Session::validated_network_policy_amendment_host(&amendment, &context)
            .expect_err("mismatched hosts should be rejected");

        let message = err.to_string();
        assert!(message.contains("does not match approved host"));
    }

    #[tokio::test]
    async fn get_base_instructions_no_user_content() {
        let prompt_with_apply_patch_instructions =
            include_str!("../prompt_with_apply_patch_instructions.md");
        let models_response: ModelsResponse =
            serde_json::from_str(include_str!("../models.json")).expect("valid models.json");
        let model_info_for_slug = |slug: &str, config: &Config| {
            let model = models_response
                .models
                .iter()
                .find(|candidate| candidate.slug == slug)
                .cloned()
                .unwrap_or_else(|| panic!("model slug {slug} is missing from models.json"));
            model_info::with_config_overrides(model, config)
        };
        let test_cases = vec![
            InstructionsTestCase {
                slug: "gpt-5",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex-max",
                expects_apply_patch_instructions: false,
            },
        ];

        let (session, _turn_context) = make_session_and_context().await;
        let config = test_config();

        for test_case in test_cases {
            let model_info = model_info_for_slug(test_case.slug, &config);
            if test_case.expects_apply_patch_instructions {
                assert_eq!(
                    model_info.base_instructions.as_str(),
                    prompt_with_apply_patch_instructions
                );
            }

            {
                let mut state = session.state.lock().await;
                state.session_configuration.base_instructions =
                    model_info.base_instructions.clone();
            }

            let base_instructions = session.get_base_instructions().await;
            assert_eq!(base_instructions.text, model_info.base_instructions);
        }
    }

    #[tokio::test]
    async fn reload_user_config_layer_updates_effective_apps_config() {
        let (session, _turn_context) = make_session_and_context().await;
        let codex_home = session.codex_home().await;
        std::fs::create_dir_all(&codex_home).expect("create codex home");
        let config_toml_path = codex_home.join(CONFIG_TOML_FILE);
        std::fs::write(
            &config_toml_path,
            "[apps.calendar]\nenabled = false\ndestructive_enabled = false\n",
        )
        .expect("write user config");

        session.reload_user_config_layer().await;

        let config = session.get_config().await;
        let apps_toml = config
            .config_layer_stack
            .effective_config()
            .as_table()
            .and_then(|table| table.get("apps"))
            .cloned()
            .expect("apps table");
        let apps = crate::config::types::AppsConfigToml::deserialize(apps_toml)
            .expect("deserialize apps config");
        let app = apps
            .apps
            .get("calendar")
            .expect("calendar app config exists");

        assert!(!app.enabled);
        assert_eq!(app.destructive_enabled, Some(false));
    }

    #[tokio::test]
    async fn reload_user_config_layer_updates_model_sub_fields() {
        let (session, _turn_context) = make_session_and_context().await;
        let codex_home = session.codex_home().await;
        std::fs::create_dir_all(&codex_home).expect("create codex home");
        let config_toml_path = codex_home.join(CONFIG_TOML_FILE);
        std::fs::write(
            &config_toml_path,
            r#"
profile = "dev"

[profiles.dev]
model_sub = "claude-haiku-4-5-20251001"
model_sub_responses = "gpt-5.3-codex-spark|[pro]"
"#,
        )
        .expect("write user config");

        session.reload_user_config_layer().await;

        let config = session.get_config().await;
        assert_eq!(
            config.model_sub.as_deref(),
            Some("claude-haiku-4-5-20251001")
        );
        assert_eq!(
            config.model_sub_responses.as_deref(),
            Some("gpt-5.3-codex-spark|[pro]")
        );
    }

    #[test]
    fn filter_connectors_for_input_skips_duplicate_slug_mentions() {
        let connectors = vec![
            make_connector("one", "Foo Bar"),
            make_connector("two", "Foo-Bar"),
        ];
        let input = vec![user_message("use $foo-bar")];
        let explicitly_enabled_connectors = HashSet::new();
        let skill_name_counts_lower = HashMap::new();

        let selected = filter_connectors_for_input(
            &connectors,
            &input,
            &explicitly_enabled_connectors,
            &skill_name_counts_lower,
        );

        assert_eq!(selected, Vec::new());
    }

    #[test]
    fn filter_connectors_for_input_skips_when_skill_name_conflicts() {
        let connectors = vec![make_connector("one", "Todoist")];
        let input = vec![user_message("use $todoist")];
        let explicitly_enabled_connectors = HashSet::new();
        let skill_name_counts_lower = HashMap::from([("todoist".to_string(), 1)]);

        let selected = filter_connectors_for_input(
            &connectors,
            &input,
            &explicitly_enabled_connectors,
            &skill_name_counts_lower,
        );

        assert_eq!(selected, Vec::new());
    }

    #[test]
    fn filter_connectors_for_input_skips_disabled_connectors() {
        let mut connector = make_connector("calendar", "Calendar");
        connector.is_enabled = false;
        let input = vec![user_message("use $calendar")];
        let explicitly_enabled_connectors = HashSet::new();
        let selected = filter_connectors_for_input(
            &[connector],
            &input,
            &explicitly_enabled_connectors,
            &HashMap::new(),
        );

        assert_eq!(selected, Vec::new());
    }

    #[test]
    fn collect_explicit_app_ids_from_skill_items_includes_linked_mentions() {
        let connectors = vec![make_connector("calendar", "Calendar")];
        let skill_items = vec![skill_message(
            "<skill>\n<name>demo</name>\n<path>/tmp/skills/demo/SKILL.md</path>\nuse [$calendar](app://calendar)\n</skill>",
        )];

        let connector_ids =
            collect_explicit_app_ids_from_skill_items(&skill_items, &connectors, &HashMap::new());

        assert_eq!(connector_ids, HashSet::from(["calendar".to_string()]));
    }

    #[test]
    fn collect_explicit_app_ids_from_skill_items_resolves_unambiguous_plain_mentions() {
        let connectors = vec![make_connector("calendar", "Calendar")];
        let skill_items = vec![skill_message(
            "<skill>\n<name>demo</name>\n<path>/tmp/skills/demo/SKILL.md</path>\nuse $calendar\n</skill>",
        )];

        let connector_ids =
            collect_explicit_app_ids_from_skill_items(&skill_items, &connectors, &HashMap::new());

        assert_eq!(connector_ids, HashSet::from(["calendar".to_string()]));
    }

    #[test]
    fn collect_explicit_app_ids_from_skill_items_skips_plain_mentions_with_skill_conflicts() {
        let connectors = vec![make_connector("calendar", "Calendar")];
        let skill_items = vec![skill_message(
            "<skill>\n<name>demo</name>\n<path>/tmp/skills/demo/SKILL.md</path>\nuse $calendar\n</skill>",
        )];
        let skill_name_counts_lower = HashMap::from([("calendar".to_string(), 1)]);

        let connector_ids = collect_explicit_app_ids_from_skill_items(
            &skill_items,
            &connectors,
            &skill_name_counts_lower,
        );

        assert_eq!(connector_ids, HashSet::<String>::new());
    }

    #[test]
    fn non_app_mcp_tools_remain_visible_without_search_selection() {
        let mcp_tools = HashMap::from([
            (
                "mcp__codex_apps__calendar_create_event".to_string(),
                make_mcp_tool(
                    CODEX_APPS_MCP_SERVER_NAME,
                    "calendar_create_event",
                    Some("calendar"),
                    Some("Calendar"),
                ),
            ),
            (
                "mcp__rmcp__echo".to_string(),
                make_mcp_tool("rmcp", "echo", None, None),
            ),
        ]);

        let mut selected_mcp_tools = mcp_tools
            .iter()
            .filter(|(_, tool)| tool.server_name != CODEX_APPS_MCP_SERVER_NAME)
            .map(|(name, tool)| (name.clone(), tool.clone()))
            .collect::<HashMap<_, _>>();

        let connectors = connectors::accessible_connectors_from_mcp_tools(&mcp_tools);
        let explicitly_enabled_connectors = HashSet::new();
        let connectors = filter_connectors_for_input(
            &connectors,
            &[user_message("run echo")],
            &explicitly_enabled_connectors,
            &HashMap::new(),
        );
        let apps_mcp_tools = filter_codex_apps_mcp_tools_only(&mcp_tools, &connectors);
        selected_mcp_tools.extend(apps_mcp_tools);

        let mut tool_names: Vec<String> = selected_mcp_tools.into_keys().collect();
        tool_names.sort();
        assert_eq!(tool_names, vec!["mcp__rmcp__echo".to_string()]);
    }

    #[test]
    fn search_tool_selection_keeps_codex_apps_tools_without_mentions() {
        let selected_tool_names = vec![
            "mcp__codex_apps__calendar_create_event".to_string(),
            "mcp__rmcp__echo".to_string(),
        ];
        let mcp_tools = HashMap::from([
            (
                "mcp__codex_apps__calendar_create_event".to_string(),
                make_mcp_tool(
                    CODEX_APPS_MCP_SERVER_NAME,
                    "calendar_create_event",
                    Some("calendar"),
                    Some("Calendar"),
                ),
            ),
            (
                "mcp__rmcp__echo".to_string(),
                make_mcp_tool("rmcp", "echo", None, None),
            ),
        ]);

        let mut selected_mcp_tools = filter_mcp_tools_by_name(&mcp_tools, &selected_tool_names);
        let connectors = connectors::accessible_connectors_from_mcp_tools(&mcp_tools);
        let explicitly_enabled_connectors = HashSet::new();
        let connectors = filter_connectors_for_input(
            &connectors,
            &[user_message("run the selected tools")],
            &explicitly_enabled_connectors,
            &HashMap::new(),
        );
        let apps_mcp_tools = filter_codex_apps_mcp_tools_only(&mcp_tools, &connectors);
        selected_mcp_tools.extend(apps_mcp_tools);

        let mut tool_names: Vec<String> = selected_mcp_tools.into_keys().collect();
        tool_names.sort();
        assert_eq!(
            tool_names,
            vec![
                "mcp__codex_apps__calendar_create_event".to_string(),
                "mcp__rmcp__echo".to_string(),
            ]
        );
    }

    #[test]
    fn apps_mentions_add_codex_apps_tools_to_search_selected_set() {
        let selected_tool_names = vec!["mcp__rmcp__echo".to_string()];
        let mcp_tools = HashMap::from([
            (
                "mcp__codex_apps__calendar_create_event".to_string(),
                make_mcp_tool(
                    CODEX_APPS_MCP_SERVER_NAME,
                    "calendar_create_event",
                    Some("calendar"),
                    Some("Calendar"),
                ),
            ),
            (
                "mcp__rmcp__echo".to_string(),
                make_mcp_tool("rmcp", "echo", None, None),
            ),
        ]);

        let mut selected_mcp_tools = filter_mcp_tools_by_name(&mcp_tools, &selected_tool_names);
        let connectors = connectors::accessible_connectors_from_mcp_tools(&mcp_tools);
        let explicitly_enabled_connectors = HashSet::new();
        let connectors = filter_connectors_for_input(
            &connectors,
            &[user_message("use $calendar and then echo the response")],
            &explicitly_enabled_connectors,
            &HashMap::new(),
        );
        let apps_mcp_tools = filter_codex_apps_mcp_tools_only(&mcp_tools, &connectors);
        selected_mcp_tools.extend(apps_mcp_tools);

        let mut tool_names: Vec<String> = selected_mcp_tools.into_keys().collect();
        tool_names.sort();
        assert_eq!(
            tool_names,
            vec![
                "mcp__codex_apps__calendar_create_event".to_string(),
                "mcp__rmcp__echo".to_string(),
            ]
        );
    }

    #[test]
    fn extract_mcp_tool_selection_from_rollout_reads_search_tool_output() {
        let rollout_items = vec![
            function_call_rollout_item(SEARCH_TOOL_BM25_TOOL_NAME, "search-1"),
            function_call_output_rollout_item(
                "search-1",
                &json!({
                    "active_selected_tools": [
                        "mcp__codex_apps__calendar_create_event",
                        "mcp__codex_apps__calendar_list_events",
                    ],
                })
                .to_string(),
            ),
        ];

        let selected = Session::extract_mcp_tool_selection_from_rollout(&rollout_items);
        assert_eq!(
            selected,
            Some(vec![
                "mcp__codex_apps__calendar_create_event".to_string(),
                "mcp__codex_apps__calendar_list_events".to_string(),
            ])
        );
    }

    #[test]
    fn extract_mcp_tool_selection_from_rollout_latest_valid_payload_wins() {
        let rollout_items = vec![
            function_call_rollout_item(SEARCH_TOOL_BM25_TOOL_NAME, "search-1"),
            function_call_output_rollout_item(
                "search-1",
                &json!({
                    "active_selected_tools": ["mcp__codex_apps__calendar_create_event"],
                })
                .to_string(),
            ),
            function_call_rollout_item(SEARCH_TOOL_BM25_TOOL_NAME, "search-2"),
            function_call_output_rollout_item(
                "search-2",
                &json!({
                    "active_selected_tools": ["mcp__codex_apps__calendar_delete_event"],
                })
                .to_string(),
            ),
        ];

        let selected = Session::extract_mcp_tool_selection_from_rollout(&rollout_items);
        assert_eq!(
            selected,
            Some(vec!["mcp__codex_apps__calendar_delete_event".to_string(),])
        );
    }

    #[test]
    fn extract_mcp_tool_selection_from_rollout_ignores_non_search_and_malformed_payloads() {
        let rollout_items = vec![
            function_call_rollout_item("shell", "shell-1"),
            function_call_output_rollout_item(
                "shell-1",
                &json!({
                    "active_selected_tools": ["mcp__codex_apps__should_be_ignored"],
                })
                .to_string(),
            ),
            function_call_rollout_item(SEARCH_TOOL_BM25_TOOL_NAME, "search-1"),
            function_call_output_rollout_item("search-1", "{not-json"),
            function_call_output_rollout_item(
                "unknown-search-call",
                &json!({
                    "active_selected_tools": ["mcp__codex_apps__also_ignored"],
                })
                .to_string(),
            ),
            function_call_output_rollout_item(
                "search-1",
                &json!({
                    "active_selected_tools": ["mcp__codex_apps__calendar_list_events"],
                })
                .to_string(),
            ),
        ];

        let selected = Session::extract_mcp_tool_selection_from_rollout(&rollout_items);
        assert_eq!(
            selected,
            Some(vec!["mcp__codex_apps__calendar_list_events".to_string(),])
        );
    }

    #[test]
    fn extract_mcp_tool_selection_from_rollout_returns_none_without_valid_search_output() {
        let rollout_items = vec![function_call_rollout_item(
            SEARCH_TOOL_BM25_TOOL_NAME,
            "search-1",
        )];
        let selected = Session::extract_mcp_tool_selection_from_rollout(&rollout_items);
        assert_eq!(selected, None);
    }

    #[tokio::test]
    async fn reconstruct_history_matches_live_compactions() {
        let (session, turn_context) = make_session_and_context().await;
        let (rollout_items, expected) = sample_rollout(&session, &turn_context).await;

        let reconstruction_turn = session.new_default_turn().await;
        let reconstructed = session
            .reconstruct_history_from_rollout(reconstruction_turn.as_ref(), &rollout_items)
            .await;

        assert_eq!(expected, reconstructed);
    }

    #[tokio::test]
    async fn reconstruct_history_uses_replacement_history_verbatim() {
        let (session, turn_context) = make_session_and_context().await;
        let summary_item = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "summary".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        let replacement_history = vec![
            summary_item.clone(),
            ResponseItem::Message {
                id: None,
                role: "developer".to_string(),
                content: vec![ContentItem::InputText {
                    text: "stale developer instructions".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];
        let rollout_items = vec![RolloutItem::Compacted(CompactedItem {
            message: String::new(),
            replacement_history: Some(replacement_history.clone()),
        })];

        let reconstructed = session
            .reconstruct_history_from_rollout(&turn_context, &rollout_items)
            .await;

        assert_eq!(reconstructed, replacement_history);
    }

    #[tokio::test]
    async fn record_initial_history_reconstructs_resumed_transcript() {
        let (session, turn_context) = make_session_and_context().await;
        let (rollout_items, expected) = sample_rollout(&session, &turn_context).await;

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        let history = session.state.lock().await.clone_history();
        assert_eq!(expected, history.raw_items());
    }

    #[tokio::test]
    async fn record_initial_history_resumed_hydrates_previous_model() {
        let (session, turn_context) = make_session_and_context().await;
        let previous_model = "previous-rollout-model";
        let previous_context_item = TurnContextItem {
            turn_id: Some(turn_context.sub_id.clone()),
            cwd: turn_context.cwd.clone(),
            approval_policy: turn_context.approval_policy.value(),
            sandbox_policy: turn_context.sandbox_policy.get().clone(),
            network: None,
            model: previous_model.to_string(),
            personality: turn_context.personality,
            collaboration_mode: Some(turn_context.collaboration_mode.clone()),
            effort: turn_context.reasoning_effort,
            summary: turn_context.reasoning_summary,
            user_instructions: None,
            developer_instructions: None,
            final_output_json_schema: None,
            truncation_policy: Some(turn_context.truncation_policy.into()),
        };
        let rollout_items = vec![RolloutItem::TurnContext(previous_context_item)];

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        assert_eq!(
            session.previous_model().await,
            Some(previous_model.to_string())
        );
    }

    #[tokio::test]
    async fn record_initial_history_resumed_hydrates_previous_model_from_lifecycle_turn_with_missing_turn_context_id()
     {
        let (session, turn_context) = make_session_and_context().await;
        let previous_model = "previous-rollout-model";
        let mut previous_context_item = TurnContextItem {
            turn_id: Some(turn_context.sub_id.clone()),
            cwd: turn_context.cwd.clone(),
            approval_policy: turn_context.approval_policy.value(),
            sandbox_policy: turn_context.sandbox_policy.get().clone(),
            network: None,
            model: previous_model.to_string(),
            personality: turn_context.personality,
            collaboration_mode: Some(turn_context.collaboration_mode.clone()),
            effort: turn_context.reasoning_effort,
            summary: turn_context.reasoning_summary,
            user_instructions: None,
            developer_instructions: None,
            final_output_json_schema: None,
            truncation_policy: Some(turn_context.truncation_policy.into()),
        };
        let turn_id = previous_context_item
            .turn_id
            .clone()
            .expect("turn context should have turn_id");
        previous_context_item.turn_id = None;

        let rollout_items = vec![
            RolloutItem::EventMsg(EventMsg::TurnStarted(
                codex_protocol::protocol::TurnStartedEvent {
                    turn_id: turn_id.clone(),
                    model_context_window: Some(128_000),
                    collaboration_mode_kind: ModeKind::Default,
                    memory: None,
                },
            )),
            RolloutItem::EventMsg(EventMsg::UserMessage(
                codex_protocol::protocol::UserMessageEvent {
                    message: "seed".to_string(),
                    images: None,
                    local_images: Vec::new(),
                    text_elements: Vec::new(),
                },
            )),
            RolloutItem::TurnContext(previous_context_item),
            RolloutItem::EventMsg(EventMsg::TurnComplete(
                codex_protocol::protocol::TurnCompleteEvent {
                    turn_id,
                    last_agent_message: None,
                    memory: None,
                },
            )),
        ];

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        assert_eq!(
            session.previous_model().await,
            Some(previous_model.to_string())
        );
    }

    #[tokio::test]
    async fn record_initial_history_resumed_rollback_skips_only_user_turns() {
        let (session, turn_context) = make_session_and_context().await;
        let previous_context_item = turn_context.to_turn_context_item();
        let user_turn_id = previous_context_item
            .turn_id
            .clone()
            .expect("turn context should have turn_id");
        let standalone_turn_id = "standalone-task-turn".to_string();
        let rollout_items = vec![
            RolloutItem::EventMsg(EventMsg::TurnStarted(
                codex_protocol::protocol::TurnStartedEvent {
                    turn_id: user_turn_id.clone(),
                    model_context_window: Some(128_000),
                    collaboration_mode_kind: ModeKind::Default,
                    memory: None,
                },
            )),
            RolloutItem::EventMsg(EventMsg::UserMessage(
                codex_protocol::protocol::UserMessageEvent {
                    message: "seed".to_string(),
                    images: None,
                    local_images: Vec::new(),
                    text_elements: Vec::new(),
                },
            )),
            RolloutItem::TurnContext(previous_context_item),
            RolloutItem::EventMsg(EventMsg::TurnComplete(
                codex_protocol::protocol::TurnCompleteEvent {
                    turn_id: user_turn_id,
                    last_agent_message: None,
                    memory: None,
                },
            )),
            // Standalone task turn (no UserMessage) should not consume rollback skips.
            RolloutItem::EventMsg(EventMsg::TurnStarted(
                codex_protocol::protocol::TurnStartedEvent {
                    turn_id: standalone_turn_id.clone(),
                    model_context_window: Some(128_000),
                    collaboration_mode_kind: ModeKind::Default,
                    memory: None,
                },
            )),
            RolloutItem::EventMsg(EventMsg::TurnComplete(
                codex_protocol::protocol::TurnCompleteEvent {
                    turn_id: standalone_turn_id,
                    last_agent_message: None,
                    memory: None,
                },
            )),
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(
                codex_protocol::protocol::ThreadRolledBackEvent { num_turns: 1 },
            )),
        ];

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        assert_eq!(session.previous_model().await, None);
        assert!(session.reference_context_item().await.is_none());
    }

    #[tokio::test]
    async fn record_initial_history_resumed_seeds_reference_context_item_without_compaction() {
        let (session, turn_context) = make_session_and_context().await;
        let previous_context_item = turn_context.to_turn_context_item();
        let rollout_items = vec![RolloutItem::TurnContext(previous_context_item.clone())];

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        assert_eq!(
            serde_json::to_value(session.reference_context_item().await)
                .expect("serialize seeded reference context item"),
            serde_json::to_value(Some(previous_context_item))
                .expect("serialize expected reference context item")
        );
    }

    #[tokio::test]
    async fn record_initial_history_resumed_does_not_seed_reference_context_item_after_compaction()
    {
        let (session, turn_context) = make_session_and_context().await;
        let previous_context_item = turn_context.to_turn_context_item();
        let rollout_items = vec![
            RolloutItem::TurnContext(previous_context_item),
            RolloutItem::Compacted(CompactedItem {
                message: String::new(),
                replacement_history: Some(Vec::new()),
            }),
        ];

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        assert!(session.reference_context_item().await.is_none());
    }

    #[tokio::test]
    async fn resumed_history_injects_initial_context_on_first_context_update_only() {
        let (session, turn_context) = make_session_and_context().await;
        let (rollout_items, mut expected) = sample_rollout(&session, &turn_context).await;

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        let history_before_seed = session.state.lock().await.clone_history();
        assert_eq!(expected, history_before_seed.raw_items());

        session
            .record_context_updates_and_set_reference_context_item(&turn_context, None)
            .await;
        expected.extend(session.build_initial_context(&turn_context, None).await);
        let history_after_seed = session.clone_history().await;
        assert_eq!(expected, history_after_seed.raw_items());

        session
            .record_context_updates_and_set_reference_context_item(&turn_context, None)
            .await;
        let history_after_second_seed = session.clone_history().await;
        assert_eq!(expected, history_after_second_seed.raw_items());
    }

    #[tokio::test]
    async fn record_initial_history_seeds_token_info_from_rollout() {
        let (session, turn_context) = make_session_and_context().await;
        let (mut rollout_items, _expected) = sample_rollout(&session, &turn_context).await;

        let info1 = TokenUsageInfo {
            total_token_usage: TokenUsage {
                input_tokens: 10,
                cached_input_tokens: 0,
                output_tokens: 20,
                reasoning_output_tokens: 0,
                total_tokens: 30,
            },
            last_token_usage: TokenUsage {
                input_tokens: 3,
                cached_input_tokens: 0,
                output_tokens: 4,
                reasoning_output_tokens: 0,
                total_tokens: 7,
            },
            model_context_window: Some(1_000),
        };
        let info2 = TokenUsageInfo {
            total_token_usage: TokenUsage {
                input_tokens: 100,
                cached_input_tokens: 50,
                output_tokens: 200,
                reasoning_output_tokens: 25,
                total_tokens: 375,
            },
            last_token_usage: TokenUsage {
                input_tokens: 10,
                cached_input_tokens: 0,
                output_tokens: 20,
                reasoning_output_tokens: 5,
                total_tokens: 35,
            },
            model_context_window: Some(2_000),
        };

        rollout_items.push(RolloutItem::EventMsg(EventMsg::TokenCount(
            TokenCountEvent {
                info: Some(info1),
                rate_limits: None,
            },
        )));
        rollout_items.push(RolloutItem::EventMsg(EventMsg::TokenCount(
            TokenCountEvent {
                info: None,
                rate_limits: None,
            },
        )));
        rollout_items.push(RolloutItem::EventMsg(EventMsg::TokenCount(
            TokenCountEvent {
                info: Some(info2.clone()),
                rate_limits: None,
            },
        )));
        rollout_items.push(RolloutItem::EventMsg(EventMsg::TokenCount(
            TokenCountEvent {
                info: None,
                rate_limits: None,
            },
        )));

        session
            .record_initial_history(InitialHistory::Resumed(ResumedHistory {
                conversation_id: ThreadId::default(),
                history: rollout_items,
                rollout_path: PathBuf::from("/tmp/resume.jsonl"),
            }))
            .await;

        let actual = session.state.lock().await.token_info();
        assert_eq!(actual, Some(info2));
    }

    #[tokio::test]
    async fn recompute_token_usage_uses_session_base_instructions() {
        let (session, turn_context) = make_session_and_context().await;

        let override_instructions = "SESSION_OVERRIDE_INSTRUCTIONS_ONLY".repeat(120);
        {
            let mut state = session.state.lock().await;
            state.session_configuration.base_instructions = override_instructions.clone();
        }

        let item = user_message("hello");
        session
            .record_into_history(std::slice::from_ref(&item), &turn_context)
            .await;

        let history = session.clone_history().await;
        let session_base_instructions = BaseInstructions {
            text: override_instructions,
        };
        let expected_tokens = history
            .estimate_token_count_with_base_instructions(&session_base_instructions)
            .expect("estimate with session base instructions");
        let model_estimated_tokens = history
            .estimate_token_count(&turn_context)
            .expect("estimate with model instructions");
        assert_ne!(expected_tokens, model_estimated_tokens);

        session.recompute_token_usage(&turn_context).await;

        let actual_tokens = session
            .state
            .lock()
            .await
            .token_info()
            .expect("token info")
            .last_token_usage
            .total_tokens;
        assert_eq!(actual_tokens, expected_tokens.max(0));
    }

    #[tokio::test]
    async fn recompute_token_usage_updates_model_context_window() {
        let (session, mut turn_context) = make_session_and_context().await;

        {
            let mut state = session.state.lock().await;
            state.set_token_info(Some(TokenUsageInfo {
                total_token_usage: TokenUsage::default(),
                last_token_usage: TokenUsage::default(),
                model_context_window: Some(258_400),
            }));
        }

        turn_context.model_info.context_window = Some(128_000);
        turn_context.model_info.effective_context_window_percent = 100;

        session.recompute_token_usage(&turn_context).await;

        let actual = session.state.lock().await.token_info().expect("token info");
        assert_eq!(actual.model_context_window, Some(128_000));
    }

    #[tokio::test]
    async fn record_initial_history_reconstructs_forked_transcript() {
        let (session, turn_context) = make_session_and_context().await;
        let (rollout_items, mut expected) = sample_rollout(&session, &turn_context).await;

        session
            .record_initial_history(InitialHistory::Forked(rollout_items))
            .await;

        let reconstruction_turn = session.new_default_turn().await;
        expected.extend(
            session
                .build_initial_context(reconstruction_turn.as_ref(), None)
                .await,
        );
        let history = session.state.lock().await.clone_history();
        assert_eq!(expected, history.raw_items());
    }

    #[tokio::test]
    async fn record_initial_history_forked_hydrates_previous_model() {
        let (session, turn_context) = make_session_and_context().await;
        let previous_model = "forked-rollout-model";
        let previous_context_item = TurnContextItem {
            turn_id: Some(turn_context.sub_id.clone()),
            cwd: turn_context.cwd.clone(),
            approval_policy: turn_context.approval_policy.value(),
            sandbox_policy: turn_context.sandbox_policy.get().clone(),
            network: None,
            model: previous_model.to_string(),
            personality: turn_context.personality,
            collaboration_mode: Some(turn_context.collaboration_mode.clone()),
            effort: turn_context.reasoning_effort,
            summary: turn_context.reasoning_summary,
            user_instructions: None,
            developer_instructions: None,
            final_output_json_schema: None,
            truncation_policy: Some(turn_context.truncation_policy.into()),
        };
        let rollout_items = vec![RolloutItem::TurnContext(previous_context_item)];

        session
            .record_initial_history(InitialHistory::Forked(rollout_items))
            .await;

        assert_eq!(
            session.previous_model().await,
            Some(previous_model.to_string())
        );
    }

    #[tokio::test]
    async fn thread_rollback_drops_last_turn_from_history() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;

        let initial_context = sess.build_initial_context(tc.as_ref(), None).await;
        sess.record_into_history(&initial_context, tc.as_ref())
            .await;

        let turn_1 = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "turn 1 user".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "turn 1 assistant".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];
        sess.record_into_history(&turn_1, tc.as_ref()).await;

        let turn_2 = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "turn 2 user".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "turn 2 assistant".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];
        sess.record_into_history(&turn_2, tc.as_ref()).await;
        sess.set_previous_model(Some("previous-regular-model".to_string()))
            .await;

        handlers::thread_rollback(&sess, "sub-1".to_string(), 1).await;

        let rollback_event = wait_for_thread_rolled_back(&rx).await;
        assert_eq!(rollback_event.num_turns, 1);

        let mut expected = Vec::new();
        expected.extend(initial_context);
        expected.extend(turn_1);

        let history = sess.clone_history().await;
        assert_eq!(expected, history.raw_items());
        assert_eq!(
            sess.previous_model().await,
            Some("previous-regular-model".to_string())
        );
    }

    #[tokio::test]
    async fn thread_rollback_clears_history_when_num_turns_exceeds_existing_turns() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;

        let initial_context = sess.build_initial_context(tc.as_ref(), None).await;
        sess.record_into_history(&initial_context, tc.as_ref())
            .await;

        let turn_1 = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "turn 1 user".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }];
        sess.record_into_history(&turn_1, tc.as_ref()).await;

        handlers::thread_rollback(&sess, "sub-1".to_string(), 99).await;

        let rollback_event = wait_for_thread_rolled_back(&rx).await;
        assert_eq!(rollback_event.num_turns, 99);

        let history = sess.clone_history().await;
        assert_eq!(initial_context, history.raw_items());
    }

    #[tokio::test]
    async fn thread_rollback_fails_when_turn_in_progress() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;

        let initial_context = sess.build_initial_context(tc.as_ref(), None).await;
        sess.record_into_history(&initial_context, tc.as_ref())
            .await;

        *sess.active_turn.lock().await = Some(crate::state::ActiveTurn::default());
        handlers::thread_rollback(&sess, "sub-1".to_string(), 1).await;

        let error_event = wait_for_thread_rollback_failed(&rx).await;
        assert_eq!(
            error_event.codex_error_info,
            Some(CodexErrorInfo::ThreadRollbackFailed)
        );

        let history = sess.clone_history().await;
        assert_eq!(initial_context, history.raw_items());
    }

    #[tokio::test]
    async fn thread_rollback_fails_when_num_turns_is_zero() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;

        let initial_context = sess.build_initial_context(tc.as_ref(), None).await;
        sess.record_into_history(&initial_context, tc.as_ref())
            .await;

        handlers::thread_rollback(&sess, "sub-1".to_string(), 0).await;

        let error_event = wait_for_thread_rollback_failed(&rx).await;
        assert_eq!(error_event.message, "num_turns must be >= 1");
        assert_eq!(
            error_event.codex_error_info,
            Some(CodexErrorInfo::ThreadRollbackFailed)
        );

        let history = sess.clone_history().await;
        assert_eq!(initial_context, history.raw_items());
    }

    #[tokio::test]
    async fn set_rate_limits_retains_previous_credits() {
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let config = build_test_config(codex_home.path()).await;
        let config = Arc::new(config);
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let reasoning_effort = config.model_reasoning_effort;
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        };

        let mut state = SessionState::new(session_configuration);
        let initial = RateLimitSnapshot {
            limit_id: None,
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 10.0,
                window_minutes: Some(15),
                resets_at: Some(1_700),
            }),
            secondary: None,
            credits: Some(CreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some("10.00".to_string()),
            }),
            plan_type: Some(codex_protocol::account::PlanType::Plus),
        };
        state.set_rate_limits(initial.clone());

        let update = RateLimitSnapshot {
            limit_id: Some("codex_other".to_string()),
            limit_name: Some("codex_other".to_string()),
            primary: Some(RateLimitWindow {
                used_percent: 40.0,
                window_minutes: Some(30),
                resets_at: Some(1_800),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 5.0,
                window_minutes: Some(60),
                resets_at: Some(1_900),
            }),
            credits: None,
            plan_type: None,
        };
        state.set_rate_limits(update.clone());

        assert_eq!(
            state.latest_rate_limits,
            Some(RateLimitSnapshot {
                limit_id: Some("codex_other".to_string()),
                limit_name: Some("codex_other".to_string()),
                primary: update.primary.clone(),
                secondary: update.secondary,
                credits: initial.credits,
                plan_type: initial.plan_type,
            })
        );
    }

    #[tokio::test]
    async fn set_rate_limits_updates_plan_type_when_present() {
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let config = build_test_config(codex_home.path()).await;
        let config = Arc::new(config);
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let reasoning_effort = config.model_reasoning_effort;
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        };

        let mut state = SessionState::new(session_configuration);
        let initial = RateLimitSnapshot {
            limit_id: None,
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 15.0,
                window_minutes: Some(20),
                resets_at: Some(1_600),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 5.0,
                window_minutes: Some(45),
                resets_at: Some(1_650),
            }),
            credits: Some(CreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some("15.00".to_string()),
            }),
            plan_type: Some(codex_protocol::account::PlanType::Plus),
        };
        state.set_rate_limits(initial.clone());

        let update = RateLimitSnapshot {
            limit_id: None,
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 35.0,
                window_minutes: Some(25),
                resets_at: Some(1_700),
            }),
            secondary: None,
            credits: None,
            plan_type: Some(codex_protocol::account::PlanType::Pro),
        };
        state.set_rate_limits(update.clone());

        assert_eq!(
            state.latest_rate_limits,
            Some(RateLimitSnapshot {
                limit_id: Some("codex".to_string()),
                limit_name: None,
                primary: update.primary,
                secondary: update.secondary,
                credits: initial.credits,
                plan_type: update.plan_type,
            })
        );
    }

    #[test]
    fn prefers_structured_content_when_present() {
        let ctr = McpCallToolResult {
            // Content present but should be ignored because structured_content is set.
            content: vec![text_block("ignored")],
            is_error: None,
            structured_content: Some(json!({
                "ok": true,
                "value": 42
            })),
            meta: None,
        };

        let got = FunctionCallOutputPayload::from(&ctr);
        let expected = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text(
                serde_json::to_string(&json!({
                    "ok": true,
                    "value": 42
                }))
                .unwrap(),
            ),
            success: Some(true),
        };

        assert_eq!(expected, got);
    }

    #[tokio::test]
    async fn includes_timed_out_message() {
        let exec = ExecToolCallOutput {
            exit_code: 0,
            stdout: StreamOutput::new(String::new()),
            stderr: StreamOutput::new(String::new()),
            aggregated_output: StreamOutput::new("Command output".to_string()),
            duration: StdDuration::from_secs(1),
            timed_out: true,
        };
        let (_, turn_context) = make_session_and_context().await;

        let out = format_exec_output_str(&exec, turn_context.truncation_policy);

        assert_eq!(
            out,
            "command timed out after 1000 milliseconds\nCommand output"
        );
    }

    #[tokio::test]
    async fn turn_context_with_model_updates_model_fields() {
        let (session, mut turn_context) = make_session_and_context().await;
        turn_context.reasoning_effort = Some(ReasoningEffortConfig::Minimal);
        let updated = turn_context
            .with_model("gpt-5.1".to_string(), &session.services.models_manager)
            .await;
        let expected_model_info = session
            .services
            .models_manager
            .get_model_info("gpt-5.1", updated.config.as_ref())
            .await;

        assert_eq!(updated.config.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(updated.collaboration_mode.model(), "gpt-5.1");
        assert_eq!(updated.model_info, expected_model_info);
        assert_eq!(
            updated.reasoning_effort,
            Some(ReasoningEffortConfig::Medium)
        );
        assert_eq!(
            updated.collaboration_mode.reasoning_effort(),
            Some(ReasoningEffortConfig::Medium)
        );
        assert_eq!(
            updated.config.model_reasoning_effort,
            Some(ReasoningEffortConfig::Medium)
        );
        assert_eq!(
            updated.truncation_policy,
            expected_model_info.truncation_policy.into()
        );
        assert!(!Arc::ptr_eq(
            &updated.tool_call_gate,
            &turn_context.tool_call_gate
        ));
    }

    #[test]
    fn falls_back_to_content_when_structured_is_null() {
        let ctr = McpCallToolResult {
            content: vec![text_block("hello"), text_block("world")],
            is_error: None,
            structured_content: Some(serde_json::Value::Null),
            meta: None,
        };

        let got = FunctionCallOutputPayload::from(&ctr);
        let expected = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text(
                serde_json::to_string(&vec![text_block("hello"), text_block("world")]).unwrap(),
            ),
            success: Some(true),
        };

        assert_eq!(expected, got);
    }

    #[test]
    fn success_flag_reflects_is_error_true() {
        let ctr = McpCallToolResult {
            content: vec![text_block("unused")],
            is_error: Some(true),
            structured_content: Some(json!({ "message": "bad" })),
            meta: None,
        };

        let got = FunctionCallOutputPayload::from(&ctr);
        let expected = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text(
                serde_json::to_string(&json!({ "message": "bad" })).unwrap(),
            ),
            success: Some(false),
        };

        assert_eq!(expected, got);
    }

    #[test]
    fn success_flag_true_with_no_error_and_content_used() {
        let ctr = McpCallToolResult {
            content: vec![text_block("alpha")],
            is_error: Some(false),
            structured_content: None,
            meta: None,
        };

        let got = FunctionCallOutputPayload::from(&ctr);
        let expected = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text(
                serde_json::to_string(&vec![text_block("alpha")]).unwrap(),
            ),
            success: Some(true),
        };

        assert_eq!(expected, got);
    }

    async fn wait_for_thread_rolled_back(
        rx: &async_channel::Receiver<Event>,
    ) -> crate::protocol::ThreadRolledBackEvent {
        let deadline = StdDuration::from_secs(2);
        let start = std::time::Instant::now();
        loop {
            let remaining = deadline.saturating_sub(start.elapsed());
            let evt = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for event")
                .expect("event");
            match evt.msg {
                EventMsg::ThreadRolledBack(payload) => return payload,
                _ => continue,
            }
        }
    }

    async fn wait_for_thread_rollback_failed(rx: &async_channel::Receiver<Event>) -> ErrorEvent {
        let deadline = StdDuration::from_secs(2);
        let start = std::time::Instant::now();
        loop {
            let remaining = deadline.saturating_sub(start.elapsed());
            let evt = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for event")
                .expect("event");
            match evt.msg {
                EventMsg::Error(payload)
                    if payload.codex_error_info == Some(CodexErrorInfo::ThreadRollbackFailed) =>
                {
                    return payload;
                }
                _ => continue,
            }
        }
    }

    fn text_block(s: &str) -> serde_json::Value {
        json!({
            "type": "text",
            "text": s,
        })
    }

    async fn build_test_config(codex_home: &Path) -> Config {
        let mut config = ConfigBuilder::default()
            .codex_home(codex_home.to_path_buf())
            .build()
            .await
            .expect("load default test config");
        config.model_providers.insert(
            crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID.to_string(),
            crate::model_provider_info::ModelProviderInfo::create_antigravity_gemini_provider(),
        );
        config.model_providers.insert(
            crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID.to_string(),
            crate::model_provider_info::ModelProviderInfo::create_antigravity_anthropic_provider(),
        );
        config
    }

    fn otel_manager(
        conversation_id: ThreadId,
        config: &Config,
        model_info: &ModelInfo,
        session_source: SessionSource,
    ) -> OtelManager {
        OtelManager::new(
            conversation_id,
            ModelsManager::get_model_offline_for_tests(config.model.as_deref()).as_str(),
            model_info.slug.as_str(),
            None,
            Some("test@test.com".to_string()),
            Some(TelemetryAuthMode::Chatgpt),
            "test_originator".to_string(),
            false,
            "test".to_string(),
            session_source,
        )
    }

    pub(crate) async fn make_session_configuration_for_tests() -> SessionConfiguration {
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let config = build_test_config(codex_home.path()).await;
        let config = Arc::new(config);
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let reasoning_effort = config.model_reasoning_effort;
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };

        SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        }
    }

    #[tokio::test]
    async fn session_new_fails_when_zsh_fork_enabled_without_zsh_path() {
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let mut config = build_test_config(codex_home.path()).await;
        config.features.enable(Feature::ShellZshFork);
        config.zsh_path = None;
        let config = Arc::new(config);

        let auth_manager =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
        let models_manager = Arc::new(ModelsManager::new(
            config.codex_home.clone(),
            auth_manager.clone(),
            None,
            CollaborationModesConfig::default(),
        ));
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort: config.model_reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        };

        let (tx_event, _rx_event) = async_channel::unbounded();
        let (agent_status_tx, _agent_status_rx) = watch::channel(AgentStatus::PendingInit);
        let result = Session::new(
            session_configuration,
            Arc::clone(&config),
            auth_manager,
            models_manager,
            ExecPolicyManager::default(),
            tx_event,
            agent_status_tx,
            InitialHistory::New,
            SessionSource::Exec,
            Arc::new(SkillsManager::new(config.codex_home.clone())),
            Arc::new(FileWatcher::noop()),
            AgentControl::default(),
        )
        .await;

        let err = match result {
            Ok(_) => panic!("expected startup to fail"),
            Err(err) => err,
        };
        let msg = format!("{err:#}");
        assert!(msg.contains("zsh fork feature enabled, but `zsh_path` is not configured"));
    }

    // todo: use online model info
    pub(crate) async fn make_session_and_context() -> (Session, TurnContext) {
        let (tx_event, _rx_event) = async_channel::unbounded();
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let config = build_test_config(codex_home.path()).await;
        let config = Arc::new(config);
        let conversation_id = ThreadId::default();
        let auth_manager =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
        let models_manager = Arc::new(ModelsManager::new(
            config.codex_home.clone(),
            auth_manager.clone(),
            None,
            CollaborationModesConfig::default(),
        ));
        let agent_control = AgentControl::default();
        let exec_policy = ExecPolicyManager::default();
        let (agent_status_tx, _agent_status_rx) = watch::channel(AgentStatus::PendingInit);
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let reasoning_effort = config.model_reasoning_effort;
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools: Vec::new(),
            persist_extended_history: false,
        };
        let per_turn_config = Session::build_per_turn_config(&session_configuration);
        let model_info = ModelsManager::construct_model_info_offline_for_tests(
            session_configuration.collaboration_mode.model(),
            &per_turn_config,
        );
        let otel_manager = otel_manager(
            conversation_id,
            config.as_ref(),
            &model_info,
            session_configuration.session_source.clone(),
        );

        let state = SessionState::new(session_configuration.clone());
        let skills_manager = Arc::new(SkillsManager::new(config.codex_home.clone()));
        let network_approval = Arc::new(NetworkApprovalService::default());

        let file_watcher = Arc::new(FileWatcher::noop());
        let services = SessionServices {
            mcp_connection_manager: Arc::new(RwLock::new(
                McpConnectionManager::new_mcp_connection_manager_for_tests(
                    &config.permissions.approval_policy,
                ),
            )),
            mcp_startup_cancellation_token: Mutex::new(CancellationToken::new()),
            unified_exec_manager: UnifiedExecProcessManager::new(
                config.background_terminal_max_timeout,
            ),
            shell_zsh_path: None,
            main_execve_wrapper_exe: config.main_execve_wrapper_exe.clone(),
            analytics_events_client: AnalyticsEventsClient::new(
                Arc::clone(&config),
                Arc::clone(&auth_manager),
            ),
            hooks: Hooks::new(HooksConfig {
                legacy_notify_argv: config.notify.clone(),
            }),
            rollout: Mutex::new(None),
            user_shell: Arc::new(default_user_shell()),
            shell_snapshot_tx: watch::channel(None).0,
            show_raw_agent_reasoning: config.show_raw_agent_reasoning,
            exec_policy,
            auth_manager: auth_manager.clone(),
            otel_manager: otel_manager.clone(),
            models_manager: Arc::clone(&models_manager),
            tool_approvals: Mutex::new(ApprovalStore::default()),
            execve_session_approvals: RwLock::new(HashMap::new()),
            skills_manager,
            file_watcher,
            agent_control,
            network_proxy: None,
            network_approval: Arc::clone(&network_approval),
            state_db: None,
            model_client: ModelClient::new(
                Some(auth_manager.clone()),
                conversation_id,
                session_configuration.provider.clone(),
                session_configuration.session_source.clone(),
                config.model_verbosity,
                ws_version_from_features(config.as_ref()),
                config.features.enabled(Feature::EnableRequestCompression),
                config.features.enabled(Feature::RuntimeMetrics),
                Session::build_model_client_beta_features_header(config.as_ref()),
            ),
        };
        let js_repl = Arc::new(JsReplHandle::with_node_path(
            config.js_repl_node_path.clone(),
            config.js_repl_node_module_dirs.clone(),
        ));

        let skills_outcome = Arc::new(services.skills_manager.skills_for_config(&per_turn_config));
        let turn_context = Session::make_turn_context(
            Some(Arc::clone(&auth_manager)),
            &otel_manager,
            session_configuration.provider.clone(),
            &session_configuration,
            per_turn_config,
            model_info,
            None,
            "turn_id".to_string(),
            Arc::clone(&js_repl),
            skills_outcome,
        );

        let session = Session {
            conversation_id,
            tx_event,
            agent_status: agent_status_tx,
            state: Mutex::new(state),
            features: config.features.clone(),
            pending_mcp_server_refresh_config: Mutex::new(None),
            conversation: Arc::new(RealtimeConversationManager::new()),
            active_turn: Mutex::new(None),
            services,
            js_repl,
            next_internal_sub_id: AtomicU64::new(0),
        };

        (session, turn_context)
    }

    pub(crate) async fn make_session_and_context_with_dynamic_tools_and_rx(
        dynamic_tools: Vec<DynamicToolSpec>,
    ) -> (
        Arc<Session>,
        Arc<TurnContext>,
        async_channel::Receiver<Event>,
    ) {
        let (tx_event, rx_event) = async_channel::unbounded();
        let codex_home = tempfile::tempdir().expect("create temp dir");
        let config = build_test_config(codex_home.path()).await;
        let config = Arc::new(config);
        let conversation_id = ThreadId::default();
        let auth_manager =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
        let models_manager = Arc::new(ModelsManager::new(
            config.codex_home.clone(),
            auth_manager.clone(),
            None,
            CollaborationModesConfig::default(),
        ));
        let agent_control = AgentControl::default();
        let exec_policy = ExecPolicyManager::default();
        let (agent_status_tx, _agent_status_rx) = watch::channel(AgentStatus::PendingInit);
        let model = ModelsManager::get_model_offline_for_tests(config.model.as_deref());
        let model_info =
            ModelsManager::construct_model_info_offline_for_tests(model.as_str(), &config);
        let reasoning_effort = config.model_reasoning_effort;
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        };
        let session_configuration = SessionConfiguration {
            provider_id: config.model_provider_id.clone(),
            provider: config.model_provider.clone(),
            collaboration_mode,
            model_reasoning_summary: config.model_reasoning_summary,
            developer_instructions: config.developer_instructions.clone(),
            user_instructions: config.user_instructions.clone(),
            personality: config.personality,
            base_instructions: config
                .base_instructions
                .clone()
                .unwrap_or_else(|| model_info.get_model_instructions(config.personality)),
            compact_prompt: config.compact_prompt.clone(),
            approval_policy: config.permissions.approval_policy.clone(),
            approvals_reviewer: config.approvals_reviewer,
            sandbox_policy: config.permissions.sandbox_policy.clone(),
            windows_sandbox_level: WindowsSandboxLevel::from_config(&config),
            cwd: config.cwd.clone(),
            codex_home: config.codex_home.clone(),
            thread_name: None,
            original_config_do_not_use: Arc::clone(&config),
            metrics_service_name: None,
            session_source: SessionSource::Exec,
            dynamic_tools,
            persist_extended_history: false,
        };
        let per_turn_config = Session::build_per_turn_config(&session_configuration);
        let model_info = ModelsManager::construct_model_info_offline_for_tests(
            session_configuration.collaboration_mode.model(),
            &per_turn_config,
        );
        let otel_manager = otel_manager(
            conversation_id,
            config.as_ref(),
            &model_info,
            session_configuration.session_source.clone(),
        );

        let state = SessionState::new(session_configuration.clone());
        let skills_manager = Arc::new(SkillsManager::new(config.codex_home.clone()));
        let network_approval = Arc::new(NetworkApprovalService::default());

        let file_watcher = Arc::new(FileWatcher::noop());
        let services = SessionServices {
            mcp_connection_manager: Arc::new(RwLock::new(
                McpConnectionManager::new_mcp_connection_manager_for_tests(
                    &config.permissions.approval_policy,
                ),
            )),
            mcp_startup_cancellation_token: Mutex::new(CancellationToken::new()),
            unified_exec_manager: UnifiedExecProcessManager::new(
                config.background_terminal_max_timeout,
            ),
            shell_zsh_path: None,
            main_execve_wrapper_exe: config.main_execve_wrapper_exe.clone(),
            analytics_events_client: AnalyticsEventsClient::new(
                Arc::clone(&config),
                Arc::clone(&auth_manager),
            ),
            hooks: Hooks::new(HooksConfig {
                legacy_notify_argv: config.notify.clone(),
            }),
            rollout: Mutex::new(None),
            user_shell: Arc::new(default_user_shell()),
            shell_snapshot_tx: watch::channel(None).0,
            show_raw_agent_reasoning: config.show_raw_agent_reasoning,
            exec_policy,
            auth_manager: Arc::clone(&auth_manager),
            otel_manager: otel_manager.clone(),
            models_manager: Arc::clone(&models_manager),
            tool_approvals: Mutex::new(ApprovalStore::default()),
            execve_session_approvals: RwLock::new(HashMap::new()),
            skills_manager,
            file_watcher,
            agent_control,
            network_proxy: None,
            network_approval: Arc::clone(&network_approval),
            state_db: None,
            model_client: ModelClient::new(
                Some(Arc::clone(&auth_manager)),
                conversation_id,
                session_configuration.provider.clone(),
                session_configuration.session_source.clone(),
                config.model_verbosity,
                ws_version_from_features(config.as_ref()),
                config.features.enabled(Feature::EnableRequestCompression),
                config.features.enabled(Feature::RuntimeMetrics),
                Session::build_model_client_beta_features_header(config.as_ref()),
            ),
        };
        let js_repl = Arc::new(JsReplHandle::with_node_path(
            config.js_repl_node_path.clone(),
            config.js_repl_node_module_dirs.clone(),
        ));

        let skills_outcome = Arc::new(services.skills_manager.skills_for_config(&per_turn_config));
        let turn_context = Arc::new(Session::make_turn_context(
            Some(Arc::clone(&auth_manager)),
            &otel_manager,
            session_configuration.provider.clone(),
            &session_configuration,
            per_turn_config,
            model_info,
            None,
            "turn_id".to_string(),
            Arc::clone(&js_repl),
            skills_outcome,
        ));

        let session = Arc::new(Session {
            conversation_id,
            tx_event,
            agent_status: agent_status_tx,
            state: Mutex::new(state),
            features: config.features.clone(),
            pending_mcp_server_refresh_config: Mutex::new(None),
            conversation: Arc::new(RealtimeConversationManager::new()),
            active_turn: Mutex::new(None),
            services,
            js_repl,
            next_internal_sub_id: AtomicU64::new(0),
        });

        (session, turn_context, rx_event)
    }

    // Like make_session_and_context, but returns Arc<Session> and the event receiver
    // so tests can assert on emitted events.
    pub(crate) async fn make_session_and_context_with_rx() -> (
        Arc<Session>,
        Arc<TurnContext>,
        async_channel::Receiver<Event>,
    ) {
        make_session_and_context_with_dynamic_tools_and_rx(Vec::new()).await
    }

    #[tokio::test]
    async fn refresh_mcp_servers_is_deferred_until_next_turn() {
        let (session, turn_context) = make_session_and_context().await;
        let old_token = session.mcp_startup_cancellation_token().await;
        assert!(!old_token.is_cancelled());

        let mcp_oauth_credentials_store_mode =
            serde_json::to_value(OAuthCredentialsStoreMode::Auto).expect("serialize store mode");
        let refresh_config = McpServerRefreshConfig {
            mcp_servers: json!({}),
            mcp_oauth_credentials_store_mode,
        };
        {
            let mut guard = session.pending_mcp_server_refresh_config.lock().await;
            *guard = Some(refresh_config);
        }

        assert!(!old_token.is_cancelled());
        assert!(
            session
                .pending_mcp_server_refresh_config
                .lock()
                .await
                .is_some()
        );

        session
            .refresh_mcp_servers_if_requested(&turn_context)
            .await;

        assert!(old_token.is_cancelled());
        assert!(
            session
                .pending_mcp_server_refresh_config
                .lock()
                .await
                .is_none()
        );
        let new_token = session.mcp_startup_cancellation_token().await;
        assert!(!new_token.is_cancelled());
    }

    #[tokio::test]
    async fn record_model_warning_appends_user_message() {
        let (mut session, turn_context) = make_session_and_context().await;
        let features = Features::with_defaults();
        session.features = features;

        session
            .record_model_warning("too many unified exec processes", &turn_context)
            .await;

        let history = session.clone_history().await;
        let history_items = history.raw_items();
        let last = history_items.last().expect("warning recorded");

        match last {
            ResponseItem::Message { role, content, .. } => {
                assert_eq!(role, "user");
                assert_eq!(
                    content,
                    &vec![ContentItem::InputText {
                        text: "Warning: too many unified exec processes".to_string(),
                    }]
                );
            }
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn spawn_task_does_not_update_previous_model_for_non_run_turn_tasks() {
        let (sess, tc, _rx) = make_session_and_context_with_rx().await;
        sess.set_previous_model(None).await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];

        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;
        assert_eq!(sess.previous_model().await, None);
    }

    #[tokio::test]
    async fn build_settings_update_items_emits_environment_item_for_network_changes() {
        let (session, previous_context) = make_session_and_context().await;
        let previous_context = Arc::new(previous_context);
        let mut current_context = previous_context
            .with_model(
                previous_context.model_info.slug.clone(),
                &session.services.models_manager,
            )
            .await;

        let mut config = (*current_context.config).clone();
        let mut requirements = config.config_layer_stack.requirements().clone();
        requirements.network = Some(Sourced::new(
            NetworkConstraints {
                allowed_domains: Some(vec!["api.example.com".to_string()]),
                denied_domains: Some(vec!["blocked.example.com".to_string()]),
                ..Default::default()
            },
            RequirementSource::CloudRequirements,
        ));
        let layers = config
            .config_layer_stack
            .get_layers(ConfigLayerStackOrdering::LowestPrecedenceFirst, true)
            .into_iter()
            .cloned()
            .collect();
        config.config_layer_stack = ConfigLayerStack::new(
            layers,
            requirements,
            config.config_layer_stack.requirements_toml().clone(),
        )
        .expect("rebuild config layer stack with network requirements");
        current_context.config = Arc::new(config);

        let reference_context_item = previous_context.to_turn_context_item();
        let update_items = session.build_settings_update_items(
            Some(&reference_context_item),
            None,
            &current_context,
        );

        let environment_update = update_items
            .iter()
            .find_map(|item| match item {
                ResponseItem::Message { role, content, .. } if role == "user" => {
                    let [ContentItem::InputText { text }] = content.as_slice() else {
                        return None;
                    };
                    text.contains("<environment_context>").then_some(text)
                }
                _ => None,
            })
            .expect("environment update item should be emitted");
        assert!(environment_update.contains("<network enabled=\"true\">"));
        assert!(environment_update.contains("<allowed>api.example.com</allowed>"));
        assert!(environment_update.contains("<denied>blocked.example.com</denied>"));
    }

    #[tokio::test]
    async fn record_context_updates_and_set_reference_context_item_injects_full_context_when_baseline_missing()
     {
        let (session, turn_context) = make_session_and_context().await;
        session
            .record_context_updates_and_set_reference_context_item(&turn_context, None)
            .await;
        let history = session.clone_history().await;
        let initial_context = session.build_initial_context(&turn_context, None).await;
        assert_eq!(history.raw_items().to_vec(), initial_context);

        let current_context = session.reference_context_item().await;
        assert_eq!(
            serde_json::to_value(current_context).expect("serialize current context item"),
            serde_json::to_value(Some(turn_context.to_turn_context_item()))
                .expect("serialize expected context item")
        );
    }

    #[tokio::test]
    async fn record_context_updates_and_set_reference_context_item_reinjects_full_context_after_clear()
     {
        let (session, turn_context) = make_session_and_context().await;
        let compacted_summary = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!("{}\nsummary", crate::compact::SUMMARY_PREFIX),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        session
            .record_into_history(std::slice::from_ref(&compacted_summary), &turn_context)
            .await;
        session
            .record_context_updates_and_set_reference_context_item(&turn_context, None)
            .await;
        {
            let mut state = session.state.lock().await;
            state.set_reference_context_item(None);
        }
        session
            .replace_history(vec![compacted_summary.clone()], None)
            .await;

        session
            .record_context_updates_and_set_reference_context_item(&turn_context, None)
            .await;

        let history = session.clone_history().await;
        let mut expected_history = vec![compacted_summary];
        expected_history.extend(session.build_initial_context(&turn_context, None).await);
        assert_eq!(history.raw_items().to_vec(), expected_history);
    }

    #[tokio::test]
    async fn build_initial_context_prepends_model_switch_message() {
        let (session, turn_context) = make_session_and_context().await;

        let initial_context = session
            .build_initial_context(&turn_context, Some("previous-regular-model"))
            .await;

        let ResponseItem::Message { role, content, .. } = &initial_context[0] else {
            panic!("expected developer message");
        };
        assert_eq!(role, "developer");
        let [ContentItem::InputText { text }, ..] = content.as_slice() else {
            panic!("expected developer text");
        };
        assert!(text.contains("<model_switch>"));
    }

    #[tokio::test]
    async fn run_user_shell_command_does_not_set_reference_context_item() {
        let (session, _turn_context, rx) = make_session_and_context_with_rx().await;
        {
            let mut state = session.state.lock().await;
            state.set_reference_context_item(None);
        }

        handlers::run_user_shell_command(&session, "sub-id".to_string(), "echo shell".to_string())
            .await;

        let deadline = StdDuration::from_secs(5);
        let start = std::time::Instant::now();
        loop {
            let remaining = deadline.saturating_sub(start.elapsed());
            let evt = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for event")
                .expect("event");
            if matches!(evt.msg, EventMsg::TurnComplete(_)) {
                break;
            }
        }

        assert!(
            session.reference_context_item().await.is_none(),
            "standalone shell tasks should not mutate previous context"
        );
    }

    #[derive(Clone, Copy)]
    struct NeverEndingTask {
        kind: TaskKind,
        listen_to_cancellation_token: bool,
    }

    #[async_trait::async_trait]
    impl SessionTask for NeverEndingTask {
        fn kind(&self) -> TaskKind {
            self.kind
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<SessionTaskContext>,
            _ctx: Arc<TurnContext>,
            _input: Vec<UserInput>,
            cancellation_token: CancellationToken,
        ) -> Option<String> {
            if self.listen_to_cancellation_token {
                cancellation_token.cancelled().await;
                return None;
            }
            loop {
                sleep(Duration::from_secs(60)).await;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[test_log::test]
    async fn abort_regular_task_emits_turn_aborted_only() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;

        // Interrupts persist a model-visible `<turn_aborted>` marker into history, but there is no
        // separate client-visible event for that marker (only `EventMsg::TurnAborted`).
        let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout waiting for event")
            .expect("event");
        match evt.msg {
            EventMsg::TurnAborted(e) => assert_eq!(TurnAbortReason::Interrupted, e.reason),
            other => panic!("unexpected event: {other:?}"),
        }
        // No extra events should be emitted after an abort.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn abort_gracefully_emits_turn_aborted_only() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: true,
            },
        )
        .await;

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;

        // Even if tasks handle cancellation gracefully, interrupts still result in `TurnAborted`
        // being the only client-visible signal.
        let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout waiting for event")
            .expect("event");
        match evt.msg {
            EventMsg::TurnAborted(e) => assert_eq!(TurnAbortReason::Interrupted, e.reason),
            other => panic!("unexpected event: {other:?}"),
        }
        // No extra events should be emitted after an abort.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn task_finish_persists_leftover_pending_input() {
        let (sess, tc, _rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

        sess.inject_response_items(vec![ResponseInputItem::Message {
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "late pending input".to_string(),
            }],
        }])
        .await
        .expect("inject pending input into active turn");

        sess.on_task_finished(Arc::clone(&tc), None).await;

        let history = sess.clone_history().await;
        let expected = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "late pending input".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        assert!(
            history.raw_items().iter().any(|item| item == &expected),
            "expected pending input to be persisted into history on turn completion"
        );
    }

    #[tokio::test]
    async fn steer_input_requires_active_turn() {
        let (sess, _tc, _rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }];

        let err = sess
            .steer_input(input, None)
            .await
            .expect_err("steering without active turn should fail");

        assert!(matches!(err, SteerInputError::NoActiveTurn(_)));
    }

    #[tokio::test]
    async fn steer_input_enforces_expected_turn_id() {
        let (sess, tc, _rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

        let steer_input = vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }];
        let err = sess
            .steer_input(steer_input, Some("different-turn-id"))
            .await
            .expect_err("mismatched expected turn id should fail");

        match err {
            SteerInputError::ExpectedTurnMismatch { expected, actual } => {
                assert_eq!(
                    (expected, actual),
                    ("different-turn-id".to_string(), tc.sub_id.clone())
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn steer_input_returns_active_turn_id() {
        let (sess, tc, _rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

        let steer_input = vec![UserInput::Text {
            text: "steer".to_string(),
            text_elements: Vec::new(),
        }];
        let turn_id = sess
            .steer_input(steer_input, Some(&tc.sub_id))
            .await
            .expect("steering with matching expected turn id should succeed");

        assert_eq!(turn_id, tc.sub_id);
        assert!(sess.has_pending_input().await);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn user_turn_replaces_active_task_when_provider_changes() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(
            Arc::clone(&tc),
            input,
            NeverEndingTask {
                kind: TaskKind::Regular,
                listen_to_cancellation_token: false,
            },
        )
        .await;

        handlers::user_input_or_turn(
            &sess,
            "replacement-turn".to_string(),
            Op::UserTurn {
                items: vec![UserInput::Text {
                    text: "switch provider".to_string(),
                    text_elements: Vec::new(),
                }],
                cwd: tc.cwd.clone(),
                approval_policy: tc.approval_policy.value(),
                sandbox_policy: tc.sandbox_policy.get().clone(),
                model: "gemma-3n".to_string(),
                effort: tc.reasoning_effort,
                summary: tc.reasoning_summary,
                final_output_json_schema: None,
                collaboration_mode: None,
                personality: tc.personality,
            },
        )
        .await;

        let mut saw_replaced_abort = false;
        let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let event = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for event")
                .expect("event");
            if let EventMsg::TurnAborted(turn_aborted) = event.msg
                && turn_aborted.reason == TurnAbortReason::Replaced
            {
                saw_replaced_abort = true;
                break;
            }
        }

        assert!(
            saw_replaced_abort,
            "expected active task to be replaced when provider changes"
        );

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abort_review_task_emits_exited_then_aborted_and_records_history() {
        let (sess, tc, rx) = make_session_and_context_with_rx().await;
        let input = vec![UserInput::Text {
            text: "start review".to_string(),
            text_elements: Vec::new(),
        }];
        sess.spawn_task(Arc::clone(&tc), input, ReviewTask::new())
            .await;

        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;

        // Aborting a review task should exit review mode before surfacing the abort to the client.
        // We scan for these events (rather than relying on fixed ordering) since unrelated events
        // may interleave.
        let mut exited_review_mode_idx = None;
        let mut turn_aborted_idx = None;
        let mut idx = 0usize;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let evt = tokio::time::timeout(remaining, rx.recv())
                .await
                .expect("timeout waiting for event")
                .expect("event");
            let event_idx = idx;
            idx = idx.saturating_add(1);
            match evt.msg {
                EventMsg::ExitedReviewMode(ev) => {
                    assert!(ev.review_output.is_none());
                    exited_review_mode_idx = Some(event_idx);
                }
                EventMsg::TurnAborted(ev) => {
                    assert_eq!(TurnAbortReason::Interrupted, ev.reason);
                    turn_aborted_idx = Some(event_idx);
                    break;
                }
                _ => {}
            }
        }
        assert!(
            exited_review_mode_idx.is_some(),
            "expected ExitedReviewMode after abort"
        );
        assert!(
            turn_aborted_idx.is_some(),
            "expected TurnAborted after abort"
        );
        assert!(
            exited_review_mode_idx.unwrap() < turn_aborted_idx.unwrap(),
            "expected ExitedReviewMode before TurnAborted"
        );

        let history = sess.clone_history().await;
        // The `<turn_aborted>` marker is silent in the event stream, so verify it is still
        // recorded in history for the model.
        assert!(
            history.raw_items().iter().any(|item| {
                let ResponseItem::Message { role, content, .. } = item else {
                    return false;
                };
                if role != "user" {
                    return false;
                }
                content.iter().any(|content_item| {
                    let ContentItem::InputText { text } = content_item else {
                        return false;
                    };
                    text.contains(crate::contextual_user_message::TURN_ABORTED_OPEN_TAG)
                })
            }),
            "expected a model-visible turn aborted marker in history after interrupt"
        );
    }

    #[tokio::test]
    async fn fatal_tool_error_stops_turn_and_reports_error() {
        let (session, turn_context, _rx) = make_session_and_context_with_rx().await;
        let tools = {
            session
                .services
                .mcp_connection_manager
                .read()
                .await
                .list_all_tools()
                .await
        };
        let app_tools = Some(tools.clone());
        let router = ToolRouter::from_config(
            &turn_context.tools_config,
            Some(
                tools
                    .into_iter()
                    .map(|(name, tool)| (name, tool.tool))
                    .collect(),
            ),
            app_tools,
            turn_context.dynamic_tools.as_slice(),
        );
        let item = ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "call-1".to_string(),
            name: "shell".to_string(),
            input: "{}".to_string(),
        };

        let call = ToolRouter::build_tool_call(session.as_ref(), item.clone())
            .await
            .expect("build tool call")
            .expect("tool call present");
        let tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));
        let err = router
            .dispatch_tool_call(
                Arc::clone(&session),
                Arc::clone(&turn_context),
                tracker,
                call,
                ToolCallSource::Direct,
            )
            .await
            .expect_err("expected fatal error");

        match err {
            FunctionCallError::Fatal(message) => {
                assert_eq!(message, "tool shell invoked with incompatible payload");
            }
            other => panic!("expected FunctionCallError::Fatal, got {other:?}"),
        }
    }

    async fn sample_rollout(
        session: &Session,
        _turn_context: &TurnContext,
    ) -> (Vec<RolloutItem>, Vec<ResponseItem>) {
        let mut rollout_items = Vec::new();
        let mut live_history = ContextManager::new();

        // Use the same turn_context source as record_initial_history so model_info (and thus
        // personality_spec) matches reconstruction.
        let reconstruction_turn = session.new_default_turn().await;
        let mut initial_context = session
            .build_initial_context(reconstruction_turn.as_ref(), None)
            .await;
        // Ensure personality_spec is present when Personality is enabled, so expected matches
        // what reconstruction produces (build_initial_context may omit it when baked into model).
        if !initial_context.iter().any(|m| {
            matches!(m, ResponseItem::Message { role, content, .. }
                if role == "developer"
                    && content.iter().any(|c| {
                        matches!(c, ContentItem::InputText { text } if text.contains("<personality_spec>"))
                    }))
        })
            && let Some(p) = reconstruction_turn.personality
            && session.features.enabled(Feature::Personality)
            && let Some(personality_message) = reconstruction_turn
                .model_info
                .model_messages
                .as_ref()
                .and_then(|m| m.get_personality_message(Some(p)).filter(|s| !s.is_empty()))
        {
            let msg =
                DeveloperInstructions::personality_spec_message(personality_message).into();
            let insert_at = initial_context
                .iter()
                .position(|m| matches!(m, ResponseItem::Message { role, .. } if role == "developer"))
                .map(|i| i + 1)
                .unwrap_or(0);
            initial_context.insert(insert_at, msg);
        }
        for item in &initial_context {
            rollout_items.push(RolloutItem::ResponseItem(item.clone()));
        }
        live_history.record_items(
            initial_context.iter(),
            reconstruction_turn.truncation_policy,
        );

        let user1 = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "first user".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&user1),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(user1.clone()));

        let assistant1 = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: "assistant reply one".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&assistant1),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(assistant1.clone()));

        let summary1 = "summary one";
        let snapshot1 = live_history
            .clone()
            .for_prompt(&reconstruction_turn.model_info.input_modalities);
        let user_messages1 = collect_user_messages(&snapshot1);
        let rebuilt1 =
            compact::build_compacted_history(initial_context.clone(), &user_messages1, summary1);
        live_history.replace(rebuilt1);
        rollout_items.push(RolloutItem::Compacted(CompactedItem {
            message: summary1.to_string(),
            replacement_history: None,
        }));

        let user2 = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "second user".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&user2),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(user2.clone()));

        let assistant2 = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: "assistant reply two".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&assistant2),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(assistant2.clone()));

        let summary2 = "summary two";
        let snapshot2 = live_history
            .clone()
            .for_prompt(&reconstruction_turn.model_info.input_modalities);
        let user_messages2 = collect_user_messages(&snapshot2);
        let rebuilt2 =
            compact::build_compacted_history(initial_context.clone(), &user_messages2, summary2);
        live_history.replace(rebuilt2);
        rollout_items.push(RolloutItem::Compacted(CompactedItem {
            message: summary2.to_string(),
            replacement_history: None,
        }));

        let user3 = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "third user".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&user3),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(user3));

        let assistant3 = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: "assistant reply three".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        live_history.record_items(
            std::iter::once(&assistant3),
            reconstruction_turn.truncation_policy,
        );
        rollout_items.push(RolloutItem::ResponseItem(assistant3));

        (
            rollout_items,
            live_history.for_prompt(&reconstruction_turn.model_info.input_modalities),
        )
    }

    #[tokio::test]
    async fn rejects_escalated_permissions_when_policy_not_on_request() {
        use crate::exec::ExecParams;
        use crate::protocol::AskForApproval;
        use crate::protocol::SandboxPolicy;
        use crate::sandboxing::SandboxPermissions;
        use crate::turn_diff_tracker::TurnDiffTracker;
        use std::collections::HashMap;

        let (session, mut turn_context_raw) = make_session_and_context().await;
        // Ensure policy is NOT OnRequest so the early rejection path triggers
        turn_context_raw
            .approval_policy
            .set(AskForApproval::OnFailure)
            .expect("test setup should allow updating approval policy");
        let session = Arc::new(session);
        let mut turn_context = Arc::new(turn_context_raw);

        let timeout_ms = 1000;
        let sandbox_permissions = SandboxPermissions::RequireEscalated;
        let params = ExecParams {
            command: if cfg!(windows) {
                vec![
                    "cmd.exe".to_string(),
                    "/C".to_string(),
                    "echo hi".to_string(),
                ]
            } else {
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "echo hi".to_string(),
                ]
            },
            cwd: turn_context.cwd.clone(),
            expiration: timeout_ms.into(),
            env: HashMap::new(),
            network: None,
            sandbox_permissions,
            windows_sandbox_level: turn_context.windows_sandbox_level,
            justification: Some("test".to_string()),
            arg0: None,
        };

        let params2 = ExecParams {
            sandbox_permissions: SandboxPermissions::UseDefault,
            command: params.command.clone(),
            cwd: params.cwd.clone(),
            expiration: timeout_ms.into(),
            env: HashMap::new(),
            network: None,
            windows_sandbox_level: turn_context.windows_sandbox_level,
            justification: params.justification.clone(),
            arg0: None,
        };

        let turn_diff_tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));

        let tool_name = "shell";
        let call_id = "test-call".to_string();

        let handler = ShellHandler;
        let resp = handler
            .handle(ToolInvocation {
                session: Arc::clone(&session),
                turn: Arc::clone(&turn_context),
                tracker: Arc::clone(&turn_diff_tracker),
                call_id,
                tool_name: tool_name.to_string(),
                payload: ToolPayload::Function {
                    arguments: serde_json::json!({
                        "command": params.command.clone(),
                        "workdir": Some(turn_context.cwd.to_string_lossy().to_string()),
                        "timeout_ms": params.expiration.timeout_ms(),
                        "sandbox_permissions": params.sandbox_permissions,
                        "justification": params.justification.clone(),
                    })
                    .to_string(),
                },
            })
            .await;

        let Err(FunctionCallError::RespondToModel(output)) = resp else {
            panic!("expected error result");
        };

        let expected = format!(
            "approval policy is {policy:?}; reject command — you should not ask for escalated permissions if the approval policy is {policy:?}",
            policy = turn_context.approval_policy.value()
        );

        pretty_assertions::assert_eq!(output, expected);

        // Now retry the same command WITHOUT escalated permissions; should succeed.
        // Force DangerFullAccess to avoid platform sandbox dependencies in tests.
        Arc::get_mut(&mut turn_context)
            .expect("unique turn context Arc")
            .sandbox_policy
            .set(SandboxPolicy::DangerFullAccess)
            .expect("test setup should allow updating sandbox policy");

        let resp2 = handler
            .handle(ToolInvocation {
                session: Arc::clone(&session),
                turn: Arc::clone(&turn_context),
                tracker: Arc::clone(&turn_diff_tracker),
                call_id: "test-call-2".to_string(),
                tool_name: tool_name.to_string(),
                payload: ToolPayload::Function {
                    arguments: serde_json::json!({
                        "command": params2.command.clone(),
                        "workdir": Some(turn_context.cwd.to_string_lossy().to_string()),
                        "timeout_ms": params2.expiration.timeout_ms(),
                        "sandbox_permissions": params2.sandbox_permissions,
                        "justification": params2.justification.clone(),
                    })
                    .to_string(),
                },
            })
            .await;

        let output = match resp2.expect("expected Ok result") {
            ToolOutput::Function {
                body: FunctionCallOutputBody::Text(content),
                ..
            } => content,
            _ => panic!("unexpected tool output"),
        };

        #[derive(Deserialize, PartialEq, Eq, Debug)]
        struct ResponseExecMetadata {
            exit_code: i32,
        }

        #[derive(Deserialize)]
        struct ResponseExecOutput {
            output: String,
            metadata: ResponseExecMetadata,
        }

        let exec_output: ResponseExecOutput =
            serde_json::from_str(&output).expect("valid exec output json");

        pretty_assertions::assert_eq!(exec_output.metadata, ResponseExecMetadata { exit_code: 0 });
        assert!(exec_output.output.contains("hi"));
    }
    #[tokio::test]
    async fn unified_exec_rejects_escalated_permissions_when_policy_not_on_request() {
        use crate::protocol::AskForApproval;
        use crate::sandboxing::SandboxPermissions;
        use crate::turn_diff_tracker::TurnDiffTracker;

        let (session, mut turn_context_raw) = make_session_and_context().await;
        turn_context_raw
            .approval_policy
            .set(AskForApproval::OnFailure)
            .expect("test setup should allow updating approval policy");
        let session = Arc::new(session);
        let turn_context = Arc::new(turn_context_raw);
        let tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));

        let handler = UnifiedExecHandler;
        let resp = handler
            .handle(ToolInvocation {
                session: Arc::clone(&session),
                turn: Arc::clone(&turn_context),
                tracker: Arc::clone(&tracker),
                call_id: "exec-call".to_string(),
                tool_name: "exec_command".to_string(),
                payload: ToolPayload::Function {
                    arguments: serde_json::json!({
                        "cmd": "echo hi",
                        "sandbox_permissions": SandboxPermissions::RequireEscalated,
                        "justification": "need unsandboxed execution",
                    })
                    .to_string(),
                },
            })
            .await;

        let Err(FunctionCallError::RespondToModel(output)) = resp else {
            panic!("expected error result");
        };

        let expected = format!(
            "approval policy is {policy:?}; reject command — you cannot ask for escalated permissions if the approval policy is {policy:?}",
            policy = turn_context.approval_policy.value()
        );

        pretty_assertions::assert_eq!(output, expected);
    }
}
