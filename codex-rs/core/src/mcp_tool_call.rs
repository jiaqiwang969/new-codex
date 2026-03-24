use std::time::Duration;
use std::time::Instant;

use tracing::error;

use crate::analytics_client::AppInvocation;
use crate::analytics_client::InvocationType;
use crate::analytics_client::build_track_events_context;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::config::types::AppToolApproval;
use crate::connectors;
use crate::context_packet;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::GuardianMcpAnnotations;
use crate::guardian::review_approval_request;
use crate::guardian::routes_approval_to_guardian;
use crate::mcp::CODEX_APPS_MCP_SERVER_NAME;
use crate::protocol::EventMsg;
use crate::protocol::McpInvocation;
use crate::protocol::McpToolCallBeginEvent;
use crate::protocol::McpToolCallEndEvent;
use codex_hooks::HookEvent;
use codex_hooks::HookEventAfterMcpToolCall;
use codex_hooks::HookEventMcpToolCallStatus;
use codex_hooks::HookPayload;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::openai_models::InputModality;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_protocol::request_user_input::RequestUserInputResponse;
use rmcp::model::ToolAnnotations;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

const CLAUDE_CODE_TOOL_NAME: &str = "claude_code";
const CLAUDE_CODE_SERVER_NAME: &str = "claude-code";
const GEMINI_SERVER_NAME: &str = "gemini";
const GROK_SERVER_NAME: &str = "grok";
const GOOFISH_SERVER_NAME: &str = "goofish";
const MCP_AGENT_CONTEXT_KEY: &str = "context";
const MCP_AGENT_WORK_FOLDER_KEY: &str = "workFolder";
const MCP_AGENT_WORKDIR_KEY: &str = "workdir";
const MCP_AGENT_MEMORY_SCOPE_VERSION_KEY: &str = "memoryScopeVersion";
const MCP_AGENT_MEMORY_SCOPE_VERSION_SNAKE_KEY: &str = "memory_scope_version";
const MCP_AGENT_MEMORY_SCOPE_KIND_KEY: &str = "memoryScopeKind";
const MCP_AGENT_MEMORY_SCOPE_KIND_SNAKE_KEY: &str = "memory_scope_kind";
const MCP_AGENT_MEMORY_SUMMARY_SHA256_KEY: &str = "memorySummarySha256";
const MCP_AGENT_MEMORY_SUMMARY_SHA256_SNAKE_KEY: &str = "memory_summary_sha256";
const MCP_AGENT_MEMORY_BINDING_KEY: &str = "memoryBindingKey";
const MCP_AGENT_MEMORY_BINDING_SNAKE_KEY: &str = "memory_binding_key";
const MCP_AGENT_CONTEXT_KEYS: &[&str] = &[MCP_AGENT_CONTEXT_KEY];
const MCP_AGENT_WORK_FOLDER_KEYS: &[&str] = &[MCP_AGENT_WORK_FOLDER_KEY, MCP_AGENT_WORKDIR_KEY];
const MCP_AGENT_MEMORY_SCOPE_VERSION_KEYS: &[&str] = &[
    MCP_AGENT_MEMORY_SCOPE_VERSION_KEY,
    MCP_AGENT_MEMORY_SCOPE_VERSION_SNAKE_KEY,
];
const MCP_AGENT_MEMORY_SCOPE_KIND_KEYS: &[&str] = &[
    MCP_AGENT_MEMORY_SCOPE_KIND_KEY,
    MCP_AGENT_MEMORY_SCOPE_KIND_SNAKE_KEY,
];
const MCP_AGENT_MEMORY_SUMMARY_SHA256_KEYS: &[&str] = &[
    MCP_AGENT_MEMORY_SUMMARY_SHA256_KEY,
    MCP_AGENT_MEMORY_SUMMARY_SHA256_SNAKE_KEY,
];
const MCP_AGENT_MEMORY_BINDING_KEYS: &[&str] = &[
    MCP_AGENT_MEMORY_BINDING_KEY,
    MCP_AGENT_MEMORY_BINDING_SNAKE_KEY,
];
const USER_REJECTED_MCP_TOOL_CALL_MESSAGE: &str = "user rejected MCP tool call";
const USER_CANCELLED_MCP_TOOL_CALL_MESSAGE: &str = "user cancelled MCP tool call";

