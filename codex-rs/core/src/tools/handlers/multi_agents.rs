use crate::agent::AgentStatus;
use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::config::Config;
use crate::error::CodexErr;
use crate::features::Feature;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::ThreadId;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::protocol::CollabAgentInteractionBeginEvent;
use codex_protocol::protocol::CollabAgentInteractionEndEvent;
use codex_protocol::protocol::CollabAgentModelSource;
use codex_protocol::protocol::CollabAgentModelSourceDetail;
use codex_protocol::protocol::CollabAgentRef;
use codex_protocol::protocol::CollabAgentSpawnBeginEvent;
use codex_protocol::protocol::CollabAgentSpawnEndEvent;
use codex_protocol::protocol::CollabAgentStatusEntry;
use codex_protocol::protocol::CollabCloseBeginEvent;
use codex_protocol::protocol::CollabCloseEndEvent;
use codex_protocol::protocol::CollabResumeBeginEvent;
use codex_protocol::protocol::CollabResumeEndEvent;
use codex_protocol::protocol::CollabWaitingBeginEvent;
use codex_protocol::protocol::CollabWaitingEndEvent;
use codex_protocol::protocol::MemoryLink;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

pub struct MultiAgentHandler;

/// Minimum wait timeout to prevent tight polling loops from burning CPU.
pub(crate) const MIN_WAIT_TIMEOUT_MS: i64 = 10_000;
pub(crate) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;
pub(crate) const MAX_WAIT_TIMEOUT_MS: i64 = 3600 * 1000;

#[derive(Debug, Deserialize)]
struct CloseAgentArgs {
    id: String,
}

#[async_trait]
impl ToolHandler for MultiAgentHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            tool_name,
            payload,
            call_id,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "collab handler received unsupported payload".to_string(),
                ));
            }
        };

        match tool_name.as_str() {
            "spawn_agent" => spawn::handle(session, turn, call_id, arguments).await,
            "send_input" => send_input::handle(session, turn, call_id, arguments).await,
            "resume_agent" => resume_agent::handle(session, turn, call_id, arguments).await,
            "wait" => wait::handle(session, turn, call_id, arguments).await,
            "close_agent" => close_agent::handle(session, turn, call_id, arguments).await,
            "record_model_sub_duel" => {
                record_model_sub_duel::handle(session, turn, call_id, arguments).await
            }
            "record_model_sub_winner" => {
                record_model_sub_winner::handle(session, turn, call_id, arguments).await
            }
            "calibrate_model_sub" => {
                calibrate_model_sub::handle(session, turn, call_id, arguments).await
            }
            other => Err(FunctionCallError::RespondToModel(format!(
                "unsupported collab tool {other}"
            ))),
        }
    }
}

mod spawn {
    use super::*;
    use crate::agent::MAX_THREAD_SPAWN_DEPTH;
    use crate::agent::role::DEFAULT_ROLE_NAME;
    use crate::agent::role::apply_role_to_config;
    use crate::agent_worktree;
    use crate::context_packet;
    use crate::utility_model;

    use crate::agent::exceeds_thread_spawn_depth_limit;
    use crate::agent::next_thread_spawn_depth;
    use std::sync::Arc;

    #[derive(Debug, Deserialize)]
    struct SpawnAgentArgs {
        message: Option<String>,
        items: Option<Vec<UserInput>>,
        agent_type: Option<String>,
        model: Option<String>,
    }

    const AUTO_CALIBRATION_PREVIEW_CHARS: usize = 240;

    #[derive(Debug, Clone, Deserialize)]
    struct AutoCalibrationRawResult {
        task_bucket: Option<String>,
        #[serde(default)]
        runs: Vec<AutoCalibrationRawRun>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_vouch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_latency: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_session: Option<String>,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct AutoCalibrationRawRun {
        model: String,
        #[serde(default)]
        agent_id: Option<String>,
        status: AgentStatus,
        elapsed_ms: u64,
    }

    #[derive(Debug, Clone, Serialize)]
    struct AutoCalibrationResult {
        #[serde(skip_serializing_if = "Option::is_none")]
        task_bucket: Option<String>,
        runs: Vec<AutoCalibrationRunSummary>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_vouch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_latency: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_session: Option<String>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct AutoCalibrationRunSummary {
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        status: String,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_preview: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct SpawnAgentResult {
        agent_id: String,
        nickname: Option<String>,
        agent_type: String,
        model: String,
        model_provider_id: String,
        model_source: CollabAgentModelSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_source_detail: Option<CollabAgentModelSourceDetail>,
        parent_thread_id: String,
        spawn_depth: i32,
        worktree_path: Option<String>,
        branch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_binding_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        auto_calibration: Option<AutoCalibrationResult>,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: SpawnAgentArgs = parse_arguments(&arguments)?;
        let SpawnAgentArgs {
            message,
            items,
            agent_type,
            model,
        } = args;
        let role_name = agent_type
            .as_deref()
            .map(str::trim)
            .filter(|role| !role.is_empty());
        let requested_model = model
            .as_deref()
            .map(str::trim)
            .filter(|model_slug| !model_slug.is_empty())
            .map(ToOwned::to_owned);
        let agent_type = role_name.unwrap_or("default").to_string();
        let mut input_items = parse_collab_input(message, items)?;
        let prompt = input_preview(&input_items);
        let prompt_for_events = prompt.clone();
        let session_source = turn.session_source.clone();
        let child_depth = next_thread_spawn_depth(&session_source);
        let max_depth = turn.config.agent_max_depth;
        if exceeds_thread_spawn_depth_limit(child_depth, max_depth) {
            return Err(FunctionCallError::RespondToModel(
                "Agent depth limit reached. Solve the task yourself.".to_string(),
            ));
        }
        let memory = super::active_memory_link(turn.as_ref()).await;
        session
            .send_event(
                &turn,
                CollabAgentSpawnBeginEvent {
                    call_id: call_id.clone(),
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    agent_type: Some(agent_type.clone()),
                    prompt: prompt_for_events.clone(),
                }
                .into(),
            )
            .await;
        let mut model_source: Option<CollabAgentModelSource> = None;
        let mut model_source_detail: Option<CollabAgentModelSourceDetail> = None;
        let (mut config, mut model_for_events, mut model_provider_id_for_events) =
            match build_agent_spawn_config(&session.get_base_instructions().await, turn.as_ref()) {
                Ok(config) => {
                    let model_for_events = config.model.clone();
                    let model_provider_id_for_events = Some(config.model_provider_id.clone());
                    (config, model_for_events, model_provider_id_for_events)
                }
                Err(err) => {
                    session
                        .send_event(
                            &turn,
                            CollabAgentSpawnEndEvent {
                                call_id,
                                sender_thread_id: session.conversation_id,
                                memory: memory.clone(),
                                agent_type: Some(agent_type),
                                model: None,
                                model_provider_id: None,
                                model_source: None,
                                model_source_detail: None,
                                new_thread_id: None,
                                new_agent_nickname: None,
                                new_agent_role: None,
                                prompt: prompt_for_events,
                                status: AgentStatus::Errored(err.to_string()),
                            }
                            .into(),
                        )
                        .await;
                    return Err(err);
                }
            };
        if let Err(err) = apply_role_to_config(&mut config, role_name)
            .await
            .map_err(FunctionCallError::RespondToModel)
            .and_then(|_| apply_spawn_agent_runtime_overrides(&mut config, turn.as_ref()))
        {
            session
                .send_event(
                    &turn,
                    CollabAgentSpawnEndEvent {
                        call_id,
                        sender_thread_id: session.conversation_id,
                        memory: memory.clone(),
                        agent_type: Some(agent_type),
                        model: model_for_events,
                        model_provider_id: model_provider_id_for_events,
                        model_source,
                        model_source_detail,
                        new_thread_id: None,
                        new_agent_nickname: None,
                        new_agent_role: None,
                        prompt: prompt_for_events,
                        status: AgentStatus::Errored(err.to_string()),
                    }
                    .into(),
                )
                .await;
            return Err(err);
        }
        model_source = Some(
            if role_name.is_some_and(|role| !role.eq_ignore_ascii_case("default")) {
                CollabAgentModelSource::Role
            } else {
                CollabAgentModelSource::Parent
            },
        );
        model_source_detail = role_name
            .is_some_and(|role| !role.eq_ignore_ascii_case("default"))
            .then_some(CollabAgentModelSourceDetail::RoleConfig);
        let mut auto_calibration = None;
        let uses_default_role = role_name.is_none_or(|role| role.eq_ignore_ascii_case("default"));
        let uses_explorer_role =
            role_name.is_some_and(|role| role.eq_ignore_ascii_case("explorer"));
        if uses_default_role || uses_explorer_role {
            let mut selected_model_sub = turn.config.model_sub.clone().map(|model_sub| {
                (
                    model_sub,
                    CollabAgentModelSource::ModelSub,
                    CollabAgentModelSourceDetail::ConfigModelSub,
                )
            });
            if selected_model_sub.is_none() {
                if let Some(auto_model_sub) = session.get_auto_model_sub_selection().await {
                    selected_model_sub = Some((
                        auto_model_sub,
                        CollabAgentModelSource::ModelSubAuto,
                        CollabAgentModelSourceDetail::SessionCache,
                    ));
                } else {
                    for candidate in
                        crate::model_sub_vouch::ranked_model_sub_candidates(&turn.config.codex_home)
                    {
                        if utility_model::provider_for_model_slug(&config, &candidate).is_some() {
                            session
                                .set_auto_model_sub_selection(Some(candidate.clone()))
                                .await;
                            selected_model_sub = Some((
                                candidate,
                                CollabAgentModelSource::ModelSubAuto,
                                CollabAgentModelSourceDetail::ModelSubVouch,
                            ));
                            break;
                        }
                    }
                }
            }
            if selected_model_sub.is_none()
                && requested_model.is_none()
                && !session.get_auto_model_sub_calibration_attempted().await
            {
                session.set_auto_model_sub_calibration_attempted(true).await;
                let calibration_args = serde_json::json!({
                    "items": input_items.clone(),
                });
                let calibration_call_id = format!("{call_id}-auto-calibration");
                let calibration = Box::pin(super::calibrate_model_sub::handle(
                    session.clone(),
                    turn.clone(),
                    calibration_call_id,
                    calibration_args.to_string(),
                ));
                match calibration.await {
                    Ok(ToolOutput::Function {
                        body: FunctionCallOutputBody::Text(content),
                        ..
                    }) => {
                        if let Ok(result) =
                            serde_json::from_str::<AutoCalibrationRawResult>(&content)
                        {
                            let completed_models = result
                                .runs
                                .iter()
                                .filter(|run| matches!(run.status, AgentStatus::Completed(_)))
                                .map(|run| run.model.to_ascii_lowercase())
                                .collect::<std::collections::BTreeSet<_>>();
                            let recommended_model_sub = result
                                .recommended_for_session
                                .clone()
                                .or_else(|| result.recommended_for_latency.clone())
                                .filter(|model_sub| {
                                    completed_models.contains(&model_sub.to_ascii_lowercase())
                                });
                            let runs = result
                                .runs
                                .iter()
                                .map(|run| {
                                    let (status, output_preview) = match &run.status {
                                        AgentStatus::PendingInit => ("pending_init", None),
                                        AgentStatus::Running => ("running", None),
                                        AgentStatus::Completed(message) => {
                                            let preview = message.as_ref().and_then(|value| {
                                                let trimmed = value.trim();
                                                if trimmed.is_empty() {
                                                    None
                                                } else {
                                                    let mut preview = trimmed
                                                        .chars()
                                                        .take(AUTO_CALIBRATION_PREVIEW_CHARS)
                                                        .collect::<String>();
                                                    if trimmed
                                                        .chars()
                                                        .nth(AUTO_CALIBRATION_PREVIEW_CHARS)
                                                        .is_some()
                                                    {
                                                        preview.push('…');
                                                    }
                                                    Some(preview)
                                                }
                                            });
                                            ("completed", preview)
                                        }
                                        AgentStatus::Errored(message) => {
                                            let trimmed = message.trim();
                                            let preview = if trimmed.is_empty() {
                                                None
                                            } else {
                                                let mut preview = trimmed
                                                    .chars()
                                                    .take(AUTO_CALIBRATION_PREVIEW_CHARS)
                                                    .collect::<String>();
                                                if trimmed
                                                    .chars()
                                                    .nth(AUTO_CALIBRATION_PREVIEW_CHARS)
                                                    .is_some()
                                                {
                                                    preview.push('…');
                                                }
                                                Some(preview)
                                            };
                                            ("errored", preview)
                                        }
                                        AgentStatus::Shutdown => ("shutdown", None),
                                        AgentStatus::NotFound => ("not_found", None),
                                    };
                                    AutoCalibrationRunSummary {
                                        model: run.model.clone(),
                                        agent_id: run.agent_id.clone(),
                                        status: status.to_string(),
                                        elapsed_ms: run.elapsed_ms,
                                        output_preview,
                                    }
                                })
                                .collect::<Vec<_>>();
                            auto_calibration = Some(AutoCalibrationResult {
                                task_bucket: result.task_bucket.clone(),
                                runs,
                                recommended_for_vouch: result.recommended_for_vouch.clone(),
                                recommended_for_latency: result.recommended_for_latency.clone(),
                                recommended_for_session: result.recommended_for_session.clone(),
                            });
                            if let Some(model_sub) = recommended_model_sub
                                && utility_model::provider_for_model_slug(&config, &model_sub)
                                    .is_some()
                            {
                                session
                                    .set_auto_model_sub_selection(Some(model_sub.clone()))
                                    .await;
                                selected_model_sub = Some((
                                    model_sub,
                                    CollabAgentModelSource::ModelSubAuto,
                                    CollabAgentModelSourceDetail::AutoCalibration,
                                ));
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, "spawn_agent auto calibration failed");
                    }
                }
            }
            if let Some((model_sub, source, selection_origin)) = selected_model_sub
                && let Some((provider_id, provider)) =
                    utility_model::provider_for_model_slug(&config, &model_sub)
            {
                tracing::info!(
                    model_sub = model_sub.as_str(),
                    selection_origin = ?selection_origin,
                    "spawn_agent selected default child model"
                );
                config.model = Some(model_sub);
                config.model_provider_id = provider_id;
                config.model_provider = provider;
                model_source = Some(source);
                model_source_detail = Some(selection_origin);
            }
        }
        if let Some(requested_model) = requested_model.as_deref() {
            if let Some((provider_id, provider)) =
                utility_model::provider_for_model_slug(&config, requested_model)
            {
                config.model = Some(requested_model.to_string());
                config.model_provider_id = provider_id;
                config.model_provider = provider;
                model_source = Some(CollabAgentModelSource::Explicit);
                model_source_detail = Some(CollabAgentModelSourceDetail::ToolModelOverride);
            } else {
                let err = FunctionCallError::RespondToModel(format!(
                    "Model override `{requested_model}` is unavailable for spawn_agent."
                ));
                session
                    .send_event(
                        &turn,
                        CollabAgentSpawnEndEvent {
                            call_id,
                            sender_thread_id: session.conversation_id,
                            memory: memory.clone(),
                            agent_type: Some(agent_type),
                            model: config.model.clone(),
                            model_provider_id: Some(config.model_provider_id.clone()),
                            model_source,
                            model_source_detail,
                            new_thread_id: None,
                            new_agent_nickname: None,
                            new_agent_role: None,
                            prompt: prompt_for_events,
                            status: AgentStatus::Errored(err.to_string()),
                        }
                        .into(),
                    )
                    .await;
                return Err(err);
            }
        }
        apply_spawn_agent_overrides(&mut config, child_depth);
        model_for_events = config.model.clone();
        model_provider_id_for_events = Some(config.model_provider_id.clone());
        let model_for_result = config
            .model
            .clone()
            .unwrap_or_else(|| turn.model_info.slug.clone());
        let model_provider_id_for_result = config.model_provider_id.clone();

        let mut worktree = None;
        if turn.features.enabled(Feature::AgentWorktrees) {
            match agent_worktree::create_agent_worktree(
                &config.cwd,
                agent_worktree::WorktreePurpose::SpawnedAgent,
            )
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to create isolated agent worktree: {err}"
                ))
            }) {
                Ok(worktree_value) => {
                    worktree = worktree_value;
                    if let Some(worktree) = worktree.as_ref() {
                        config.cwd = worktree.path.clone();
                    }
                }
                Err(err) => {
                    session
                        .send_event(
                            &turn,
                            CollabAgentSpawnEndEvent {
                                call_id,
                                sender_thread_id: session.conversation_id,
                                memory: memory.clone(),
                                agent_type: Some(agent_type),
                                model: model_for_events,
                                model_provider_id: model_provider_id_for_events,
                                model_source,
                                model_source_detail,
                                new_thread_id: None,
                                new_agent_nickname: None,
                                new_agent_role: None,
                                prompt: prompt_for_events,
                                status: AgentStatus::Errored(err.to_string()),
                            }
                            .into(),
                        )
                        .await;
                    return Err(err);
                }
            };
        }