/// Handles the specified tool call dispatches the appropriate
/// `McpToolCallBegin` and `McpToolCallEnd` events to the `Session`.
pub(crate) async fn handle_mcp_tool_call(
    sess: Arc<Session>,
    turn_context: &Arc<TurnContext>,
    call_id: String,
    server: String,
    tool_name: String,
    arguments: String,
) -> ResponseInputItem {
    // Parse the `arguments` as JSON. An empty string is OK, but invalid JSON
    // is not.
    let arguments_value = if arguments.trim().is_empty() {
        None
    } else {
        match serde_json::from_str::<Value>(&arguments) {
            Ok(value) => Some(value),
            Err(e) => {
                error!("failed to parse tool call arguments: {e}");
                return ResponseInputItem::FunctionCallOutput {
                    call_id: call_id.clone(),
                    output: FunctionCallOutputPayload {
                        body: FunctionCallOutputBody::Text(format!("err: {e}")),
                        success: Some(false),
                    },
                };
            }
        }
    };

    let call_tool_arguments_value = maybe_inject_mcp_agent_context(
        sess.as_ref(),
        turn_context,
        server.as_str(),
        tool_name.as_str(),
        arguments_value.clone(),
    )
    .await;

    let invocation = McpInvocation {
        server: server.clone(),
        tool: tool_name.clone(),
        arguments: arguments_value.clone(),
    };

    let metadata = lookup_mcp_tool_metadata(sess.as_ref(), &server, &tool_name).await;
    let app_tool_policy = if server == CODEX_APPS_MCP_SERVER_NAME {
        connectors::app_tool_policy(
            &turn_context.config,
            metadata
                .as_ref()
                .and_then(|metadata| metadata.connector_id.as_deref()),
            &tool_name,
            metadata
                .as_ref()
                .and_then(|metadata| metadata.tool_title.as_deref()),
            metadata
                .as_ref()
                .and_then(|metadata| metadata.annotations.as_ref()),
        )
    } else {
        connectors::AppToolPolicy::default()
    };

    if server == CODEX_APPS_MCP_SERVER_NAME && !app_tool_policy.enabled {
        let result = notify_mcp_tool_call_skip(
            sess.as_ref(),
            turn_context,
            &call_id,
            invocation,
            "MCP tool call blocked by app configuration".to_string(),
        )
        .await;
        let status = if result.is_ok() { "ok" } else { "error" };
        turn_context
            .otel_manager
            .counter("codex.mcp.call", 1, &[("status", status)]);
        return ResponseInputItem::McpToolCallOutput { call_id, result };
    }

    if let Some(decision) = maybe_request_mcp_tool_approval(
        &sess,
        turn_context,
        &call_id,
        &invocation,
        metadata.as_ref(),
        app_tool_policy.approval,
    )
    .await
    {
        let result = match decision {
            McpToolApprovalDecision::Accept | McpToolApprovalDecision::AcceptAndRemember => {
                let tool_call_begin_event = EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
                    call_id: call_id.clone(),
                    invocation: invocation.clone(),
                });
                notify_mcp_tool_call_event(sess.as_ref(), turn_context, tool_call_begin_event)
                    .await;

                let start = Instant::now();
                let result = sess
                    .call_tool(&server, &tool_name, call_tool_arguments_value.clone())
                    .await
                    .map_err(|e| format!("tool call error: {e:?}"));
                let result = sanitize_mcp_tool_result_for_model(
                    turn_context
                        .model_info
                        .input_modalities
                        .contains(&InputModality::Image),
                    result,
                );
                if let Err(e) = &result {
                    tracing::warn!("MCP tool call error: {e:?}");
                }
                let tool_call_end_event = EventMsg::McpToolCallEnd(McpToolCallEndEvent {
                    call_id: call_id.clone(),
                    invocation,
                    duration: start.elapsed(),
                    result: result.clone(),
                });
                notify_mcp_tool_call_event(
                    sess.as_ref(),
                    turn_context,
                    tool_call_end_event.clone(),
                )
                .await;
                maybe_track_codex_app_used(sess.as_ref(), turn_context, &server, &tool_name).await;
                result
            }
            McpToolApprovalDecision::Decline => {
                let message = USER_REJECTED_MCP_TOOL_CALL_MESSAGE.to_string();
                notify_mcp_tool_call_skip(
                    sess.as_ref(),
                    turn_context,
                    &call_id,
                    invocation,
                    message,
                )
                .await
            }
            McpToolApprovalDecision::Cancel => {
                let message = USER_CANCELLED_MCP_TOOL_CALL_MESSAGE.to_string();
                notify_mcp_tool_call_skip(
                    sess.as_ref(),
                    turn_context,
                    &call_id,
                    invocation,
                    message,
                )
                .await
            }
        };

        let status = if result.is_ok() { "ok" } else { "error" };
        turn_context
            .otel_manager
            .counter("codex.mcp.call", 1, &[("status", status)]);

        return ResponseInputItem::McpToolCallOutput { call_id, result };
    }

    let tool_call_begin_event = EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
        call_id: call_id.clone(),
        invocation: invocation.clone(),
    });
    notify_mcp_tool_call_event(sess.as_ref(), turn_context, tool_call_begin_event).await;

    let start = Instant::now();
    // Perform the tool call.
    let result = sess
        .call_tool(&server, &tool_name, call_tool_arguments_value.clone())
        .await
        .map_err(|e| format!("tool call error: {e:?}"));
    let result = sanitize_mcp_tool_result_for_model(
        turn_context
            .model_info
            .input_modalities
            .contains(&InputModality::Image),
        result,
    );
    if let Err(e) = &result {
        tracing::warn!("MCP tool call error: {e:?}");
    }
    let tool_call_end_event = EventMsg::McpToolCallEnd(McpToolCallEndEvent {
        call_id: call_id.clone(),
        invocation,
        duration: start.elapsed(),
        result: result.clone(),
    });

    notify_mcp_tool_call_event(sess.as_ref(), turn_context, tool_call_end_event.clone()).await;
    maybe_track_codex_app_used(sess.as_ref(), turn_context, &server, &tool_name).await;

    let status = if result.is_ok() { "ok" } else { "error" };
    turn_context
        .otel_manager
        .counter("codex.mcp.call", 1, &[("status", status)]);

    ResponseInputItem::McpToolCallOutput { call_id, result }
}