        let prompt_for_agent = build_spawn_agent_prompt_with_context(
            session.as_ref(),
            turn.as_ref(),
            &config,
            &prompt,
            &memory,
            child_depth,
            worktree.as_ref(),
            &agent_type,
        )
        .await;
        if prompt_for_agent != prompt {
            apply_spawn_agent_context_prompt(&mut input_items, prompt_for_agent);
        }

        let result = session
            .services
            .agent_control
            .spawn_agent(
                config,
                input_items,
                Some(thread_spawn_source(
                    session.conversation_id,
                    child_depth,
                    role_name,
                )),
            )
            .await
            .map_err(collab_spawn_error);
        if result.is_err()
            && let Some(worktree) = worktree.as_ref()
        {
            let _ = agent_worktree::remove_agent_worktree(worktree).await;
        }
        let (new_thread_id, status) = match &result {
            Ok(thread_id) => (
                Some(*thread_id),
                session.services.agent_control.get_status(*thread_id).await,
            ),
            Err(err) => (
                None,
                AgentStatus::Errored(match err {
                    FunctionCallError::RespondToModel(message) => message.clone(),
                    FunctionCallError::MissingLocalShellCallId => {
                        "LocalShellCall without call_id or id".to_string()
                    }
                    FunctionCallError::Fatal(message) => format!("Fatal error: {message}"),
                }),
            ),
        };
        let (new_agent_nickname, new_agent_role) = match new_thread_id {
            Some(thread_id) => session
                .services
                .agent_control
                .get_agent_nickname_and_role(thread_id)
                .await
                .unwrap_or((None, None)),
            None => (None, None),
        };
        let nickname = new_agent_nickname.clone();
        session
            .send_event(
                &turn,
                CollabAgentSpawnEndEvent {
                    call_id,
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    agent_type: Some(agent_type.clone()),
                    model: model_for_events,
                    model_provider_id: model_provider_id_for_events,
                    model_source,
                    model_source_detail,
                    new_thread_id,
                    new_agent_nickname,
                    new_agent_role,
                    prompt,
                    status,
                }
                .into(),
            )
            .await;
        let new_thread_id = result?;
        let role_tag = role_name.unwrap_or(DEFAULT_ROLE_NAME);
        turn.otel_manager
            .counter("codex.multi_agent.spawn", 1, &[("role", role_tag)]);

        if let Some(worktree) = worktree.as_ref() {
            let lease = agent_worktree::build_lease(
                &new_thread_id.to_string(),
                Some(session.conversation_id.to_string()),
                worktree,
            );
            let _ = agent_worktree::write_lease(&lease);
        }

        let (memory_scope_version, memory_binding_key) = super::binding_fields_from_memory(&memory);

        let content = serde_json::to_string(&SpawnAgentResult {
            agent_id: new_thread_id.to_string(),
            nickname,
            agent_type,
            model: model_for_result,
            model_provider_id: model_provider_id_for_result,
            model_source: model_source.unwrap_or(CollabAgentModelSource::Parent),
            model_source_detail,
            parent_thread_id: session.conversation_id.to_string(),
            spawn_depth: child_depth,
            worktree_path: worktree
                .as_ref()
                .map(|worktree| worktree.path.display().to_string()),
            branch: worktree.as_ref().map(|worktree| worktree.branch.clone()),
            memory_scope_version,
            memory_binding_key,
            auto_calibration,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize spawn_agent result: {err}"))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    async fn build_spawn_agent_prompt_with_context(
        session: &Session,
        turn: &TurnContext,
        config: &Config,
        prompt: &str,
        memory: &Option<MemoryLink>,
        child_depth: i32,
        worktree: Option<&agent_worktree::AgentWorktree>,
        agent_type: &str,
    ) -> String {
        if prompt_already_has_context_packet(prompt) {
            return prompt.to_string();
        }

        let mut handoff_lines = vec![
            "Swarm handoff (from parent session):".to_string(),
            format!("- Parent thread id: {}", session.conversation_id),
            format!("- Spawn depth: {child_depth}/{MAX_THREAD_SPAWN_DEPTH}"),
        ];
        if let Some(memory_scope_version) = memory
            .as_ref()
            .and_then(|memory_link| memory_link.scope_version.as_deref())
        {
            handoff_lines.push(format!("- Memory scope version: {memory_scope_version}"));
        }
        if let Some(memory_binding_key) = memory
            .as_ref()
            .and_then(|memory_link| memory_link.binding_key.as_deref())
        {
            handoff_lines.push(format!("- Memory binding key: {memory_binding_key}"));
        }
        if let Some(worktree) = worktree {
            handoff_lines.push(format!(
                "- Workspace: isolated worktree at {}",
                worktree.path.display()
            ));
        } else {
            handoff_lines.push(
                "- Workspace: shared with other agents, so do not overwrite or revert unrelated changes."
                    .to_string(),
            );
        }
        handoff_lines.push(
            "- Coordination: stay within the assigned scope and include a concise handoff summary with touched files."
                .to_string(),
        );
        let handoff_block = format!("{}\n\n", handoff_lines.join("\n"));
        let worktree_block = worktree.map(|worktree| {
            format!(
                "Agent worktree:\nPath: {}\nBranch: {}\n\nNote: Commit your changes to the branch above when done.\n\n",
                worktree.path.display(),
                worktree.branch
            )
       });
        let packet_config =
            if config.model_provider.wire_api == crate::model_provider_info::WireApi::Anthropic {
                let mut cfg = context_packet::CLAUDE_CODE_LARGE_CONTEXT_PACKET_CONFIG;

                // Enable entire summary for leader/default agents
                if agent_type == "default" || agent_type == "leader" {
                    cfg.include_entire_summary = true;
                    cfg.max_entire_checkpoints = 5;
                    cfg.max_entire_summary_bytes = 3_000;
                }

                cfg
            } else {
                context_packet::CLAUDE_CODE_CONTEXT_PACKET_CONFIG
            };
        let context = context_packet::build_context_packet(session, turn, packet_config).await;
        if context.trim().is_empty() && worktree_block.is_none() {
            return format!("{}Task:\n{}", handoff_block, prompt.trim());
        }

        format!(
            "{}{}Context packet (from parent session):\n{}\n\nTask:\n{}",
            handoff_block,
            worktree_block.unwrap_or_default(),
            context.trim(),
            prompt.trim()
        )
    }

    fn apply_spawn_agent_context_prompt(
        input_items: &mut Vec<UserInput>,
        prompt_for_agent: String,
    ) {
        if let [
            UserInput::Text {
                text,
                text_elements,
            },
        ] = input_items.as_mut_slice()
            && text_elements.is_empty()
        {
            *text = prompt_for_agent;
            return;
        }

        input_items.insert(
            0,
            UserInput::Text {
                text: prompt_for_agent,
                text_elements: Vec::new(),
            },
        );
    }

    fn prompt_already_has_context_packet(prompt: &str) -> bool {
        let lower = prompt.to_ascii_lowercase();
        lower.contains("context packet")
            || lower.contains("swarm handoff (from parent session):")
            || lower.contains("parent thread id:")
            || lower.contains("working directory:")
            || lower.contains("active memory scope:")
            || lower.contains("active memory binding key:")
            || lower.contains("session summary:")
            || lower.contains("saved thread memory:")
            || lower.contains("recent chat excerpt:")
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde_json::json;

        #[test]
        fn apply_spawn_agent_context_prompt_replaces_single_plain_text_item() {
            let mut items = vec![UserInput::Text {
                text: "original".to_string(),
                text_elements: Vec::new(),
            }];

            apply_spawn_agent_context_prompt(&mut items, "wrapped".to_string());

            assert_eq!(
                items,
                vec![UserInput::Text {
                    text: "wrapped".to_string(),
                    text_elements: Vec::new(),
                }]
            );
        }

        #[test]
        fn apply_spawn_agent_context_prompt_prepends_when_items_are_rich() {
            let mut items = vec![
                UserInput::Text {
                    text: "original".to_string(),
                    text_elements: vec![codex_protocol::user_input::TextElement::new(
                        (0..8).into(),
                        Some("original".to_string()),
                    )],
                },
                UserInput::Mention {
                    name: "drive".to_string(),
                    path: "app://drive".to_string(),
                },
            ];

            apply_spawn_agent_context_prompt(&mut items, "wrapped".to_string());

            assert_eq!(
                items,
                vec![
                    UserInput::Text {
                        text: "wrapped".to_string(),
                        text_elements: Vec::new(),
                    },
                    UserInput::Text {
                        text: "original".to_string(),
                        text_elements: vec![codex_protocol::user_input::TextElement::new(
                            (0..8).into(),
                            Some("original".to_string()),
                        )],
                    },
                    UserInput::Mention {
                        name: "drive".to_string(),
                        path: "app://drive".to_string(),
                    },
                ]
            );
        }

        #[test]
        fn prompt_already_has_context_packet_detects_memory_markers() {
            assert!(prompt_already_has_context_packet(
                "Active memory binding key: cwd:abc:123"
            ));
            assert!(prompt_already_has_context_packet(
                "active memory scope: user"
            ));
            assert!(prompt_already_has_context_packet(
                "Swarm handoff (from parent session):\n- Parent thread id: abc"
            ));
            assert!(!prompt_already_has_context_packet("just do this task"));
        }

        #[test]
        fn spawn_agent_result_serialization_includes_memory_keys_when_available() {
            let result = SpawnAgentResult {
                nickname: None,
                agent_id: "agent-1".to_string(),
                agent_type: "explorer".to_string(),
                model: "gpt-5.3-codex".to_string(),
                model_provider_id: "openai".to_string(),
                model_source: CollabAgentModelSource::Role,
                model_source_detail: Some(CollabAgentModelSourceDetail::RoleConfig),
                parent_thread_id: "parent-1".to_string(),
                spawn_depth: 1,
                worktree_path: Some("/tmp/worktree".to_string()),
                branch: Some("agent/branch".to_string()),
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
                auto_calibration: None,
            };

            let actual =
                serde_json::to_value(result).expect("spawn agent result to serialize to value");
            let expected = json!({
                "agent_id": "agent-1",
                "nickname": null,
                "agent_type": "explorer",
                "model": "gpt-5.3-codex",
                "model_provider_id": "openai",
                "model_source": "role",
                "model_source_detail": "role_config",
                "parent_thread_id": "parent-1",
                "spawn_depth": 1,
                "worktree_path": "/tmp/worktree",
                "branch": "agent/branch",
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            });
            assert_eq!(actual, expected);
        }

        #[test]
        fn spawn_agent_result_serialization_omits_memory_keys_when_unavailable() {
            let result = SpawnAgentResult {
                nickname: None,
                agent_id: "agent-1".to_string(),
                agent_type: "explorer".to_string(),
                model: "gpt-5.3-codex".to_string(),
                model_provider_id: "openai".to_string(),
                model_source: CollabAgentModelSource::Parent,
                model_source_detail: None,
                parent_thread_id: "parent-1".to_string(),
                spawn_depth: 1,
                worktree_path: None,
                branch: None,
                memory_scope_version: None,
                memory_binding_key: None,
                auto_calibration: None,
            };

            let actual =
                serde_json::to_value(result).expect("spawn agent result to serialize to value");
            let expected = json!({
                "agent_id": "agent-1",
                "nickname": null,
                "agent_type": "explorer",
                "model": "gpt-5.3-codex",
                "model_provider_id": "openai",
                "model_source": "parent",
                "parent_thread_id": "parent-1",
                "spawn_depth": 1,
                "worktree_path": null,
                "branch": null
            });
            assert_eq!(actual, expected);
        }
    }
}

mod send_input {
    use super::*;
    use std::sync::Arc;