async fn maybe_inject_mcp_agent_context(
    sess: &Session,
    turn_context: &TurnContext,
    server: &str,
    tool_name: &str,
    arguments: Option<Value>,
) -> Option<Value> {
    let Some(Value::Object(mut args)) = arguments else {
        return arguments;
    };

    let tool_properties = sess
        .services
        .mcp_connection_manager
        .read()
        .await
        .list_all_tools()
        .await
        .into_values()
        .find(|tool_info| tool_info.server_name == server && tool_info.tool_name == tool_name)
        .and_then(|tool_info| tool_info.tool.input_schema.get("properties").cloned())
        .and_then(|properties| properties.as_object().cloned());
    let has_context_fallback = tool_name == CLAUDE_CODE_TOOL_NAME;
    let work_folder_key = supported_schema_key(&tool_properties, MCP_AGENT_WORK_FOLDER_KEYS)
        .or_else(|| has_context_fallback.then_some(MCP_AGENT_WORK_FOLDER_KEY));
    if let Some(work_folder_key) = work_folder_key
        && should_inject_string_argument(&args, work_folder_key)
    {
        args.insert(
            work_folder_key.to_string(),
            Value::String(turn_context.cwd.to_string_lossy().into_owned()),
        );
    }

    let context_key = supported_schema_key(&tool_properties, MCP_AGENT_CONTEXT_KEYS)
        .or_else(|| has_context_fallback.then_some(MCP_AGENT_CONTEXT_KEY));
    if let Some(context_key) = context_key
        && should_inject_string_argument(&args, context_key)
    {
        let packet_config = if server == CLAUDE_CODE_SERVER_NAME {
            context_packet::CLAUDE_CODE_LARGE_CONTEXT_PACKET_CONFIG
        } else {
            context_packet::CLAUDE_CODE_CONTEXT_PACKET_CONFIG
        };
        let context = context_packet::build_context_packet(sess, turn_context, packet_config).await;
        if !context.trim().is_empty() {
            args.insert(context_key.to_string(), Value::String(context));
        }
    }

    let memory_scope_version_key =
        supported_schema_key(&tool_properties, MCP_AGENT_MEMORY_SCOPE_VERSION_KEYS);
    let memory_scope_kind_key =
        supported_schema_key(&tool_properties, MCP_AGENT_MEMORY_SCOPE_KIND_KEYS);
    let memory_summary_sha256_key =
        supported_schema_key(&tool_properties, MCP_AGENT_MEMORY_SUMMARY_SHA256_KEYS);
    let memory_binding_key = supported_schema_key(&tool_properties, MCP_AGENT_MEMORY_BINDING_KEYS);
    if (memory_scope_version_key.is_some()
        || memory_scope_kind_key.is_some()
        || memory_summary_sha256_key.is_some()
        || memory_binding_key.is_some())
        && let Some(memory_context) = turn_context.resolve_hook_memory_context().await
    {
        if let Some(memory_scope_version_key) = memory_scope_version_key
            && should_inject_string_argument(&args, memory_scope_version_key)
            && let Some(scope_version) = memory_context.active_memory_scope_version.as_ref()
        {
            args.insert(
                memory_scope_version_key.to_string(),
                Value::String(scope_version.clone()),
            );
        }
        if let Some(memory_scope_kind_key) = memory_scope_kind_key
            && should_inject_string_argument(&args, memory_scope_kind_key)
            && let Some(scope_kind) = memory_context.active_scope_kind.as_ref()
        {
            args.insert(
                memory_scope_kind_key.to_string(),
                Value::String(scope_kind.clone()),
            );
        }
        if let Some(memory_summary_sha256_key) = memory_summary_sha256_key
            && should_inject_string_argument(&args, memory_summary_sha256_key)
            && let Some(memory_summary_sha256) =
                memory_context.active_memory_summary_sha256.as_ref()
        {
            args.insert(
                memory_summary_sha256_key.to_string(),
                Value::String(memory_summary_sha256.clone()),
            );
        }
        if let Some(memory_binding_key) = memory_binding_key
            && should_inject_string_argument(&args, memory_binding_key)
            && let Some(binding_key) = memory_context.active_memory_binding_key.as_ref()
        {
            args.insert(
                memory_binding_key.to_string(),
                Value::String(binding_key.clone()),
            );
        }
    }

    Some(Value::Object(args))
}