    #[derive(Debug, Deserialize)]
    struct SendInputArgs {
        id: String,
        message: Option<String>,
        items: Option<Vec<UserInput>>,
        #[serde(default)]
        interrupt: bool,
    }

    #[derive(Debug, Serialize)]
    struct SendInputResult {
        submission_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_binding_key: Option<String>,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: SendInputArgs = parse_arguments(&arguments)?;
        let receiver_thread_id = agent_id(&args.id)?;
        let input_items = parse_collab_input(args.message, args.items)?;
        let prompt = input_preview(&input_items);
        let (receiver_agent_nickname, receiver_agent_role) = session
            .services
            .agent_control
            .get_agent_nickname_and_role(receiver_thread_id)
            .await
            .unwrap_or((None, None));
        if args.interrupt {
            session
                .services
                .agent_control
                .interrupt_agent(receiver_thread_id)
                .await
                .map_err(|err| collab_agent_error(receiver_thread_id, err))?;
        }
        let memory = super::active_memory_link(turn.as_ref()).await;
        session
            .send_event(
                &turn,
                CollabAgentInteractionBeginEvent {
                    call_id: call_id.clone(),
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id,
                    prompt: prompt.clone(),
                }
                .into(),
            )
            .await;
        let result = session
            .services
            .agent_control
            .send_input(receiver_thread_id, input_items)
            .await
            .map_err(|err| collab_agent_error(receiver_thread_id, err));
        let status = session
            .services
            .agent_control
            .get_status(receiver_thread_id)
            .await;
        session
            .send_event(
                &turn,
                CollabAgentInteractionEndEvent {
                    call_id,
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id,
                    receiver_agent_nickname,
                    receiver_agent_role,
                    prompt,
                    status,
                }
                .into(),
            )
            .await;
        let submission_id = result?;
        let (memory_scope_version, memory_binding_key) = super::binding_fields_from_memory(&memory);

        let content = serde_json::to_string(&SendInputResult {
            submission_id,
            memory_scope_version,
            memory_binding_key,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize send_input result: {err}"))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde_json::json;

        #[test]
        fn send_input_result_serialization_includes_memory_keys_when_available() {
            let result = SendInputResult {
                submission_id: "sub-1".to_string(),
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
            };

            let actual =
                serde_json::to_value(result).expect("send input result to serialize to value");
            let expected = json!({
                "submission_id": "sub-1",
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            });
            assert_eq!(actual, expected);
        }

        #[test]
        fn send_input_result_serialization_omits_memory_keys_when_unavailable() {
            let result = SendInputResult {
                submission_id: "sub-1".to_string(),
                memory_scope_version: None,
                memory_binding_key: None,
            };

            let actual =
                serde_json::to_value(result).expect("send input result to serialize to value");
            let expected = json!({
                "submission_id": "sub-1"
            });
            assert_eq!(actual, expected);
        }
    }
}

mod resume_agent {
    use super::*;
    use crate::agent::next_thread_spawn_depth;
    use std::sync::Arc;

    #[derive(Debug, Deserialize)]
    struct ResumeAgentArgs {
        id: String,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
    pub(super) struct ResumeAgentResult {
        pub(super) status: AgentStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(super) memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(super) memory_binding_key: Option<String>,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: ResumeAgentArgs = parse_arguments(&arguments)?;
        let receiver_thread_id = agent_id(&args.id)?;
        let (receiver_agent_nickname, receiver_agent_role) = session
            .services
            .agent_control
            .get_agent_nickname_and_role(receiver_thread_id)
            .await
            .unwrap_or((None, None));
        let child_depth = next_thread_spawn_depth(&turn.session_source);
        let max_depth = turn.config.agent_max_depth;
        if exceeds_thread_spawn_depth_limit(child_depth, max_depth) {
            return Err(FunctionCallError::RespondToModel(
                "Agent depth limit reached. Solve the task yourself.".to_string(),
            ));
        }
        let memory = super::active_memory_link(turn.as_ref()).await;

        session
            .send_event(
                &turn,
                CollabResumeBeginEvent {
                    call_id: call_id.clone(),
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id,
                    receiver_agent_nickname: receiver_agent_nickname.clone(),
                    receiver_agent_role: receiver_agent_role.clone(),
                }
                .into(),
            )
            .await;

        let mut status = session
            .services
            .agent_control
            .get_status(receiver_thread_id)
            .await;
        let error = if matches!(status, AgentStatus::NotFound) {
            // If the thread is no longer active, attempt to restore it from rollout.
            match try_resume_closed_agent(&session, &turn, receiver_thread_id, child_depth).await {
                Ok(resumed_status) => {
                    status = resumed_status;
                    None
                }
                Err(err) => {
                    status = session
                        .services
                        .agent_control
                        .get_status(receiver_thread_id)
                        .await;
                    Some(err)
                }
            }
        } else {
            None
        };

        let (receiver_agent_nickname, receiver_agent_role) = session
            .services
            .agent_control
            .get_agent_nickname_and_role(receiver_thread_id)
            .await
            .unwrap_or((receiver_agent_nickname, receiver_agent_role));
        session
            .send_event(
                &turn,
                CollabResumeEndEvent {
                    call_id,
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id,
                    receiver_agent_nickname,
                    receiver_agent_role,
                    status: status.clone(),
                }
                .into(),
            )
            .await;

        if let Some(err) = error {
            return Err(err);
        }
        turn.otel_manager
            .counter("codex.multi_agent.resume", 1, &[]);

        let (memory_scope_version, memory_binding_key) = super::binding_fields_from_memory(&memory);
        let content = serde_json::to_string(&ResumeAgentResult {
            status,
            memory_scope_version,
            memory_binding_key,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize resume_agent result: {err}"))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    async fn try_resume_closed_agent(
        session: &Arc<Session>,
        turn: &Arc<TurnContext>,
        receiver_thread_id: ThreadId,
        child_depth: i32,
    ) -> Result<AgentStatus, FunctionCallError> {
        let config = build_agent_resume_config(turn.as_ref(), child_depth)?;
        let resumed_thread_id = session
            .services
            .agent_control
            .resume_agent_from_rollout(
                config,
                receiver_thread_id,
                thread_spawn_source(session.conversation_id, child_depth, None),
            )
            .await
            .map_err(|err| collab_agent_error(receiver_thread_id, err))?;

        Ok(session
            .services
            .agent_control
            .get_status(resumed_thread_id)
            .await)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde_json::json;

        #[test]
        fn resume_agent_result_serialization_includes_memory_keys_when_available() {
            let result = ResumeAgentResult {
                status: AgentStatus::Running,
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
            };

            let actual = serde_json::to_value(result).expect("resume result to serialize to value");
            let expected = json!({
                "status": "running",
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            });
            assert_eq!(actual, expected);
        }

        #[test]
        fn resume_agent_result_serialization_omits_memory_keys_when_unavailable() {
            let result = ResumeAgentResult {
                status: AgentStatus::NotFound,
                memory_scope_version: None,
                memory_binding_key: None,
            };

            let actual = serde_json::to_value(result).expect("resume result to serialize to value");
            let expected = json!({
                "status": "not_found"
            });
            assert_eq!(actual, expected);
        }
    }
}

pub(crate) mod wait {
    use super::*;
    use crate::agent::status::is_final;
    use futures::FutureExt;
    use futures::StreamExt;
    use futures::stream::FuturesUnordered;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::watch::Receiver;
    use tokio::time::Instant;

    use tokio::time::timeout_at;

    #[derive(Debug, Deserialize)]
    struct WaitArgs {
        ids: Vec<String>,
        timeout_ms: Option<i64>,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
    pub(crate) struct WaitResult {
        pub(crate) status: HashMap<ThreadId, AgentStatus>,
        pub(crate) timed_out: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(crate) memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(crate) memory_binding_key: Option<String>,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: WaitArgs = parse_arguments(&arguments)?;
        if args.ids.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "ids must be non-empty".to_owned(),
            ));
        }
        let receiver_thread_ids = args
            .ids
            .iter()
            .map(|id| agent_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut receiver_agents = Vec::with_capacity(receiver_thread_ids.len());
        for receiver_thread_id in &receiver_thread_ids {
            let (agent_nickname, agent_role) = session
                .services
                .agent_control
                .get_agent_nickname_and_role(*receiver_thread_id)
                .await
                .unwrap_or((None, None));
            receiver_agents.push(CollabAgentRef {
                thread_id: *receiver_thread_id,
                agent_nickname,
                agent_role,
            });
        }

        // Validate timeout.
        // Very short timeouts encourage busy-polling loops in the orchestrator prompt and can
        // cause high CPU usage even with a single active worker, so clamp to a minimum.
        let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
        let timeout_ms = match timeout_ms {
            ms if ms <= 0 => {
                return Err(FunctionCallError::RespondToModel(
                    "timeout_ms must be greater than zero".to_owned(),
                ));
            }
            ms => ms.clamp(MIN_WAIT_TIMEOUT_MS, MAX_WAIT_TIMEOUT_MS),
        };
        let memory = super::active_memory_link(turn.as_ref()).await;

        session
            .send_event(
                &turn,
                CollabWaitingBeginEvent {
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_ids: receiver_thread_ids.clone(),
                    receiver_agents: receiver_agents.clone(),
                    call_id: call_id.clone(),
                }
                .into(),
            )
            .await;

        let mut status_rxs = Vec::with_capacity(receiver_thread_ids.len());
        let mut initial_final_statuses = Vec::new();
        for id in &receiver_thread_ids {
            match session.services.agent_control.subscribe_status(*id).await {
                Ok(rx) => {
                    let status = rx.borrow().clone();
                    if is_final(&status) {
                        initial_final_statuses.push((*id, status));
                    }
                    status_rxs.push((*id, rx));
                }
                Err(CodexErr::ThreadNotFound(_)) => {
                    initial_final_statuses.push((*id, AgentStatus::NotFound));
                }
                Err(err) => {
                    let mut statuses = HashMap::with_capacity(1);
                    statuses.insert(*id, session.services.agent_control.get_status(*id).await);
                    session
                        .send_event(
                            &turn,
                            CollabWaitingEndEvent {
                                sender_thread_id: session.conversation_id,
                                memory: memory.clone(),
                                call_id: call_id.clone(),
                                agent_statuses: build_wait_agent_statuses(
                                    &statuses,
                                    &receiver_agents,
                                ),
                                statuses,
                            }
                            .into(),
                        )
                        .await;
                    return Err(collab_agent_error(*id, err));
                }
            }
        }

        let statuses = if !initial_final_statuses.is_empty() {
            initial_final_statuses
        } else {
            // Wait for the first agent to reach a final status.
            let mut futures = FuturesUnordered::new();
            for (id, rx) in status_rxs.into_iter() {
                let session = session.clone();
                futures.push(wait_for_final_status(session, id, rx));
            }
            let mut results = Vec::new();
            let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
            loop {
                match timeout_at(deadline, futures.next()).await {
                    Ok(Some(Some(result))) => {
                        results.push(result);
                        break;
                    }
                    Ok(Some(None)) => continue,
                    Ok(None) | Err(_) => break,
                }
            }
            if !results.is_empty() {
                // Drain the unlikely last elements to prevent race.
                loop {
                    match futures.next().now_or_never() {
                        Some(Some(Some(result))) => results.push(result),
                        Some(Some(None)) => continue,
                        Some(None) | None => break,
                    }
                }
            }
            results
        };

        // Convert payload.
        let (memory_scope_version, memory_binding_key) = super::binding_fields_from_memory(&memory);
        let statuses_map = statuses.clone().into_iter().collect::<HashMap<_, _>>();
        let agent_statuses = build_wait_agent_statuses(&statuses_map, &receiver_agents);
        let result = WaitResult {
            status: statuses_map.clone(),
            timed_out: statuses.is_empty(),
            memory_scope_version,
            memory_binding_key,
        };

        // Final event emission.
        session
            .send_event(
                &turn,
                CollabWaitingEndEvent {
                    sender_thread_id: session.conversation_id,
                    memory,
                    call_id,
                    agent_statuses,
                    statuses: statuses_map,
                }
                .into(),
            )
            .await;

        let content = serde_json::to_string(&result).map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize wait result: {err}"))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: None,
        })
    }

    async fn wait_for_final_status(
        session: Arc<Session>,
        thread_id: ThreadId,
        mut status_rx: Receiver<AgentStatus>,
    ) -> Option<(ThreadId, AgentStatus)> {
        let mut status = status_rx.borrow().clone();
        if is_final(&status) {
            return Some((thread_id, status));
        }

        loop {
            if status_rx.changed().await.is_err() {
                let latest = session.services.agent_control.get_status(thread_id).await;
                return is_final(&latest).then_some((thread_id, latest));
            }
            status = status_rx.borrow().clone();
            if is_final(&status) {
                return Some((thread_id, status));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde_json::json;
        use std::collections::HashMap;

        #[test]
        fn wait_result_serialization_includes_memory_keys_when_available() {
            let thread_id =
                ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666").expect("thread id");
            let result = WaitResult {
                status: HashMap::from([(thread_id, AgentStatus::Shutdown)]),
                timed_out: false,
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
            };

            let actual = serde_json::to_value(result).expect("wait result to serialize to value");
            let expected = json!({
                "status": {
                    "b5f6c1c2-1111-2222-3333-444455556666": "shutdown"
                },
                "timed_out": false,
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            });
            assert_eq!(actual, expected);
        }

        #[test]
        fn wait_result_serialization_omits_memory_keys_when_unavailable() {
            let result = WaitResult {
                status: HashMap::new(),
                timed_out: true,
                memory_scope_version: None,
                memory_binding_key: None,
            };

            let actual = serde_json::to_value(result).expect("wait result to serialize to value");
            let expected = json!({
                "status": {},
                "timed_out": true
            });
            assert_eq!(actual, expected);
        }
    }
}

pub mod close_agent {
    use super::*;
    use std::sync::Arc;

    #[derive(Debug, Deserialize, Serialize)]
    pub(super) struct CloseAgentResult {
        pub(super) status: AgentStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(super) memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub(super) memory_binding_key: Option<String>,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: CloseAgentArgs = parse_arguments(&arguments)?;
        let agent_id = agent_id(&args.id)?;
        let (receiver_agent_nickname, receiver_agent_role) = session
            .services
            .agent_control
            .get_agent_nickname_and_role(agent_id)
            .await
            .unwrap_or((None, None));
        let memory = super::active_memory_link(turn.as_ref()).await;
        session
            .send_event(
                &turn,
                CollabCloseBeginEvent {
                    call_id: call_id.clone(),
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id: agent_id,
                }
                .into(),
            )
            .await;
        let status = match session
            .services
            .agent_control
            .subscribe_status(agent_id)
            .await
        {
            Ok(mut status_rx) => status_rx.borrow_and_update().clone(),
            Err(err) => {
                let status = session.services.agent_control.get_status(agent_id).await;
                session
                    .send_event(
                        &turn,
                        CollabCloseEndEvent {
                            call_id: call_id.clone(),
                            sender_thread_id: session.conversation_id,
                            memory: memory.clone(),
                            receiver_thread_id: agent_id,
                            receiver_agent_nickname: receiver_agent_nickname.clone(),
                            receiver_agent_role: receiver_agent_role.clone(),
                            status,
                        }
                        .into(),
                    )
                    .await;
                return Err(collab_agent_error(agent_id, err));
            }
        };
        let result = if !matches!(status, AgentStatus::Shutdown) {
            session
                .services
                .agent_control
                .shutdown_agent(agent_id)
                .await
                .map_err(|err| collab_agent_error(agent_id, err))
                .map(|_| ())
        } else {
            Ok(())
        };
        session
            .send_event(
                &turn,
                CollabCloseEndEvent {
                    call_id,
                    sender_thread_id: session.conversation_id,
                    memory: memory.clone(),
                    receiver_thread_id: agent_id,
                    receiver_agent_nickname,
                    receiver_agent_role,
                    status: status.clone(),
                }
                .into(),
            )
            .await;
        result?;

        let (memory_scope_version, memory_binding_key) = super::binding_fields_from_memory(&memory);
        let content = serde_json::to_string(&CloseAgentResult {
            status,
            memory_scope_version,
            memory_binding_key,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize close_agent result: {err}"))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use serde_json::json;

        #[test]
        fn close_agent_result_serialization_includes_memory_keys_when_available() {
            let result = CloseAgentResult {
                status: AgentStatus::Shutdown,
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
            };

            let actual = serde_json::to_value(result).expect("close result to serialize to value");
            let expected = json!({
                "status": "shutdown",
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            });
            assert_eq!(actual, expected);
        }

        #[test]
        fn close_agent_result_serialization_omits_memory_keys_when_unavailable() {
            let result = CloseAgentResult {
                status: AgentStatus::NotFound,
                memory_scope_version: None,
                memory_binding_key: None,
            };

            let actual = serde_json::to_value(result).expect("close result to serialize to value");
            let expected = json!({
                "status": "not_found"
            });
            assert_eq!(actual, expected);
        }
    }
}

mod calibrate_model_sub {
    use super::*;
    use crate::agent::status::is_final;
    use crate::utility_model;
    use codex_protocol::ThreadId;
    use futures::future;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::Instant;

    const DEFAULT_CALIBRATION_CANDIDATES: [&str; 4] = [
        "claude-sonnet-4-6",
        "gpt-5.2-codex",
        "gpt-5.3-codex",
        "claude-opus-4-6",
    ];
    const DEFAULT_WAIT_TIMEOUT_MS: i64 = 1_500;
    const MIN_WAIT_TIMEOUT_MS: i64 = 100;
    const MAX_WAIT_TIMEOUT_MS: i64 = 30_000;

    #[derive(Debug, Deserialize)]
    struct CalibrateModelSubArgs {
        message: Option<String>,
        items: Option<Vec<UserInput>>,
        candidates: Option<Vec<String>>,
        task_bucket: Option<String>,
        wait_timeout_ms: Option<i64>,
    }

    #[derive(Debug, Deserialize)]
    struct SpawnAgentResult {
        agent_id: String,
    }

    #[derive(Debug, Serialize)]
    struct CalibrationRun {
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        status: AgentStatus,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct CalibrateModelSubResult {
        task_bucket: Option<String>,
        runs: Vec<CalibrationRun>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_vouch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_latency: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommended_for_session: Option<String>,
        next_step: String,
    }

    struct ActiveRun {
        model: String,
        agent_id: ThreadId,
        agent_id_string: String,
        started_at: Instant,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: CalibrateModelSubArgs = parse_arguments(&arguments)?;
        let items = parse_collab_input(args.message, args.items)?;
        let task_bucket = normalize_task_bucket(args.task_bucket)?;
        let wait_timeout_ms = args
            .wait_timeout_ms
            .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS)
            .clamp(MIN_WAIT_TIMEOUT_MS, MAX_WAIT_TIMEOUT_MS);
        let candidates = normalized_candidates(args.candidates, turn.config.model_sub.as_deref());
        let mut filtered = Vec::new();
        for candidate in candidates {
            if utility_model::provider_for_model_slug(&turn.config, &candidate).is_some() {
                filtered.push(candidate);
            }
        }
        if filtered.len() < 2 {
            return Err(FunctionCallError::RespondToModel(
                "Need at least two available candidate models for calibration.".to_string(),
            ));
        }
        session
            .set_last_model_sub_calibration_models(filtered.clone())
            .await;

        let mut runs = Vec::new();
        let mut active_runs = Vec::new();
        for (index, candidate) in filtered.into_iter().enumerate() {
            let spawn_call_id = format!("{call_id}-spawn-{index}");
            let spawn_args = build_spawn_arguments(&items, &candidate);
            match super::spawn::handle(
                session.clone(),
                turn.clone(),
                spawn_call_id,
                spawn_args.to_string(),
            )
            .await
            {
                Ok(ToolOutput::Function {
                    body: FunctionCallOutputBody::Text(content),
                    ..
                }) => match serde_json::from_str::<SpawnAgentResult>(&content) {
                    Ok(parsed) => match ThreadId::from_string(&parsed.agent_id) {
                        Ok(agent_id) => {
                            active_runs.push(ActiveRun {
                                model: candidate,
                                agent_id,
                                agent_id_string: parsed.agent_id,
                                started_at: Instant::now(),
                            });
                        }
                        Err(err) => {
                            runs.push(CalibrationRun {
                                model: candidate,
                                agent_id: None,
                                status: AgentStatus::Errored(format!(
                                    "invalid spawned agent id: {err:?}"
                                )),
                                elapsed_ms: 0,
                                error: Some("spawn returned invalid agent id".to_string()),
                            });
                        }
                    },
                    Err(err) => {
                        runs.push(CalibrationRun {
                            model: candidate,
                            agent_id: None,
                            status: AgentStatus::Errored(format!(
                                "failed to parse spawn output: {err}"
                            )),
                            elapsed_ms: 0,
                            error: Some("spawn output parsing failed".to_string()),
                        });
                    }
                },
                Ok(_) => {
                    runs.push(CalibrationRun {
                        model: candidate,
                        agent_id: None,
                        status: AgentStatus::Errored(
                            "spawn returned non-text tool output".to_string(),
                        ),
                        elapsed_ms: 0,
                        error: Some("spawn returned non-text tool output".to_string()),
                    });
                }
                Err(err) => {
                    runs.push(CalibrationRun {
                        model: candidate,
                        agent_id: None,
                        status: AgentStatus::Errored(err.to_string()),
                        elapsed_ms: 0,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        let waited = future::join_all(active_runs.into_iter().map(|run| {
            let session = session.clone();
            async move {
                let mut status =
                    wait_for_final_status(session.clone(), run.agent_id, wait_timeout_ms).await;
                if !is_final(&status) {
                    let _ = session
                        .services
                        .agent_control
                        .shutdown_agent(run.agent_id)
                        .await;
                    status = session
                        .services
                        .agent_control
                        .get_status(run.agent_id)
                        .await;
                }
                let elapsed_ms = run.started_at.elapsed().as_millis();
                let elapsed_ms = elapsed_ms.min(u128::from(u64::MAX)) as u64;
                CalibrationRun {
                    model: run.model,
                    agent_id: Some(run.agent_id_string),
                    status,
                    elapsed_ms,
                    error: None,
                }
            }
        }))
        .await;
        runs.extend(waited);

        let available_models = runs
            .iter()
            .filter(|run| run.agent_id.is_some())
            .map(|run| run.model.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        let recommended_for_vouch =
            crate::model_sub_vouch::ranked_model_sub_candidates(&turn.config.codex_home)
                .into_iter()
                .find(|candidate| available_models.contains(&candidate.to_ascii_lowercase()));
        let recommended_for_latency = runs
            .iter()
            .filter(|run| matches!(run.status, AgentStatus::Completed(_)))
            .min_by_key(|run| run.elapsed_ms)
            .map(|run| run.model.clone());
        let recommended_for_session = recommended_for_vouch
            .clone()
            .or_else(|| recommended_for_latency.clone());
        let recommended_for_session_cache = recommended_for_session.clone().filter(|model| {
            runs.iter().any(|run| {
                run.model.eq_ignore_ascii_case(model)
                    && matches!(run.status, AgentStatus::Completed(_))
            })
        });
        session
            .set_last_model_sub_calibration_recommended_for_session(recommended_for_session_cache)
            .await;
        let next_step =
            "Compare completed outputs, then call `record_model_sub_winner` (or `record_model_sub_duel`) to persist the winner; `record_model_sub_winner` can omit winner/candidates to reuse this round's cached recommendation."
                .to_string();

        let content = serde_json::to_string(&CalibrateModelSubResult {
            task_bucket,
            runs,
            recommended_for_vouch,
            recommended_for_latency,
            recommended_for_session,
            next_step,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize calibrate_model_sub result: {err}"
            ))
        })?;
        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    fn normalize_task_bucket(
        task_bucket: Option<String>,
    ) -> Result<Option<String>, FunctionCallError> {
        let Some(task_bucket) = task_bucket else {
            return Ok(None);
        };
        let normalized = task_bucket.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(None);
        }
        if normalized == "general" || normalized == "debug" || normalized == "review" {
            Ok(Some(normalized))
        } else {
            Err(FunctionCallError::RespondToModel(
                "task_bucket must be one of: general, debug, review".to_string(),
            ))
        }
    }

    fn normalized_candidates(
        candidates: Option<Vec<String>>,
        model_sub: Option<&str>,
    ) -> Vec<String> {
        let mut ordered = Vec::new();
        if let Some(model_sub) = model_sub {
            ordered.push(model_sub.to_string());
        }
        if let Some(candidates) = candidates {
            ordered.extend(candidates);
        } else {
            ordered.extend(
                DEFAULT_CALIBRATION_CANDIDATES
                    .into_iter()
                    .map(ToOwned::to_owned),
            );
        }
        let mut seen = BTreeSet::new();
        let mut normalized = Vec::new();
        for candidate in ordered {
            let value = candidate.trim();
            if value.is_empty() {
                continue;
            }
            let key = value.to_ascii_lowercase();
            if seen.insert(key) {
                normalized.push(value.to_string());
            }
        }
        normalized
    }

    fn build_spawn_arguments(items: &[UserInput], model: &str) -> Value {
        serde_json::json!({
            "items": items,
            "model": model,
        })
    }

    async fn wait_for_final_status(
        session: Arc<Session>,
        agent_id: ThreadId,
        timeout_ms: i64,
    ) -> AgentStatus {
        let mut status_rx = match session
            .services
            .agent_control
            .subscribe_status(agent_id)
            .await
        {
            Ok(status_rx) => status_rx,
            Err(_) => return AgentStatus::NotFound,
        };
        let current = status_rx.borrow().clone();
        if is_final(&current) {
            return current;
        }
        let wait_future = async {
            loop {
                if status_rx.changed().await.is_err() {
                    return session.services.agent_control.get_status(agent_id).await;
                }
                let status = status_rx.borrow().clone();
                if is_final(&status) {
                    return status;
                }
            }
        };
        match tokio::time::timeout(Duration::from_millis(timeout_ms as u64), wait_future).await {
            Ok(status) => status,
            Err(_) => session.services.agent_control.get_status(agent_id).await,
        }
    }
}

mod record_model_sub_duel {
    use super::*;
    use crate::model_sub_vouch::ModelSubVouchVerdict;
    use std::sync::Arc;

    #[derive(Debug, Deserialize)]
    struct RecordModelSubDuelArgs {
        winner_model: String,
        loser_model: String,
        task_bucket: Option<String>,
        note: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct RecordModelSubDuelResult {
        winner_model: String,
        loser_model: String,
        task_bucket: Option<String>,
        winner_wins: u32,
        winner_losses: u32,
        loser_wins: u32,
        loser_losses: u32,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        _call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: RecordModelSubDuelArgs = parse_arguments(&arguments)?;
        let winner_model = args.winner_model.trim();
        let loser_model = args.loser_model.trim();
        if winner_model.is_empty() || loser_model.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "winner_model and loser_model must be non-empty".to_string(),
            ));
        }
        if winner_model.eq_ignore_ascii_case(loser_model) {
            return Err(FunctionCallError::RespondToModel(
                "winner_model and loser_model must be different".to_string(),
            ));
        }

        let task_bucket = normalize_task_bucket(args.task_bucket)?;
        let winner_stats = crate::model_sub_vouch::record_model_sub_vouch(
            &turn.config.codex_home,
            winner_model,
            ModelSubVouchVerdict::Win,
            task_bucket.as_deref(),
            args.note.as_deref(),
        )
        .map_err(FunctionCallError::RespondToModel)?;
        let loser_stats = crate::model_sub_vouch::record_model_sub_vouch(
            &turn.config.codex_home,
            loser_model,
            ModelSubVouchVerdict::Loss,
            task_bucket.as_deref(),
            args.note.as_deref(),
        )
        .map_err(FunctionCallError::RespondToModel)?;
        session
            .set_auto_model_sub_selection(Some(winner_model.to_string()))
            .await;

        let content = serde_json::to_string(&RecordModelSubDuelResult {
            winner_model: winner_model.to_string(),
            loser_model: loser_model.to_string(),
            task_bucket,
            winner_wins: winner_stats.wins,
            winner_losses: winner_stats.losses,
            loser_wins: loser_stats.wins,
            loser_losses: loser_stats.losses,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize record_model_sub_duel result: {err}"
            ))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    fn normalize_task_bucket(
        task_bucket: Option<String>,
    ) -> Result<Option<String>, FunctionCallError> {
        let Some(task_bucket) = task_bucket else {
            return Ok(None);
        };
        let normalized = task_bucket.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(None);
        }
        if normalized == "general" || normalized == "debug" || normalized == "review" {
            Ok(Some(normalized))
        } else {
            Err(FunctionCallError::RespondToModel(
                "task_bucket must be one of: general, debug, review".to_string(),
            ))
        }
    }
}

mod record_model_sub_winner {
    use super::*;
    use crate::model_sub_vouch::ModelSubVouchVerdict;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    #[derive(Debug, Deserialize)]
    struct RecordModelSubWinnerArgs {
        winner_model: Option<String>,
        compared_models: Option<Vec<String>>,
        task_bucket: Option<String>,
        note: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct RecordModelSubWinnerResult {
        winner_model: String,
        winner_model_source: String,
        compared_models_source: String,
        losers_recorded: Vec<String>,
        task_bucket: Option<String>,
        winner_wins: u32,
        winner_losses: u32,
    }

    pub async fn handle(
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        _call_id: String,
        arguments: String,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args: RecordModelSubWinnerArgs = parse_arguments(&arguments)?;
        let RecordModelSubWinnerArgs {
            winner_model,
            compared_models,
            task_bucket,
            note,
        } = args;
        let (winner_model, winner_model_source) = match winner_model {
            Some(winner_model) => {
                let winner_model = winner_model.trim();
                if winner_model.is_empty() {
                    return Err(FunctionCallError::RespondToModel(
                        "winner_model must be non-empty when provided".to_string(),
                    ));
                }
                (winner_model.to_string(), "provided".to_string())
            }
            None => {
                let Some(winner_model) = session
                    .get_last_model_sub_calibration_recommended_for_session()
                    .await
                else {
                    return Err(FunctionCallError::RespondToModel(
                        "winner_model is required unless calibrate_model_sub has already run in this session.".to_string(),
                    ));
                };
                (winner_model, "session_last_calibration".to_string())
            }
        };

        let task_bucket = normalize_task_bucket(task_bucket)?;
        let (compared_models, compared_models_source) =
            if let Some(compared_models) = compared_models {
                (compared_models, "provided".to_string())
            } else {
                (
                    session.get_last_model_sub_calibration_models().await,
                    "session_last_calibration".to_string(),
                )
            };
        let losers = normalize_losers(winner_model.as_str(), compared_models);
        if losers.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "compared_models must include at least one model different from winner_model; if omitted, run calibrate_model_sub first in this session."
                    .to_string(),
            ));
        }

        let mut winner_stats = crate::model_sub_vouch::ModelSubVouchStats { wins: 0, losses: 0 };
        for loser in &losers {
            winner_stats = crate::model_sub_vouch::record_model_sub_vouch(
                &turn.config.codex_home,
                winner_model.as_str(),
                ModelSubVouchVerdict::Win,
                task_bucket.as_deref(),
                note.as_deref(),
            )
            .map_err(FunctionCallError::RespondToModel)?;
            crate::model_sub_vouch::record_model_sub_vouch(
                &turn.config.codex_home,
                loser,
                ModelSubVouchVerdict::Loss,
                task_bucket.as_deref(),
                note.as_deref(),
            )
            .map_err(FunctionCallError::RespondToModel)?;
        }
        session
            .set_auto_model_sub_selection(Some(winner_model.to_string()))
            .await;

        let content = serde_json::to_string(&RecordModelSubWinnerResult {
            winner_model: winner_model.clone(),
            winner_model_source,
            compared_models_source,
            losers_recorded: losers,
            task_bucket,
            winner_wins: winner_stats.wins,
            winner_losses: winner_stats.losses,
        })
        .map_err(|err| {
            FunctionCallError::Fatal(format!(
                "failed to serialize record_model_sub_winner result: {err}"
            ))
        })?;

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }

    fn normalize_task_bucket(
        task_bucket: Option<String>,
    ) -> Result<Option<String>, FunctionCallError> {
        let Some(task_bucket) = task_bucket else {
            return Ok(None);
        };
        let normalized = task_bucket.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(None);
        }
        if normalized == "general" || normalized == "debug" || normalized == "review" {
            Ok(Some(normalized))
        } else {
            Err(FunctionCallError::RespondToModel(
                "task_bucket must be one of: general, debug, review".to_string(),
            ))
        }
    }

    fn normalize_losers(winner_model: &str, compared_models: Vec<String>) -> Vec<String> {
        let winner_key = winner_model.to_ascii_lowercase();
        let mut seen = BTreeSet::new();
        let mut losers = Vec::new();
        for compared_model in compared_models {
            let model = compared_model.trim();
            if model.is_empty() {
                continue;
            }
            let key = model.to_ascii_lowercase();
            if key == winner_key || !seen.insert(key) {
                continue;
            }
            losers.push(model.to_string());
        }
        losers
    }
}

fn agent_id(id: &str) -> Result<ThreadId, FunctionCallError> {
    ThreadId::from_string(id)
        .map_err(|e| FunctionCallError::RespondToModel(format!("invalid agent id {id}: {e:?}")))
}

fn build_wait_agent_statuses(
    statuses: &HashMap<ThreadId, AgentStatus>,
    receiver_agents: &[CollabAgentRef],
) -> Vec<CollabAgentStatusEntry> {
    if statuses.is_empty() {
        return Vec::new();
    }

    let mut entries = Vec::with_capacity(statuses.len());
    let mut seen = HashMap::with_capacity(receiver_agents.len());
    for receiver_agent in receiver_agents {
        seen.insert(receiver_agent.thread_id, ());
        if let Some(status) = statuses.get(&receiver_agent.thread_id) {
            entries.push(CollabAgentStatusEntry {
                thread_id: receiver_agent.thread_id,
                agent_nickname: receiver_agent.agent_nickname.clone(),
                agent_role: receiver_agent.agent_role.clone(),
                status: status.clone(),
            });
        }
    }

    let mut extras = statuses
        .iter()
        .filter(|(thread_id, _)| !seen.contains_key(thread_id))
        .map(|(thread_id, status)| CollabAgentStatusEntry {
            thread_id: *thread_id,
            agent_nickname: None,
            agent_role: None,
            status: status.clone(),
        })
        .collect::<Vec<_>>();
    extras.sort_by(|left, right| left.thread_id.to_string().cmp(&right.thread_id.to_string()));
    entries.extend(extras);
    entries
}

fn collab_spawn_error(err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::UnsupportedOperation(_) => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        err => FunctionCallError::RespondToModel(format!("collab spawn failed: {err}")),
    }
}

fn collab_agent_error(agent_id: ThreadId, err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::ThreadNotFound(id) => {
            FunctionCallError::RespondToModel(format!("agent with id {id} not found"))
        }
        CodexErr::InternalAgentDied => {
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} is closed"))
        }
        CodexErr::UnsupportedOperation(_) => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        err => FunctionCallError::RespondToModel(format!("collab tool failed: {err}")),
    }
}

fn thread_spawn_source(
    parent_thread_id: ThreadId,
    depth: i32,
    agent_role: Option<&str>,
) -> SessionSource {
    SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id,
        depth,
        agent_nickname: None,
        agent_role: agent_role.map(str::to_string),
    })
}

fn parse_collab_input(
    message: Option<String>,
    items: Option<Vec<UserInput>>,
) -> Result<Vec<UserInput>, FunctionCallError> {
    match (message, items) {
        (Some(_), Some(_)) => Err(FunctionCallError::RespondToModel(
            "Provide either message or items, but not both".to_string(),
        )),
        (None, None) => Err(FunctionCallError::RespondToModel(
            "Provide one of: message or items".to_string(),
        )),
        (Some(message), None) => {
            if message.trim().is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Empty message can't be sent to an agent".to_string(),
                ));
            }
            Ok(vec![UserInput::Text {
                text: message,
                text_elements: Vec::new(),
            }])
        }
        (None, Some(items)) => {
            if items.is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Items can't be empty".to_string(),
                ));
            }
            Ok(items)
        }
    }
}