fn should_inject_string_argument(args: &serde_json::Map<String, Value>, key: &str) -> bool {
    match args.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(_) => false,
    }
}

fn schema_has_property(
    properties: &Option<serde_json::Map<String, Value>>,
    property_name: &str,
) -> bool {
    properties
        .as_ref()
        .is_some_and(|properties| properties.contains_key(property_name))
}

fn supported_schema_key<'a>(
    properties: &Option<serde_json::Map<String, Value>>,
    candidate_keys: &'a [&'a str],
) -> Option<&'a str> {
    candidate_keys
        .iter()
        .copied()
        .find(|candidate_key| schema_has_property(properties, candidate_key))
}

fn sanitize_mcp_tool_result_for_model(
    supports_image_input: bool,
    result: Result<CallToolResult, String>,
) -> Result<CallToolResult, String> {
    if supports_image_input {
        return result;
    }

    result.map(|call_tool_result| CallToolResult {
        content: call_tool_result
            .content
            .iter()
            .map(|block| {
                if let Some(content_type) = block.get("type").and_then(serde_json::Value::as_str)
                    && content_type == "image"
                {
                    return serde_json::json!({
                        "type": "text",
                        "text": "<image content omitted because you do not support image input>",
                    });
                }

                block.clone()
            })
            .collect::<Vec<_>>(),
        structured_content: call_tool_result.structured_content,
        is_error: call_tool_result.is_error,
        meta: call_tool_result.meta,
    })
}

async fn notify_mcp_tool_call_event(sess: &Session, turn_context: &TurnContext, event: EventMsg) {
    let hook_event = if let EventMsg::McpToolCallEnd(tool_call_end) = &event {
        let memory_context = turn_context.resolve_hook_memory_context().await;
        let memory = turn_context.resolve_memory_link().await;
        let memory_scope_version = memory
            .as_ref()
            .and_then(|memory| memory.scope_version.clone());
        let memory_scope_kind = memory.as_ref().and_then(|memory| memory.scope_kind.clone());
        let memory_summary_sha256 = memory
            .as_ref()
            .and_then(|memory| memory.summary_sha256.clone());
        let memory_binding_key = memory
            .as_ref()
            .and_then(|memory| memory.binding_key.clone());
        let (status, error_message) = mcp_tool_call_status_and_error(&tool_call_end.result);
        let duration_ms = u64::try_from(tool_call_end.duration.as_millis()).unwrap_or(u64::MAX);
        let server = tool_call_end.invocation.server.clone();
        let tool_name = tool_call_end.invocation.tool.clone();
        Some(HookEvent::AfterMcpToolCall {
            event: HookEventAfterMcpToolCall {
                thread_id: sess.conversation_id,
                turn_id: turn_context.sub_id.clone(),
                call_id: tool_call_end.call_id.clone(),
                server: server.clone(),
                tool_name: tool_name.clone(),
                duration_ms,
                status,
                error_message,
                provider_name: turn_context.provider.name.clone(),
                model_slug: turn_context.model_info.slug.clone(),
                agent_name: mcp_tool_call_agent_name(server.as_str(), tool_name.as_str()),
                memory,
                memory_scope_version,
                memory_scope_kind,
                memory_summary_sha256,
                memory_binding_key,
                memory_context,
            },
        })
    } else {
        None
    };

    sess.send_event(turn_context, event).await;

    if let Some(hook_event) = hook_event {
        sess.hooks()
            .dispatch(HookPayload {
                session_id: sess.conversation_id,
                cwd: turn_context.cwd.clone(),
                triggered_at: chrono::Utc::now(),
                hook_event,
            })
            .await;
    }
}

fn mcp_tool_call_status_and_error(
    result: &Result<CallToolResult, String>,
) -> (HookEventMcpToolCallStatus, Option<String>) {
    match result {
        Ok(tool_result) => {
            if tool_result.is_error == Some(true) {
                (HookEventMcpToolCallStatus::ToolError, None)
            } else {
                (HookEventMcpToolCallStatus::Ok, None)
            }
        }
        Err(message) => {
            let status = match message.as_str() {
                USER_REJECTED_MCP_TOOL_CALL_MESSAGE => HookEventMcpToolCallStatus::Declined,
                USER_CANCELLED_MCP_TOOL_CALL_MESSAGE => HookEventMcpToolCallStatus::Cancelled,
                _ => HookEventMcpToolCallStatus::TransportError,
            };
            (status, Some(message.clone()))
        }
    }
}

fn mcp_tool_call_agent_name(server: &str, tool_name: &str) -> Option<String> {
    if tool_name == CLAUDE_CODE_TOOL_NAME || server == CLAUDE_CODE_SERVER_NAME {
        return Some(CLAUDE_CODE_SERVER_NAME.to_string());
    }
    match server {
        GEMINI_SERVER_NAME | GROK_SERVER_NAME | GOOFISH_SERVER_NAME => Some(server.to_string()),
        _ => None,
    }
}

struct McpAppUsageMetadata {
    connector_id: Option<String>,
    app_name: Option<String>,
}