fn input_preview(items: &[UserInput]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|item| match item {
            UserInput::Text { text, .. } => text.clone(),
            UserInput::Image { .. } => "[image]".to_string(),
            UserInput::LocalImage { path } => format!("[local_image:{}]", path.display()),
            UserInput::Skill { name, path } => {
                format!("[skill:${name}]({})", path.display())
            }
            UserInput::Mention { name, path } => format!("[mention:${name}]({path})"),
            _ => "[input]".to_string(),
        })
        .collect();

    parts.join("\n")
}

pub(crate) fn build_agent_spawn_config(
    base_instructions: &BaseInstructions,
    turn: &TurnContext,
) -> Result<Config, FunctionCallError> {
    let mut config = build_agent_shared_config(turn)?;
    config.base_instructions = Some(base_instructions.text.clone());
    Ok(config)
}

fn build_agent_resume_config(
    turn: &TurnContext,
    child_depth: i32,
) -> Result<Config, FunctionCallError> {
    let mut config = build_agent_shared_config(turn)?;
    apply_spawn_agent_overrides(&mut config, child_depth);
    // For resume, keep base instructions sourced from rollout/session metadata.
    config.base_instructions = None;
    Ok(config)
}

async fn active_memory_link(turn: &TurnContext) -> Option<MemoryLink> {
    let memory_context = turn.resolve_hook_memory_context().await;
    let scope_version = memory_context
        .as_ref()
        .and_then(|memory_context| memory_context.active_memory_scope_version.clone());
    let scope_kind = memory_context
        .as_ref()
        .and_then(|memory_context| memory_context.active_scope_kind.clone());
    let summary_sha256 = memory_context
        .as_ref()
        .and_then(|memory_context| memory_context.active_memory_summary_sha256.clone());
    let binding_key = memory_context
        .as_ref()
        .and_then(|memory_context| memory_context.active_memory_binding_key.clone());
    if scope_version.is_none()
        && scope_kind.is_none()
        && summary_sha256.is_none()
        && binding_key.is_none()
    {
        None
    } else {
        Some(MemoryLink {
            scope_version,
            scope_kind,
            summary_sha256,
            binding_key,
        })
    }
}

fn binding_fields_from_memory(memory: &Option<MemoryLink>) -> (Option<String>, Option<String>) {
    let memory_scope_version = memory
        .as_ref()
        .and_then(|memory| memory.scope_version.clone());
    let memory_binding_key = memory
        .as_ref()
        .and_then(|memory| memory.binding_key.clone());
    (memory_scope_version, memory_binding_key)
}

fn build_agent_shared_config(turn: &TurnContext) -> Result<Config, FunctionCallError> {
    let base_config = turn.config.clone();
    let mut config = (*base_config).clone();
    config.model = Some(turn.model_info.slug.clone());
    config.model_reasoning_effort = turn.reasoning_effort;
    config.model_reasoning_summary = turn.reasoning_summary;
    config.developer_instructions = turn.developer_instructions.clone();
    config.compact_prompt = turn.compact_prompt.clone();
    apply_spawn_agent_runtime_overrides(&mut config, turn)?;

    Ok(config)
}