async fn maybe_track_codex_app_used(
    sess: &Session,
    turn_context: &TurnContext,
    server: &str,
    tool_name: &str,
) {
    if server != CODEX_APPS_MCP_SERVER_NAME {
        return;
    }
    let metadata = lookup_mcp_app_usage_metadata(sess, server, tool_name).await;
    let (connector_id, app_name) = metadata
        .map(|metadata| (metadata.connector_id, metadata.app_name))
        .unwrap_or((None, None));
    let invocation_type = if let Some(connector_id) = connector_id.as_deref() {
        let mentioned_connector_ids = sess.get_connector_selection().await;
        if mentioned_connector_ids.contains(connector_id) {
            InvocationType::Explicit
        } else {
            InvocationType::Implicit
        }
    } else {
        InvocationType::Implicit
    };

    let tracking = build_track_events_context(
        turn_context.model_info.slug.clone(),
        sess.conversation_id.to_string(),
        turn_context.sub_id.clone(),
    );
    sess.services.analytics_events_client.track_app_used(
        tracking,
        AppInvocation {
            connector_id,
            app_name,
            invocation_type: Some(invocation_type),
        },
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpToolApprovalDecision {
    Accept,
    AcceptAndRemember,
    Decline,
    Cancel,
}

struct McpToolApprovalMetadata {
    annotations: Option<ToolAnnotations>,
    connector_id: Option<String>,
    connector_name: Option<String>,
    tool_title: Option<String>,
    tool_description: Option<String>,
}

const MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX: &str = "mcp_tool_call_approval";
const MCP_TOOL_APPROVAL_ACCEPT: &str = "Approve Once";
const MCP_TOOL_APPROVAL_ACCEPT_AND_REMEMBER: &str = "Approve this Session";
const MCP_TOOL_APPROVAL_DECLINE: &str = "Deny";
const MCP_TOOL_APPROVAL_CANCEL: &str = "Cancel";

#[derive(Debug, Serialize)]
struct McpToolApprovalKey {
    server: String,
    connector_id: Option<String>,
    tool_name: String,
}

async fn maybe_request_mcp_tool_approval(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    call_id: &str,
    invocation: &McpInvocation,
    metadata: Option<&McpToolApprovalMetadata>,
    approval_mode: AppToolApproval,
) -> Option<McpToolApprovalDecision> {
    if approval_mode == AppToolApproval::Approve {
        return None;
    }
    let annotations = metadata.and_then(|metadata| metadata.annotations.as_ref());
    if approval_mode == AppToolApproval::Auto {
        if is_full_access_mode(turn_context) {
            return None;
        }
        if !annotations.is_some_and(requires_mcp_tool_approval) {
            return None;
        }
    }

    let approval_key = if approval_mode == AppToolApproval::Auto {
        let connector_id = metadata.and_then(|metadata| metadata.connector_id.clone());
        if invocation.server == CODEX_APPS_MCP_SERVER_NAME && connector_id.is_none() {
            None
        } else {
            Some(McpToolApprovalKey {
                server: invocation.server.clone(),
                connector_id,
                tool_name: invocation.tool.clone(),
            })
        }
    } else {
        None
    };
    if let Some(key) = approval_key.as_ref()
        && mcp_tool_approval_is_remembered(sess, key).await
    {
        return Some(McpToolApprovalDecision::Accept);
    }
    if routes_approval_to_guardian(turn_context) {
        let decision = review_approval_request(
            sess,
            turn_context,
            build_guardian_mcp_tool_review_request(call_id, invocation, metadata),
            None,
        )
        .await;
        let decision = mcp_tool_approval_decision_from_guardian(decision);
        if matches!(decision, McpToolApprovalDecision::AcceptAndRemember)
            && let Some(key) = approval_key
        {
            remember_mcp_tool_approval(sess, key).await;
        }
        return Some(decision);
    }

    let question_id = format!("{MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX}_{call_id}");
    let question = build_mcp_tool_approval_question(
        question_id.clone(),
        &invocation.server,
        &invocation.tool,
        metadata.and_then(|metadata| metadata.tool_title.as_deref()),
        metadata.and_then(|metadata| metadata.connector_name.as_deref()),
        annotations,
        approval_key.is_some(),
    );
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = sess
        .request_user_input(turn_context, call_id.to_string(), args)
        .await;
    let decision = normalize_approval_decision_for_mode(
        parse_mcp_tool_approval_response(response, &question_id),
        approval_mode,
    );
    if matches!(decision, McpToolApprovalDecision::AcceptAndRemember)
        && let Some(key) = approval_key
    {
        remember_mcp_tool_approval(sess, key).await;
    }
    Some(decision)
}

fn is_full_access_mode(turn_context: &TurnContext) -> bool {
    matches!(turn_context.approval_policy.value(), AskForApproval::Never)
        && matches!(
            turn_context.sandbox_policy.get(),
            SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. }
        )
}

async fn lookup_mcp_tool_metadata(
    sess: &Session,
    server: &str,
    tool_name: &str,
) -> Option<McpToolApprovalMetadata> {
    let tools = sess
        .services
        .mcp_connection_manager
        .read()
        .await
        .list_all_tools()
        .await;

    tools.into_values().find_map(|tool_info| {
        if tool_info.server_name == server && tool_info.tool_name == tool_name {
            Some(McpToolApprovalMetadata {
                annotations: tool_info.tool.annotations,
                connector_id: tool_info.connector_id,
                connector_name: tool_info.connector_name,
                tool_title: tool_info.tool.title,
                tool_description: tool_info.tool.description.map(std::borrow::Cow::into_owned),
            })
        } else {
            None
        }
    })
}

fn build_guardian_mcp_tool_review_request(
    call_id: &str,
    invocation: &McpInvocation,
    metadata: Option<&McpToolApprovalMetadata>,
) -> GuardianApprovalRequest {
    GuardianApprovalRequest::McpToolCall {
        id: call_id.to_string(),
        server: invocation.server.clone(),
        tool_name: invocation.tool.clone(),
        arguments: invocation.arguments.clone(),
        connector_id: metadata.and_then(|metadata| metadata.connector_id.clone()),
        connector_name: metadata.and_then(|metadata| metadata.connector_name.clone()),
        connector_description: None,
        tool_title: metadata.and_then(|metadata| metadata.tool_title.clone()),
        tool_description: metadata.and_then(|metadata| metadata.tool_description.clone()),
        annotations: metadata
            .and_then(|metadata| metadata.annotations.as_ref())
            .map(|annotations| GuardianMcpAnnotations {
                destructive_hint: annotations.destructive_hint,
                open_world_hint: annotations.open_world_hint,
                read_only_hint: annotations.read_only_hint,
            }),
    }
}

fn mcp_tool_approval_decision_from_guardian(decision: ReviewDecision) -> McpToolApprovalDecision {
    match decision {
        ReviewDecision::Approved
        | ReviewDecision::ApprovedExecpolicyAmendment { .. }
        | ReviewDecision::NetworkPolicyAmendment { .. } => McpToolApprovalDecision::Accept,
        ReviewDecision::ApprovedForSession => McpToolApprovalDecision::AcceptAndRemember,
        ReviewDecision::Denied => McpToolApprovalDecision::Decline,
        ReviewDecision::Abort => McpToolApprovalDecision::Cancel,
    }
}

async fn lookup_mcp_app_usage_metadata(
    sess: &Session,
    server: &str,
    tool_name: &str,
) -> Option<McpAppUsageMetadata> {
    let tools = sess
        .services
        .mcp_connection_manager
        .read()
        .await
        .list_all_tools()
        .await;

    tools.into_values().find_map(|tool_info| {
        if tool_info.server_name == server && tool_info.tool_name == tool_name {
            Some(McpAppUsageMetadata {
                connector_id: tool_info.connector_id,
                app_name: tool_info.connector_name,
            })
        } else {
            None
        }
    })
}

fn build_mcp_tool_approval_question(
    question_id: String,
    server: &str,
    tool_name: &str,
    tool_title: Option<&str>,
    connector_name: Option<&str>,
    annotations: Option<&ToolAnnotations>,
    allow_remember_option: bool,
) -> RequestUserInputQuestion {
    let destructive =
        annotations.and_then(|annotations| annotations.destructive_hint) == Some(true);
    let open_world = annotations.and_then(|annotations| annotations.open_world_hint) == Some(true);
    let reason = match (destructive, open_world) {
        (true, true) => "may modify data and access external systems",
        (true, false) => "may modify or delete data",
        (false, true) => "may access external systems",
        (false, false) => "may have side effects",
    };

    let tool_label = tool_title.unwrap_or(tool_name);
    let app_label = connector_name
        .map(|name| format!("The {name} app"))
        .unwrap_or_else(|| {
            if server == CODEX_APPS_MCP_SERVER_NAME {
                "This app".to_string()
            } else {
                format!("The {server} MCP server")
            }
        });
    let question = format!(
        "{app_label} wants to run the tool \"{tool_label}\", which {reason}. Allow this action?"
    );

    let mut options = vec![RequestUserInputQuestionOption {
        label: MCP_TOOL_APPROVAL_ACCEPT.to_string(),
        description: "Run the tool and continue.".to_string(),
    }];
    if allow_remember_option {
        options.push(RequestUserInputQuestionOption {
            label: MCP_TOOL_APPROVAL_ACCEPT_AND_REMEMBER.to_string(),
            description: "Run the tool and remember this choice for this session.".to_string(),
        });
    }
    options.extend([
        RequestUserInputQuestionOption {
            label: MCP_TOOL_APPROVAL_DECLINE.to_string(),
            description: "Decline this tool call and continue.".to_string(),
        },
        RequestUserInputQuestionOption {
            label: MCP_TOOL_APPROVAL_CANCEL.to_string(),
            description: "Cancel this tool call".to_string(),
        },
    ]);

    RequestUserInputQuestion {
        id: question_id,
        header: "Approve app tool call?".to_string(),
        question,
        is_other: false,
        is_secret: false,
        options: Some(options),
    }
}

fn parse_mcp_tool_approval_response(
    response: Option<RequestUserInputResponse>,
    question_id: &str,
) -> McpToolApprovalDecision {
    let Some(response) = response else {
        return McpToolApprovalDecision::Cancel;
    };
    let answers = response
        .answers
        .get(question_id)
        .map(|answer| answer.answers.as_slice());
    let Some(answers) = answers else {
        return McpToolApprovalDecision::Cancel;
    };
    if answers
        .iter()
        .any(|answer| answer == MCP_TOOL_APPROVAL_ACCEPT_AND_REMEMBER)
    {
        McpToolApprovalDecision::AcceptAndRemember
    } else if answers
        .iter()
        .any(|answer| answer == MCP_TOOL_APPROVAL_ACCEPT)
    {
        McpToolApprovalDecision::Accept
    } else if answers
        .iter()
        .any(|answer| answer == MCP_TOOL_APPROVAL_CANCEL)
    {
        McpToolApprovalDecision::Cancel
    } else {
        McpToolApprovalDecision::Decline
    }
}

fn normalize_approval_decision_for_mode(
    decision: McpToolApprovalDecision,
    approval_mode: AppToolApproval,
) -> McpToolApprovalDecision {
    if approval_mode == AppToolApproval::Prompt
        && decision == McpToolApprovalDecision::AcceptAndRemember
    {
        McpToolApprovalDecision::Accept
    } else {
        decision
    }
}

async fn mcp_tool_approval_is_remembered(sess: &Session, key: &McpToolApprovalKey) -> bool {
    let store = sess.services.tool_approvals.lock().await;
    matches!(store.get(key), Some(ReviewDecision::ApprovedForSession))
}

async fn remember_mcp_tool_approval(sess: &Session, key: McpToolApprovalKey) {
    let mut store = sess.services.tool_approvals.lock().await;
    store.put(key, ReviewDecision::ApprovedForSession);
}

fn requires_mcp_tool_approval(annotations: &ToolAnnotations) -> bool {
    if annotations.destructive_hint == Some(true) {
        return true;
    }

    annotations.read_only_hint == Some(false) && annotations.open_world_hint == Some(true)
}

async fn notify_mcp_tool_call_skip(
    sess: &Session,
    turn_context: &TurnContext,
    call_id: &str,
    invocation: McpInvocation,
    message: String,
) -> Result<CallToolResult, String> {
    let tool_call_begin_event = EventMsg::McpToolCallBegin(McpToolCallBeginEvent {
        call_id: call_id.to_string(),
        invocation: invocation.clone(),
    });
    notify_mcp_tool_call_event(sess, turn_context, tool_call_begin_event).await;

    let tool_call_end_event = EventMsg::McpToolCallEnd(McpToolCallEndEvent {
        call_id: call_id.to_string(),
        invocation,
        duration: Duration::ZERO,
        result: Err(message.clone()),
    });
    notify_mcp_tool_call_event(sess, turn_context, tool_call_end_event).await;
    Err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn annotations(
        read_only: Option<bool>,
        destructive: Option<bool>,
        open_world: Option<bool>,
    ) -> ToolAnnotations {
        ToolAnnotations {
            destructive_hint: destructive,
            idempotent_hint: None,
            open_world_hint: open_world,
            read_only_hint: read_only,
            title: None,
        }
    }

    #[test]
    fn approval_required_when_read_only_false_and_destructive() {
        let annotations = annotations(Some(false), Some(true), None);
        assert_eq!(requires_mcp_tool_approval(&annotations), true);
    }

    #[test]
    fn approval_required_when_read_only_false_and_open_world() {
        let annotations = annotations(Some(false), None, Some(true));
        assert_eq!(requires_mcp_tool_approval(&annotations), true);
    }

    #[test]
    fn approval_required_when_destructive_even_if_read_only_true() {
        let annotations = annotations(Some(true), Some(true), Some(true));
        assert_eq!(requires_mcp_tool_approval(&annotations), true);
    }

    #[test]
    fn prompt_mode_does_not_allow_session_remember() {
        assert_eq!(
            normalize_approval_decision_for_mode(
                McpToolApprovalDecision::AcceptAndRemember,
                AppToolApproval::Prompt,
            ),
            McpToolApprovalDecision::Accept
        );
    }

    #[test]
    fn custom_mcp_tool_question_mentions_server_name() {
        let question = build_mcp_tool_approval_question(
            "q".to_string(),
            "custom_server",
            "run_action",
            Some("Run Action"),
            None,
            Some(&annotations(Some(false), Some(true), None)),
            true,
        );

        assert_eq!(question.header, "Approve app tool call?");
        assert_eq!(
            question.question,
            "The custom_server MCP server wants to run the tool \"Run Action\", which may modify or delete data. Allow this action?"
        );
        assert!(
            question
                .options
                .expect("options")
                .into_iter()
                .map(|option| option.label)
                .any(|label| label == MCP_TOOL_APPROVAL_ACCEPT_AND_REMEMBER)
        );
    }

    #[test]
    fn codex_apps_tool_question_keeps_legacy_app_label() {
        let question = build_mcp_tool_approval_question(
            "q".to_string(),
            CODEX_APPS_MCP_SERVER_NAME,
            "run_action",
            Some("Run Action"),
            None,
            Some(&annotations(Some(false), Some(true), None)),
            true,
        );

        assert!(
            question
                .question
                .starts_with("This app wants to run the tool \"Run Action\"")
        );
    }

    #[test]
    fn sanitize_mcp_tool_result_for_model_rewrites_image_content() {
        let result = Ok(CallToolResult {
            content: vec![
                serde_json::json!({
                    "type": "image",
                    "data": "Zm9v",
                    "mimeType": "image/png",
                }),
                serde_json::json!({
                    "type": "text",
                    "text": "hello",
                }),
            ],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        });

        let got = sanitize_mcp_tool_result_for_model(false, result).expect("sanitized result");

        assert_eq!(
            got.content,
            vec![
                serde_json::json!({
                    "type": "text",
                    "text": "<image content omitted because you do not support image input>",
                }),
                serde_json::json!({
                    "type": "text",
                    "text": "hello",
                }),
            ]
        );
    }

    #[test]
    fn sanitize_mcp_tool_result_for_model_preserves_image_when_supported() {
        let original = CallToolResult {
            content: vec![serde_json::json!({
                "type": "image",
                "data": "Zm9v",
                "mimeType": "image/png",
            })],
            structured_content: Some(serde_json::json!({"x": 1})),
            is_error: Some(false),
            meta: Some(serde_json::json!({"k": "v"})),
        };

        let got = sanitize_mcp_tool_result_for_model(true, Ok(original.clone()))
            .expect("unsanitized result");

        assert_eq!(got, original);
    }
}