fn apply_spawn_agent_runtime_overrides(
    config: &mut Config,
    turn: &TurnContext,
) -> Result<(), FunctionCallError> {
    config
        .permissions
        .approval_policy
        .set(turn.approval_policy.value())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("approval_policy is invalid: {err}"))
        })?;
    config.permissions.shell_environment_policy = turn.shell_environment_policy.clone();
    config.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
    config.cwd = turn.cwd.clone();
    config
        .permissions
        .sandbox_policy
        .set(turn.sandbox_policy.get().clone())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("sandbox_policy is invalid: {err}"))
        })?;
    Ok(())
}

fn apply_spawn_agent_overrides(config: &mut Config, child_depth: i32) {
    if child_depth >= config.agent_max_depth {
        config.features.disable(Feature::Collab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthManager;
    use crate::CodexAuth;
    use crate::ThreadManager;
    use crate::built_in_model_providers;
    use crate::codex::make_session_and_context;
    use crate::config::DEFAULT_AGENT_MAX_DEPTH;
    use crate::config::types::ShellEnvironmentPolicy;
    use crate::function_tool::FunctionCallError;
    use crate::protocol::AskForApproval;
    use crate::protocol::Op;
    use crate::protocol::SandboxPolicy;
    use crate::protocol::SessionSource;
    use crate::protocol::SubAgentSource;
    use crate::turn_diff_tracker::TurnDiffTracker;
    use codex_protocol::ThreadId;
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::protocol::InitialHistory;
    use codex_protocol::protocol::RolloutItem;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    fn invocation(
        session: Arc<crate::codex::Session>,
        turn: Arc<TurnContext>,
        tool_name: &str,
        payload: ToolPayload,
    ) -> ToolInvocation {
        ToolInvocation {
            session,
            turn,
            tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
            call_id: "call-1".to_string(),
            tool_name: tool_name.to_string(),
            payload,
        }
    }

    fn function_payload(args: serde_json::Value) -> ToolPayload {
        ToolPayload::Function {
            arguments: args.to_string(),
        }
    }

    fn thread_manager() -> ThreadManager {
        ThreadManager::with_models_provider_for_tests(
            CodexAuth::from_api_key("dummy"),
            built_in_model_providers()["openai"].clone(),
        )
    }

    fn write_model_sub_vouch(codex_home: &std::path::Path, content: &str) {
        let memories_dir = codex_home.join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(memories_dir.join("model_sub_vouch.json"), content).expect("write vouch file");
    }

    #[tokio::test]
    async fn handler_rejects_non_function_payloads() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            ToolPayload::Custom {
                input: "hello".to_string(),
            },
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("payload should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "collab handler received unsupported payload".to_string()
            )
        );
    }

    #[tokio::test]
    async fn handler_rejects_unknown_tool() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "unknown_tool",
            function_payload(json!({})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("tool should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel("unsupported collab tool unknown_tool".to_string())
        );
    }

    #[tokio::test]
    async fn record_model_sub_duel_rejects_identical_models() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "record_model_sub_duel",
            function_payload(json!({
                "winner_model": "claude-sonnet-4-6",
                "loser_model": "CLAUDE-SONNET-4-6"
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("duel should fail for identical models");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "winner_model and loser_model must be different".to_string()
            )
        );
    }

    #[tokio::test]
    async fn record_model_sub_duel_writes_vouch_ledger() {
        #[derive(Debug, Deserialize)]
        struct DuelResult {
            winner_model: String,
            loser_model: String,
            task_bucket: Option<String>,
            winner_wins: u32,
            winner_losses: u32,
            loser_wins: u32,
            loser_losses: u32,
        }

        let (session, turn) = make_session_and_context().await;
        let session = Arc::new(session);
        let codex_home = turn.config.codex_home.clone();
        let invocation = invocation(
            session.clone(),
            Arc::new(turn),
            "record_model_sub_duel",
            function_payload(json!({
                "winner_model": "claude-sonnet-4-6",
                "loser_model": "gpt-5.2-codex",
                "task_bucket": "debug",
                "note": "better root cause"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("duel should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: DuelResult = serde_json::from_str(&content).expect("duel result should parse");
        assert_eq!(result.winner_model, "claude-sonnet-4-6");
        assert_eq!(result.loser_model, "gpt-5.2-codex");
        assert_eq!(result.task_bucket, Some("debug".to_string()));
        assert_eq!(result.winner_wins, 1);
        assert_eq!(result.winner_losses, 0);
        assert_eq!(result.loser_wins, 0);
        assert_eq!(result.loser_losses, 1);

        let ledger_raw = fs::read_to_string(codex_home.join("memories/model_sub_vouch.json"))
            .expect("model-sub vouch file should exist");
        let ledger: serde_json::Value =
            serde_json::from_str(&ledger_raw).expect("ledger should parse");
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["wins"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            ledger["models"]["gpt-5.2-codex"]["losses"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            session.get_auto_model_sub_selection().await,
            Some("claude-sonnet-4-6".to_string())
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_records_all_losers_and_updates_cache() {
        #[derive(Debug, Deserialize)]
        struct WinnerResult {
            winner_model: String,
            winner_model_source: String,
            compared_models_source: String,
            losers_recorded: Vec<String>,
            winner_wins: u32,
            winner_losses: u32,
        }

        let (session, turn) = make_session_and_context().await;
        let session = Arc::new(session);
        let codex_home = turn.config.codex_home.clone();
        let invocation = invocation(
            session.clone(),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({
                "winner_model": "claude-sonnet-4-6",
                "compared_models": ["gpt-5.2-codex", "gpt-5.1-codex-mini", "claude-sonnet-4-6"],
                "task_bucket": "review",
                "note": "best review quality"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("winner record should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: WinnerResult =
            serde_json::from_str(&content).expect("winner result should parse");
        assert_eq!(result.winner_model, "claude-sonnet-4-6");
        assert_eq!(result.winner_model_source, "provided");
        assert_eq!(result.compared_models_source, "provided");
        assert_eq!(result.losers_recorded.len(), 2);
        assert_eq!(result.winner_wins, 2);
        assert_eq!(result.winner_losses, 0);

        let ledger_raw = fs::read_to_string(codex_home.join("memories/model_sub_vouch.json"))
            .expect("model-sub vouch file should exist");
        let ledger: serde_json::Value =
            serde_json::from_str(&ledger_raw).expect("ledger should parse");
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["wins"],
            serde_json::Value::from(2)
        );
        assert_eq!(
            ledger["models"]["gpt-5.2-codex"]["losses"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            ledger["models"]["gpt-5.1-codex-mini"]["losses"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            session.get_auto_model_sub_selection().await,
            Some("claude-sonnet-4-6".to_string())
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_uses_session_calibration_models_when_compared_omitted() {
        #[derive(Debug, Deserialize)]
        struct WinnerResult {
            winner_model: String,
            winner_model_source: String,
            compared_models_source: String,
            losers_recorded: Vec<String>,
        }

        let (session, turn) = make_session_and_context().await;
        let session = Arc::new(session);
        let codex_home = turn.config.codex_home.clone();
        session
            .set_last_model_sub_calibration_models(vec![
                "claude-sonnet-4-6".to_string(),
                "gpt-5.2-codex".to_string(),
                "gpt-5.1-codex-mini".to_string(),
            ])
            .await;
        session
            .set_last_model_sub_calibration_recommended_for_session(Some(
                "claude-sonnet-4-6".to_string(),
            ))
            .await;
        let invocation = invocation(
            session.clone(),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({
                "task_bucket": "general",
                "note": "best overall output"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("winner record should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: WinnerResult =
            serde_json::from_str(&content).expect("winner result should parse");
        assert_eq!(result.winner_model, "claude-sonnet-4-6");
        assert_eq!(result.winner_model_source, "session_last_calibration");
        assert_eq!(result.compared_models_source, "session_last_calibration");
        assert_eq!(result.losers_recorded.len(), 2);

        let ledger_raw = fs::read_to_string(codex_home.join("memories/model_sub_vouch.json"))
            .expect("model-sub vouch file should exist");
        let ledger: serde_json::Value =
            serde_json::from_str(&ledger_raw).expect("ledger should parse");
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["wins"],
            serde_json::Value::from(2)
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_rejects_without_distinct_losers() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({
                "winner_model": "claude-sonnet-4-6",
                "compared_models": ["claude-sonnet-4-6"]
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("winner record should fail");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "compared_models must include at least one model different from winner_model; if omitted, run calibrate_model_sub first in this session."
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_rejects_when_omitted_and_no_cached_calibration() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("winner record should fail without a winner model fallback");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "winner_model is required unless calibrate_model_sub has already run in this session."
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_rejects_when_winner_is_empty() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({
                "winner_model": "   "
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("winner record should fail for empty winner");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "winner_model must be non-empty when provided".to_string()
            )
        );
    }

    #[tokio::test]
    async fn record_model_sub_winner_rejects_when_omitted_compared_models_without_cache() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "record_model_sub_winner",
            function_payload(json!({
                "winner_model": "claude-sonnet-4-6"
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("winner record should fail without compared models cache");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "compared_models must include at least one model different from winner_model; if omitted, run calibrate_model_sub first in this session."
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn calibrate_model_sub_rejects_when_less_than_two_candidates() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "calibrate_model_sub",
            function_payload(json!({
                "message": "check",
                "candidates": ["gpt-5.2-codex"]
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("calibration should fail");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Need at least two available candidate models for calibration.".to_string()
            )
        );
    }

    #[tokio::test]
    async fn calibrate_model_sub_returns_runs_for_candidates() {
        #[derive(Debug, Deserialize)]
        struct CalibrateResult {
            runs: Vec<CalibrateRun>,
            recommended_for_vouch: Option<String>,
            recommended_for_latency: Option<String>,
            recommended_for_session: Option<String>,
        }

        #[derive(Debug, Deserialize)]
        struct CalibrateRun {
            model: String,
            agent_id: Option<String>,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "calibrate_model_sub",
            function_payload(json!({
                "message": "inspect this repo",
                "candidates": ["gpt-5.2-codex", "gpt-5.1-codex-mini"],
                "wait_timeout_ms": 100
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("calibration should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: CalibrateResult =
            serde_json::from_str(&content).expect("calibration result should parse");
        assert_eq!(result.runs.len(), 2);
        let mut models = result
            .runs
            .iter()
            .map(|run| run.model.clone())
            .collect::<Vec<_>>();
        models.sort();
        assert_eq!(
            models,
            vec![
                "gpt-5.1-codex-mini".to_string(),
                "gpt-5.2-codex".to_string()
            ]
        );
        assert!(result.runs.iter().all(|run| run.agent_id.is_some()));
        assert_eq!(result.recommended_for_vouch, None);
        assert_eq!(
            result.recommended_for_session,
            result.recommended_for_latency
        );
    }

    #[tokio::test]
    async fn calibrate_model_sub_uses_vouch_hint_for_session_recommendation() {
        #[derive(Debug, Deserialize)]
        struct CalibrateResult {
            recommended_for_vouch: Option<String>,
            recommended_for_session: Option<String>,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        write_model_sub_vouch(
            &turn.config.codex_home,
            r#"{
  "models": {
    "gpt-5.2-codex": {
      "wins": 4,
      "losses": 0,
      "recent_events": [{ "verdict": "Win" }, { "verdict": "Win" }]
    },
    "gpt-5.1-codex-mini": {
      "wins": 1,
      "losses": 0,
      "recent_events": [{ "verdict": "Loss" }]
    }
  }
}"#,
        );
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "calibrate_model_sub",
            function_payload(json!({
                "message": "inspect this repo",
                "candidates": ["gpt-5.2-codex", "gpt-5.1-codex-mini"],
                "wait_timeout_ms": 100
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("calibration should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: CalibrateResult =
            serde_json::from_str(&content).expect("calibration result should parse");
        assert_eq!(
            result.recommended_for_vouch.as_deref(),
            Some("gpt-5.2-codex")
        );
        assert_eq!(
            result.recommended_for_session.as_deref(),
            Some("gpt-5.2-codex")
        );
    }

    #[tokio::test]
    async fn calibrate_model_sub_caches_candidate_models_for_follow_up_recording() {
        #[derive(Debug, Deserialize)]
        struct CalibrateResult {
            recommended_for_session: Option<String>,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let session = Arc::new(session);
        let invocation = invocation(
            session.clone(),
            Arc::new(turn),
            "calibrate_model_sub",
            function_payload(json!({
                "message": "inspect this repo",
                "candidates": ["gpt-5.2-codex", "gpt-5.1-codex-mini"],
                "wait_timeout_ms": 100
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("calibration should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: CalibrateResult =
            serde_json::from_str(&content).expect("calibration result should parse");
        assert_eq!(
            session.get_last_model_sub_calibration_models().await,
            vec![
                "gpt-5.2-codex".to_string(),
                "gpt-5.1-codex-mini".to_string(),
            ]
        );
        let cached_recommended = session
            .get_last_model_sub_calibration_recommended_for_session()
            .await;
        if let Some(cached_recommended) = cached_recommended {
            assert_eq!(Some(cached_recommended), result.recommended_for_session);
        }
    }

    #[tokio::test]
    async fn spawn_agent_rejects_empty_message() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({"message": "   "})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("empty message should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Empty message can't be sent to an agent".to_string()
            )
        );
    }

    #[tokio::test]
    async fn spawn_agent_rejects_when_message_and_items_are_both_set() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "hello",
                "items": [{"type": "mention", "name": "drive", "path": "app://drive"}]
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("message+items should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Provide either message or items, but not both".to_string()
            )
        );
    }

    #[tokio::test]
    async fn spawn_agent_uses_explorer_role_and_preserves_approval_policy() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            nickname: Option<String>,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        session.set_auto_model_sub_calibration_attempted(true).await;
        let mut config = (*turn.config).clone();
        config
            .permissions
            .approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should be set");
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should be set");
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "agent_type": "explorer"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        assert!(
            result
                .nickname
                .as_deref()
                .is_some_and(|nickname| !nickname.is_empty())
        );
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "gpt-5.2");
        assert_eq!(snapshot.approval_policy, AskForApproval::OnRequest);
    }

    #[tokio::test]
    async fn spawn_agent_default_role_uses_model_sub_override_for_child_model() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            agent_type: String,
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.model_sub = Some("claude-sonnet-4-6".to_string());
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.agent_type, "default");
        assert_eq!(result.model, "claude-sonnet-4-6");
        assert_eq!(result.model_provider_id, "anthropic");
        assert_eq!(result.model_source, "model_sub");

        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "claude-sonnet-4-6");
        assert_eq!(snapshot.model_provider_id, "anthropic");
    }

    #[tokio::test]
    async fn spawn_agent_explorer_role_uses_model_sub_override_for_child_model() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            agent_type: String,
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.model_sub = Some("claude-sonnet-4-6".to_string());
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "agent_type": "explorer"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.agent_type, "explorer");
        assert_eq!(result.model, "claude-sonnet-4-6");
        assert_eq!(result.model_provider_id, "anthropic");
        assert_eq!(result.model_source, "model_sub");

        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "claude-sonnet-4-6");
        assert_eq!(snapshot.model_provider_id, "anthropic");
    }

    #[tokio::test]
    async fn spawn_agent_explicit_default_role_uses_model_sub_override_for_child_model() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            agent_type: String,
            model: String,
            model_provider_id: String,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.model_sub = Some("claude-sonnet-4-6".to_string());
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "agent_type": "default"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.agent_type, "default");
        assert_eq!(result.model, "claude-sonnet-4-6");
        assert_eq!(result.model_provider_id, "anthropic");

        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "claude-sonnet-4-6");
        assert_eq!(snapshot.model_provider_id, "anthropic");
    }

    #[tokio::test]
    async fn spawn_agent_model_override_takes_precedence_over_model_sub() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.model_sub = Some("claude-sonnet-4-6".to_string());
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "model": "gpt-5.1-codex-mini"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.model, "gpt-5.1-codex-mini");
        assert_eq!(result.model_provider_id, "openai");
        assert_eq!(result.model_source, "explicit");

        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "gpt-5.1-codex-mini");
        assert_eq!(snapshot.model_provider_id, "openai");
    }

    #[tokio::test]
    async fn spawn_agent_explorer_model_override_takes_precedence_over_model_sub() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.model_sub = Some("claude-sonnet-4-6".to_string());
        turn.config = Arc::new(config);

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "agent_type": "explorer",
                "model": "gpt-5.1-codex-mini"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.model, "gpt-5.1-codex-mini");
        assert_eq!(result.model_provider_id, "openai");
        assert_eq!(result.model_source, "explicit");

        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.model, "gpt-5.1-codex-mini");
        assert_eq!(snapshot.model_provider_id, "openai");
    }

    #[tokio::test]
    async fn spawn_agent_default_role_without_model_sub_reports_parent_model_source() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, turn) = make_session_and_context().await;
        session.set_auto_model_sub_calibration_attempted(true).await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let expected_model = turn.model_info.slug.clone();
        let expected_provider_id = turn.config.model_provider_id.clone();
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.model, expected_model);
        assert_eq!(result.model_provider_id, expected_provider_id);
        assert_eq!(result.model_source, "parent");
    }

    #[tokio::test]
    async fn spawn_agent_marks_auto_calibration_attempted_when_no_signal() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            model_source: String,
            auto_calibration: Option<AutoCalibrationResult>,
        }

        #[derive(Debug, Deserialize)]
        struct AutoCalibrationResult {
            runs: Vec<AutoCalibrationRunSummary>,
            recommended_for_latency: Option<String>,
            recommended_for_session: Option<String>,
        }

        #[derive(Debug, Deserialize)]
        struct AutoCalibrationRunSummary {
            model: String,
            status: String,
            output_preview: Option<String>,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let session = Arc::new(session);
        let turn = Arc::new(turn);

        let output = MultiAgentHandler
            .handle(invocation(
                session.clone(),
                turn,
                "spawn_agent",
                function_payload(json!({
                    "message": "inspect this repo"
                })),
            ))
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert!(!result.model_source.is_empty());
        let auto_calibration = result
            .auto_calibration
            .expect("auto_calibration should be present");
        assert!(!auto_calibration.runs.is_empty());
        assert!(
            auto_calibration
                .runs
                .iter()
                .all(|run| !run.model.is_empty())
        );
        assert!(
            auto_calibration
                .runs
                .iter()
                .all(|run| !run.status.is_empty())
        );
        assert!(
            auto_calibration
                .runs
                .iter()
                .filter_map(|run| run.output_preview.as_ref())
                .all(|preview| !preview.trim().is_empty())
        );
        let _ = (
            auto_calibration.recommended_for_session,
            auto_calibration.recommended_for_latency,
        );
        assert_eq!(
            session.get_auto_model_sub_calibration_attempted().await,
            true
        );
    }

    #[tokio::test]
    async fn spawn_agent_default_role_uses_auto_model_sub_from_vouch_when_unset() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            model: String,
            model_provider_id: String,
            model_source: String,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        write_model_sub_vouch(
            &turn.config.codex_home,
            r#"{
  "models": {
    "gpt-5.2": {
      "wins": 3,
      "losses": 0,
      "recent_events": [{ "verdict": "Win" }, { "verdict": "Win" }]
    },
    "gpt-5.1-codex-mini": {
      "wins": 1,
      "losses": 0,
      "recent_events": [{ "verdict": "Loss" }]
    }
  }
}"#,
        );

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(result.model, "gpt-5.2");
        assert_eq!(result.model_provider_id, "openai");
        assert_eq!(result.model_source, "model_sub_auto");
    }

    #[tokio::test]
    async fn spawn_agent_auto_model_sub_uses_session_cache_after_first_selection() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            model: String,
            model_source: String,
        }

        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        write_model_sub_vouch(
            &turn.config.codex_home,
            r#"{
  "models": {
    "gpt-5.2": {
      "wins": 3,
      "losses": 0,
      "recent_events": [{ "verdict": "Win" }]
    },
    "gpt-5.1-codex-mini": {
      "wins": 1,
      "losses": 0,
      "recent_events": [{ "verdict": "Loss" }]
    }
  }
}"#,
        );
        let session = Arc::new(session);
        let turn = Arc::new(turn);

        let first = MultiAgentHandler
            .handle(invocation(
                session.clone(),
                turn.clone(),
                "spawn_agent",
                function_payload(json!({
                    "message": "inspect this repo"
                })),
            ))
            .await
            .expect("first spawn should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = first
        else {
            panic!("expected function output");
        };
        let first_result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(first_result.model, "gpt-5.2");
        assert_eq!(first_result.model_source, "model_sub_auto");
        assert_eq!(
            session.get_auto_model_sub_selection().await,
            Some("gpt-5.2".to_string())
        );

        // Flip the ledger recommendation; cached session selection should still win.
        write_model_sub_vouch(
            &turn.config.codex_home,
            r#"{
  "models": {
    "gpt-5.2": {
      "wins": 1,
      "losses": 0,
      "recent_events": [{ "verdict": "Loss" }]
    },
    "gpt-5.1-codex-mini": {
      "wins": 3,
      "losses": 0,
      "recent_events": [{ "verdict": "Win" }]
    }
  }
}"#,
        );

        let second = MultiAgentHandler
            .handle(invocation(
                session,
                turn,
                "spawn_agent",
                function_payload(json!({
                    "message": "inspect this repo again"
                })),
            ))
            .await
            .expect("second spawn should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = second
        else {
            panic!("expected function output");
        };
        let second_result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert_eq!(second_result.model, "gpt-5.2");
        assert_eq!(second_result.model_source, "model_sub_auto");
    }

    #[tokio::test]
    async fn spawn_agent_rejects_unavailable_model_override() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "inspect this repo",
                "model": "unknown-model"
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("spawn should fail");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Model override `unknown-model` is unavailable for spawn_agent.".to_string()
            )
        );
    }

    #[tokio::test]
    async fn spawn_agent_errors_when_manager_dropped() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({"message": "hello"})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("spawn should fail without a manager");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        );
    }

    #[tokio::test]
    async fn spawn_agent_reapplies_runtime_sandbox_after_role_config() {
        fn pick_allowed_sandbox_policy(
            constraint: &crate::config::Constrained<SandboxPolicy>,
            base: SandboxPolicy,
        ) -> SandboxPolicy {
            let candidates = [
                SandboxPolicy::DangerFullAccess,
                SandboxPolicy::new_workspace_write_policy(),
                SandboxPolicy::new_read_only_policy(),
            ];
            candidates
                .into_iter()
                .find(|candidate| *candidate != base && constraint.can_set(candidate).is_ok())
                .unwrap_or(base)
        }

        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            nickname: Option<String>,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let expected_sandbox = pick_allowed_sandbox_policy(
            &turn.config.permissions.sandbox_policy,
            turn.config.permissions.sandbox_policy.get().clone(),
        );
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should be set");
        turn.sandbox_policy
            .set(expected_sandbox.clone())
            .expect("sandbox policy should be set");
        assert_ne!(
            expected_sandbox,
            turn.config.permissions.sandbox_policy.get().clone(),
            "test requires a runtime sandbox override that differs from base config"
        );

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({
                "message": "await this command",
                "agent_type": "awaiter"
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        let agent_id = agent_id(&result.agent_id).expect("agent_id should be valid");
        assert!(
            result
                .nickname
                .as_deref()
                .is_some_and(|nickname| !nickname.is_empty())
        );

        let snapshot = manager
            .get_thread(agent_id)
            .await
            .expect("spawned agent thread should exist")
            .config_snapshot()
            .await;
        assert_eq!(snapshot.sandbox_policy, expected_sandbox);
        assert_eq!(snapshot.approval_policy, AskForApproval::OnRequest);
    }

    #[tokio::test]
    async fn spawn_agent_rejects_when_depth_limit_exceeded() {
        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let max_depth = turn.config.agent_max_depth;
        turn.session_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: session.conversation_id,
            depth: max_depth,
            agent_nickname: None,
            agent_role: None,
        });

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({"message": "hello"})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("spawn should fail when depth limit exceeded");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Agent depth limit reached. Solve the task yourself.".to_string()
            )
        );
    }

    #[tokio::test]
    async fn spawn_agent_allows_depth_up_to_configured_max_depth() {
        #[derive(Debug, Deserialize)]
        struct SpawnAgentResult {
            agent_id: String,
            nickname: Option<String>,
        }

        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let mut config = (*turn.config).clone();
        config.agent_max_depth = DEFAULT_AGENT_MAX_DEPTH + 1;
        turn.config = Arc::new(config);
        turn.session_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: session.conversation_id,
            depth: DEFAULT_AGENT_MAX_DEPTH,
            agent_nickname: None,
            agent_role: None,
        });

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "spawn_agent",
            function_payload(json!({"message": "hello"})),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("spawn should succeed within configured depth");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: SpawnAgentResult =
            serde_json::from_str(&content).expect("spawn_agent result should be json");
        assert!(!result.agent_id.is_empty());
        assert!(
            result
                .nickname
                .as_deref()
                .is_some_and(|nickname| !nickname.is_empty())
        );
        assert_eq!(success, Some(true));
    }

    #[tokio::test]
    async fn send_input_rejects_empty_message() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({"id": ThreadId::new().to_string(), "message": ""})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("empty message should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Empty message can't be sent to an agent".to_string()
            )
        );
    }

    #[tokio::test]
    async fn send_input_rejects_when_message_and_items_are_both_set() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({
                "id": ThreadId::new().to_string(),
                "message": "hello",
                "items": [{"type": "mention", "name": "drive", "path": "app://drive"}]
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("message+items should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Provide either message or items, but not both".to_string()
            )
        );
    }

    #[tokio::test]
    async fn send_input_rejects_invalid_id() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({"id": "not-a-uuid", "message": "hi"})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("invalid id should be rejected");
        };
        let FunctionCallError::RespondToModel(msg) = err else {
            panic!("expected respond-to-model error");
        };
        assert!(msg.starts_with("invalid agent id not-a-uuid:"));
    }

    #[tokio::test]
    async fn send_input_reports_missing_agent() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let agent_id = ThreadId::new();
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({"id": agent_id.to_string(), "message": "hi"})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("missing agent should be reported");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} not found"))
        );
    }

    #[tokio::test]
    async fn send_input_interrupts_before_prompt() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({
                "id": agent_id.to_string(),
                "message": "hi",
                "interrupt": true
            })),
        );
        MultiAgentHandler
            .handle(invocation)
            .await
            .expect("send_input should succeed");

        let ops = manager.captured_ops();
        let ops_for_agent: Vec<&Op> = ops
            .iter()
            .filter_map(|(id, op)| (*id == agent_id).then_some(op))
            .collect();
        assert_eq!(ops_for_agent.len(), 2);
        assert!(matches!(ops_for_agent[0], Op::Interrupt));
        assert!(matches!(ops_for_agent[1], Op::UserInput { .. }));

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
    }

    #[tokio::test]
    async fn send_input_accepts_structured_items() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "send_input",
            function_payload(json!({
                "id": agent_id.to_string(),
                "items": [
                    {"type": "mention", "name": "drive", "path": "app://google_drive"},
                    {"type": "text", "text": "read the folder"}
                ]
            })),
        );
        MultiAgentHandler
            .handle(invocation)
            .await
            .expect("send_input should succeed");

        let expected = Op::UserInput {
            items: vec![
                UserInput::Mention {
                    name: "drive".to_string(),
                    path: "app://google_drive".to_string(),
                },
                UserInput::Text {
                    text: "read the folder".to_string(),
                    text_elements: Vec::new(),
                },
            ],
            final_output_json_schema: None,
        };
        let captured = manager
            .captured_ops()
            .into_iter()
            .find(|(id, op)| *id == agent_id && *op == expected);
        assert_eq!(captured, Some((agent_id, expected)));

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
    }

    #[tokio::test]
    async fn resume_agent_rejects_invalid_id() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "resume_agent",
            function_payload(json!({"id": "not-a-uuid"})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("invalid id should be rejected");
        };
        let FunctionCallError::RespondToModel(msg) = err else {
            panic!("expected respond-to-model error");
        };
        assert!(msg.starts_with("invalid agent id not-a-uuid:"));
    }

    #[tokio::test]
    async fn resume_agent_reports_missing_agent() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let agent_id = ThreadId::new();
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "resume_agent",
            function_payload(json!({"id": agent_id.to_string()})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("missing agent should be reported");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} not found"))
        );
    }

    #[tokio::test]
    async fn resume_agent_noops_for_active_agent() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let status_before = manager.agent_control().get_status(agent_id).await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "resume_agent",
            function_payload(json!({"id": agent_id.to_string()})),
        );

        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("resume_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: resume_agent::ResumeAgentResult =
            serde_json::from_str(&content).expect("resume_agent result should be json");
        assert_eq!(result.status, status_before);
        assert_eq!(success, Some(true));

        let thread_ids = manager.list_thread_ids().await;
        assert_eq!(thread_ids, vec![agent_id]);

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
    }

    #[tokio::test]
    async fn resume_agent_restores_closed_agent_and_accepts_send_input() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager
            .resume_thread_with_history(
                config,
                InitialHistory::Forked(vec![RolloutItem::ResponseItem(ResponseItem::Message {
                    id: None,
                    role: "user".to_string(),
                    content: vec![ContentItem::InputText {
                        text: "materialized".to_string(),
                    }],
                    end_turn: None,
                    phase: None,
                    thought_signature: None,
                })]),
                AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
                false,
            )
            .await
            .expect("start thread");
        let agent_id = thread.thread_id;
        let _ = manager
            .agent_control()
            .shutdown_agent(agent_id)
            .await
            .expect("shutdown agent");
        assert_eq!(
            manager.agent_control().get_status(agent_id).await,
            AgentStatus::NotFound
        );
        let session = Arc::new(session);
        let turn = Arc::new(turn);

        let resume_invocation = invocation(
            session.clone(),
            turn.clone(),
            "resume_agent",
            function_payload(json!({"id": agent_id.to_string()})),
        );
        let output = MultiAgentHandler
            .handle(resume_invocation)
            .await
            .expect("resume_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: resume_agent::ResumeAgentResult =
            serde_json::from_str(&content).expect("resume_agent result should be json");
        assert_ne!(result.status, AgentStatus::NotFound);
        assert_eq!(success, Some(true));

        let send_invocation = invocation(
            session,
            turn,
            "send_input",
            function_payload(json!({"id": agent_id.to_string(), "message": "hello"})),
        );
        let output = MultiAgentHandler
            .handle(send_invocation)
            .await
            .expect("send_input should succeed after resume");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: serde_json::Value =
            serde_json::from_str(&content).expect("send_input result should be json");
        let submission_id = result
            .get("submission_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(!submission_id.is_empty());
        assert_eq!(success, Some(true));

        let _ = manager
            .agent_control()
            .shutdown_agent(agent_id)
            .await
            .expect("shutdown resumed agent");
    }

    #[tokio::test]
    async fn resume_agent_rejects_when_depth_limit_exceeded() {
        let (mut session, mut turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();

        let max_depth = turn.config.agent_max_depth;
        turn.session_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: session.conversation_id,
            depth: max_depth,
            agent_nickname: None,
            agent_role: None,
        });

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "resume_agent",
            function_payload(json!({"id": ThreadId::new().to_string()})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("resume should fail when depth limit exceeded");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "Agent depth limit reached. Solve the task yourself.".to_string()
            )
        );
    }

    #[tokio::test]
    async fn wait_rejects_non_positive_timeout() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({
                "ids": [ThreadId::new().to_string()],
                "timeout_ms": 0
            })),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("non-positive timeout should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel("timeout_ms must be greater than zero".to_string())
        );
    }

    #[tokio::test]
    async fn wait_rejects_invalid_id() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({"ids": ["invalid"]})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("invalid id should be rejected");
        };
        let FunctionCallError::RespondToModel(msg) = err else {
            panic!("expected respond-to-model error");
        };
        assert!(msg.starts_with("invalid agent id invalid:"));
    }

    #[tokio::test]
    async fn wait_rejects_empty_ids() {
        let (session, turn) = make_session_and_context().await;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({"ids": []})),
        );
        let Err(err) = MultiAgentHandler.handle(invocation).await else {
            panic!("empty ids should be rejected");
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel("ids must be non-empty".to_string())
        );
    }

    #[tokio::test]
    async fn wait_returns_not_found_for_missing_agents() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let id_a = ThreadId::new();
        let id_b = ThreadId::new();
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({
                "ids": [id_a.to_string(), id_b.to_string()],
                "timeout_ms": 1000
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("wait should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: wait::WaitResult =
            serde_json::from_str(&content).expect("wait result should be json");
        assert_eq!(
            result,
            wait::WaitResult {
                status: HashMap::from([
                    (id_a, AgentStatus::NotFound),
                    (id_b, AgentStatus::NotFound),
                ]),
                timed_out: false,
                memory_scope_version: None,
                memory_binding_key: None,
            }
        );
        assert_eq!(success, None);
    }

    #[tokio::test]
    async fn wait_times_out_when_status_is_not_final() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({
                "ids": [agent_id.to_string()],
                "timeout_ms": MIN_WAIT_TIMEOUT_MS
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("wait should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: wait::WaitResult =
            serde_json::from_str(&content).expect("wait result should be json");
        assert_eq!(
            result,
            wait::WaitResult {
                status: HashMap::new(),
                timed_out: true,
                memory_scope_version: None,
                memory_binding_key: None,
            }
        );
        assert_eq!(success, None);

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
    }

    #[tokio::test]
    async fn wait_clamps_short_timeouts_to_minimum() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({
                "ids": [agent_id.to_string()],
                "timeout_ms": 10
            })),
        );

        let early = timeout(
            Duration::from_millis(50),
            MultiAgentHandler.handle(invocation),
        )
        .await;
        assert!(
            early.is_err(),
            "wait should not return before the minimum timeout clamp"
        );

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
    }

    #[tokio::test]
    async fn wait_returns_final_status_without_timeout() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let mut status_rx = manager
            .agent_control()
            .subscribe_status(agent_id)
            .await
            .expect("subscribe should succeed");

        let _ = thread
            .thread
            .submit(Op::Shutdown {})
            .await
            .expect("shutdown should submit");
        let _ = timeout(Duration::from_secs(1), status_rx.changed())
            .await
            .expect("shutdown status should arrive");

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait",
            function_payload(json!({
                "ids": [agent_id.to_string()],
                "timeout_ms": 1000
            })),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("wait should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: wait::WaitResult =
            serde_json::from_str(&content).expect("wait result should be json");
        assert_eq!(
            result,
            wait::WaitResult {
                status: HashMap::from([(agent_id, AgentStatus::Shutdown)]),
                timed_out: false,
                memory_scope_version: None,
                memory_binding_key: None,
            }
        );
        assert_eq!(success, None);
    }

    #[tokio::test]
    async fn close_agent_submits_shutdown_and_returns_status() {
        let (mut session, turn) = make_session_and_context().await;
        let manager = thread_manager();
        session.services.agent_control = manager.agent_control();
        let config = turn.config.as_ref().clone();
        let thread = manager.start_thread(config).await.expect("start thread");
        let agent_id = thread.thread_id;
        let status_before = manager.agent_control().get_status(agent_id).await;

        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "close_agent",
            function_payload(json!({"id": agent_id.to_string()})),
        );
        let output = MultiAgentHandler
            .handle(invocation)
            .await
            .expect("close_agent should succeed");
        let ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success,
            ..
        } = output
        else {
            panic!("expected function output");
        };
        let result: close_agent::CloseAgentResult =
            serde_json::from_str(&content).expect("close_agent result should be json");
        assert_eq!(result.status, status_before);
        assert_eq!(result.memory_scope_version, None);
        assert_eq!(result.memory_binding_key, None);
        assert_eq!(success, Some(true));

        let ops = manager.captured_ops();
        let submitted_shutdown = ops
            .iter()
            .any(|(id, op)| *id == agent_id && matches!(op, Op::Shutdown));
        assert_eq!(submitted_shutdown, true);

        let status_after = manager.agent_control().get_status(agent_id).await;
        assert_eq!(status_after, AgentStatus::NotFound);
    }

    #[tokio::test]
    async fn build_agent_spawn_config_uses_turn_context_values() {
        fn pick_allowed_sandbox_policy(
            constraint: &crate::config::Constrained<SandboxPolicy>,
            base: SandboxPolicy,
        ) -> SandboxPolicy {
            let candidates = [
                SandboxPolicy::new_read_only_policy(),
                SandboxPolicy::new_workspace_write_policy(),
                SandboxPolicy::DangerFullAccess,
            ];
            candidates
                .into_iter()
                .find(|candidate| *candidate != base && constraint.can_set(candidate).is_ok())
                .unwrap_or(base)
        }

        let (_session, mut turn) = make_session_and_context().await;
        let base_instructions = BaseInstructions {
            text: "base".to_string(),
        };
        turn.developer_instructions = Some("dev".to_string());
        turn.compact_prompt = Some("compact".to_string());
        let logical_provider = crate::model_provider_info::ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                crate::model_provider_info::ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                crate::model_provider_info::ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..turn.provider.clone()
        };
        let active_provider = logical_provider.with_account(&logical_provider.account_pool[1]);
        let mut base_config = (*turn.config).clone();
        base_config.model_provider = logical_provider.clone();
        base_config.user_configured_provider = logical_provider.clone();
        turn.config = Arc::new(base_config);
        turn.provider = active_provider;
        turn.shell_environment_policy = ShellEnvironmentPolicy {
            use_profile: true,
            ..ShellEnvironmentPolicy::default()
        };
        let temp_dir = tempfile::tempdir().expect("temp dir");
        turn.cwd = temp_dir.path().to_path_buf();
        turn.codex_linux_sandbox_exe = Some(PathBuf::from("/bin/echo"));
        let sandbox_policy = pick_allowed_sandbox_policy(
            &turn.config.permissions.sandbox_policy,
            turn.config.permissions.sandbox_policy.get().clone(),
        );
        turn.sandbox_policy
            .set(sandbox_policy)
            .expect("sandbox policy set");
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy set");

        let config = build_agent_spawn_config(&base_instructions, &turn).expect("spawn config");
        let mut expected = (*turn.config).clone();
        expected.base_instructions = Some(base_instructions.text);
        expected.model = Some(turn.model_info.slug.clone());
        expected.model_provider = logical_provider;
        expected.model_reasoning_effort = turn.reasoning_effort;
        expected.model_reasoning_summary = turn.reasoning_summary;
        expected.developer_instructions = turn.developer_instructions.clone();
        expected.compact_prompt = turn.compact_prompt.clone();
        expected.permissions.shell_environment_policy = turn.shell_environment_policy.clone();
        expected.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
        expected.cwd = turn.cwd.clone();
        expected
            .permissions
            .approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy set");
        expected
            .permissions
            .sandbox_policy
            .set(turn.sandbox_policy.get().clone())
            .expect("sandbox policy set");
        assert_eq!(config, expected);
    }

    #[tokio::test]
    async fn build_agent_spawn_config_preserves_base_user_instructions() {
        let (_session, mut turn) = make_session_and_context().await;
        let mut base_config = (*turn.config).clone();
        base_config.user_instructions = Some("base-user".to_string());
        turn.user_instructions = Some("resolved-user".to_string());
        turn.config = Arc::new(base_config.clone());
        let base_instructions = BaseInstructions {
            text: "base".to_string(),
        };

        let config = build_agent_spawn_config(&base_instructions, &turn).expect("spawn config");

        assert_eq!(config.user_instructions, base_config.user_instructions);
    }

    #[tokio::test]
    async fn build_agent_resume_config_clears_base_instructions() {
        let (_session, mut turn) = make_session_and_context().await;
        let mut base_config = (*turn.config).clone();
        base_config.base_instructions = Some("caller-base".to_string());
        let logical_provider = crate::model_provider_info::ModelProviderInfo {
            base_url: None,
            env_key: None,
            account_pool: vec![
                crate::model_provider_info::ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                crate::model_provider_info::ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
            ..turn.provider.clone()
        };
        base_config.model_provider = logical_provider.clone();
        base_config.user_configured_provider = logical_provider.clone();
        turn.config = Arc::new(base_config);
        turn.provider = logical_provider.with_account(&logical_provider.account_pool[1]);
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy set");

        let config = build_agent_resume_config(&turn, 0).expect("resume config");

        let mut expected = (*turn.config).clone();
        expected.base_instructions = None;
        expected.model = Some(turn.model_info.slug.clone());
        expected.model_provider = logical_provider;
        expected.model_reasoning_effort = turn.reasoning_effort;
        expected.model_reasoning_summary = turn.reasoning_summary;
        expected.developer_instructions = turn.developer_instructions.clone();
        expected.compact_prompt = turn.compact_prompt.clone();
        expected.permissions.shell_environment_policy = turn.shell_environment_policy.clone();
        expected.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
        expected.cwd = turn.cwd.clone();
        expected
            .permissions
            .approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy set");
        expected
            .permissions
            .sandbox_policy
            .set(turn.sandbox_policy.get().clone())
            .expect("sandbox policy set");
        assert_eq!(config, expected);
    }
}
