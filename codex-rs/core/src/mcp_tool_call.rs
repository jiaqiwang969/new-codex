use std::time::Duration;
use std::time::Instant;

use tracing::error;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::compact;
use crate::mcp::CODEX_APPS_MCP_SERVER_NAME;
use crate::protocol::EventMsg;
use crate::protocol::McpInvocation;
use crate::protocol::McpToolCallBeginEvent;
use crate::protocol::McpToolCallEndEvent;
use crate::state_db;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::request_user_input::RequestUserInputArgs;
use codex_protocol::request_user_input::RequestUserInputQuestion;
use codex_protocol::request_user_input::RequestUserInputQuestionOption;
use codex_protocol::request_user_input::RequestUserInputResponse;
use codex_utils_string::take_bytes_at_char_boundary;
use codex_utils_string::take_last_bytes_at_char_boundary;
use rmcp::model::ToolAnnotations;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

const CLAUDE_CODE_TOOL_NAME: &str = "claude_code";
const CLAUDE_CODE_CONTEXT_KEY: &str = "context";
const CLAUDE_CODE_WORK_FOLDER_KEY: &str = "workFolder";
const CLAUDE_CODE_MAX_CONTEXT_BYTES: usize = 12_000;
const CLAUDE_CODE_CONTEXT_TRUNCATION_NOTICE: &str = "[Context truncated; showing most recent]\n\n";
const CLAUDE_CODE_MAX_RECENT_MESSAGES: usize = 16;
const CLAUDE_CODE_MAX_RECENT_BYTES: usize = 4_800;
const CLAUDE_CODE_MAX_MESSAGE_BYTES: usize = 1_000;
const CLAUDE_CODE_MAX_TRACE_SUMMARY_BYTES: usize = 1_600;
const CLAUDE_CODE_MAX_MEMORY_SUMMARY_BYTES: usize = 1_600;
const CLAUDE_CODE_MAX_SESSION_SUMMARY_BYTES: usize = 3_200;

/// Handles the specified tool call dispatches the appropriate
/// `McpToolCallBegin` and `McpToolCallEnd` events to the `Session`.
pub(crate) async fn handle_mcp_tool_call(
    sess: Arc<Session>,
    turn_context: &TurnContext,
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

    let call_tool_arguments_value = if tool_name == CLAUDE_CODE_TOOL_NAME {
        maybe_inject_claude_code_context(sess.as_ref(), turn_context, arguments_value.clone()).await
    } else {
        arguments_value.clone()
    };

    let invocation = McpInvocation {
        server: server.clone(),
        tool: tool_name.clone(),
        arguments: arguments_value.clone(),
    };

    if let Some(decision) =
        maybe_request_mcp_tool_approval(sess.as_ref(), turn_context, &call_id, &server, &tool_name)
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
                let result: Result<CallToolResult, String> = sess
                    .call_tool(&server, &tool_name, call_tool_arguments_value.clone())
                    .await
                    .map_err(|e| format!("tool call error: {e:?}"));
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
                result
            }
            McpToolApprovalDecision::Decline => {
                let message = "user rejected MCP tool call".to_string();
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
                let message = "user cancelled MCP tool call".to_string();
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
    let result: Result<CallToolResult, String> = sess
        .call_tool(&server, &tool_name, call_tool_arguments_value.clone())
        .await
        .map_err(|e| format!("tool call error: {e:?}"));
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

    let status = if result.is_ok() { "ok" } else { "error" };
    turn_context
        .otel_manager
        .counter("codex.mcp.call", 1, &[("status", status)]);

    ResponseInputItem::McpToolCallOutput { call_id, result }
}

async fn maybe_inject_claude_code_context(
    sess: &Session,
    turn_context: &TurnContext,
    arguments: Option<Value>,
) -> Option<Value> {
    let Some(Value::Object(mut args)) = arguments else {
        return arguments;
    };

    // Ensure Claude Code runs in the session's working directory so it can
    // pick up repo-local instructions like CLAUDE.md.
    let should_inject_work_folder = match args.get(CLAUDE_CODE_WORK_FOLDER_KEY) {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(_) => false,
    };
    if should_inject_work_folder {
        args.insert(
            CLAUDE_CODE_WORK_FOLDER_KEY.to_string(),
            Value::String(turn_context.cwd.to_string_lossy().into_owned()),
        );
    }

    let should_inject_context = match args.get(CLAUDE_CODE_CONTEXT_KEY) {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(_) => false,
    };

    if should_inject_context {
        let context = build_claude_code_context(sess, turn_context).await;
        if !context.trim().is_empty() {
            args.insert(CLAUDE_CODE_CONTEXT_KEY.to_string(), Value::String(context));
        }
    }

    Some(Value::Object(args))
}

async fn build_claude_code_context(sess: &Session, turn_context: &TurnContext) -> String {
    let mut sections = Vec::new();

    sections.push(format!("Working directory: {}", turn_context.cwd.display()));

    if let Some(memory) = state_db::get_thread_memory(
        sess.state_db().as_deref(),
        sess.conversation_id,
        "claude_code_mcp_context",
    )
    .await
    {
        let trace_summary = truncate_text_bytes(
            memory.trace_summary.trim(),
            CLAUDE_CODE_MAX_TRACE_SUMMARY_BYTES,
        );
        let memory_summary = truncate_text_bytes(
            memory.memory_summary.trim(),
            CLAUDE_CODE_MAX_MEMORY_SUMMARY_BYTES,
        );
        sections.push(format!(
            "Saved thread memory:\nTrace summary:\n{}\n\nMemory summary:\n{}",
            trace_summary, memory_summary
        ));
    }

    let history = sess.clone_history().await;
    let collected = collect_claude_code_context_from_history(history.raw_items());
    if let Some(summary) = collected.summary {
        sections.push(format!(
            "Session summary:\n{}",
            truncate_text_bytes(summary.trim(), CLAUDE_CODE_MAX_SESSION_SUMMARY_BYTES).trim()
        ));
    }
    if !collected.recent.is_empty() {
        sections.push(format!(
            "Recent chat excerpt:\n{}",
            format_role_prefixed_messages(&collected.recent).trim()
        ));
    }

    truncate_claude_code_context(sections.join("\n\n"))
}

struct ClaudeCodeHistoryContext {
    summary: Option<String>,
    recent: Vec<(String, String)>,
}

fn collect_claude_code_context_from_history(items: &[ResponseItem]) -> ClaudeCodeHistoryContext {
    let mut summary: Option<String> = None;
    let mut boundary = 0usize;
    let mut messages: Vec<(String, String)> = Vec::new();

    let summary_prefix = format!("{}\n", compact::SUMMARY_PREFIX);

    for item in items {
        let ResponseItem::Message { role, content, .. } = item else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(text) = compact::content_items_to_text(content) else {
            continue;
        };

        if role == "user" && compact::is_summary_message(&text) {
            let suffix = text
                .strip_prefix(&summary_prefix)
                .unwrap_or(text.as_str())
                .to_string();
            summary = Some(suffix);
            boundary = messages.len();
            continue;
        }

        messages.push((
            role.clone(),
            truncate_text_bytes(&text, CLAUDE_CODE_MAX_MESSAGE_BYTES),
        ));
    }

    let after_summary = &messages[boundary..];
    let recent = take_last_messages_with_byte_budget(
        after_summary,
        CLAUDE_CODE_MAX_RECENT_MESSAGES,
        CLAUDE_CODE_MAX_RECENT_BYTES,
    );

    ClaudeCodeHistoryContext { summary, recent }
}

fn take_last_messages_with_byte_budget(
    messages: &[(String, String)],
    max_messages: usize,
    max_bytes: usize,
) -> Vec<(String, String)> {
    if max_messages == 0 || max_bytes == 0 {
        return Vec::new();
    }

    let mut used = 0usize;
    let mut selected_rev = Vec::new();
    for (role, text) in messages.iter().rev().take(max_messages) {
        // Roughly account for formatting overhead ("User: " + "\n\n", etc.).
        let cost = role.len().saturating_add(text.len()).saturating_add(16);
        if !selected_rev.is_empty() && used.saturating_add(cost) > max_bytes {
            break;
        }
        selected_rev.push((role.clone(), text.clone()));
        used = used.saturating_add(cost);
    }
    selected_rev.reverse();
    selected_rev
}

fn truncate_text_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let truncated = take_bytes_at_char_boundary(text, max_bytes);
    format!("{truncated}...")
}

fn format_role_prefixed_messages(messages: &[(String, String)]) -> String {
    let mut out = String::new();
    for (idx, (role, text)) in messages.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
            out.push('\n');
        }
        let label = match role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            other => other,
        };
        out.push_str(&format!("{label}: {}\n", text.trim()));
    }
    out
}

fn truncate_claude_code_context(mut context: String) -> String {
    if context.len() <= CLAUDE_CODE_MAX_CONTEXT_BYTES {
        return context;
    }

    let notice_len = CLAUDE_CODE_CONTEXT_TRUNCATION_NOTICE.len();
    let budget = CLAUDE_CODE_MAX_CONTEXT_BYTES.saturating_sub(notice_len);
    let truncated = take_last_bytes_at_char_boundary(&context, budget);
    context = format!("{CLAUDE_CODE_CONTEXT_TRUNCATION_NOTICE}{truncated}");
    context
}

async fn notify_mcp_tool_call_event(sess: &Session, turn_context: &TurnContext, event: EventMsg) {
    sess.send_event(turn_context, event).await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpToolApprovalDecision {
    Accept,
    AcceptAndRemember,
    Decline,
    Cancel,
}

struct McpToolApprovalMetadata {
    annotations: ToolAnnotations,
    connector_id: Option<String>,
    connector_name: Option<String>,
    tool_title: Option<String>,
}

const MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX: &str = "mcp_tool_call_approval";
const MCP_TOOL_APPROVAL_ACCEPT: &str = "Approve Once";
const MCP_TOOL_APPROVAL_ACCEPT_AND_REMEMBER: &str = "Approve this Session";
const MCP_TOOL_APPROVAL_DECLINE: &str = "Deny";
const MCP_TOOL_APPROVAL_CANCEL: &str = "Cancel";

#[derive(Debug, Serialize)]
struct McpToolApprovalKey {
    server: String,
    connector_id: String,
    tool_name: String,
}

async fn maybe_request_mcp_tool_approval(
    sess: &Session,
    turn_context: &TurnContext,
    call_id: &str,
    server: &str,
    tool_name: &str,
) -> Option<McpToolApprovalDecision> {
    if is_full_access_mode(turn_context) {
        return None;
    }
    if server != CODEX_APPS_MCP_SERVER_NAME {
        return None;
    }

    let metadata = lookup_mcp_tool_metadata(sess, server, tool_name).await?;
    if !requires_mcp_tool_approval(&metadata.annotations) {
        return None;
    }
    let approval_key = metadata
        .connector_id
        .as_deref()
        .map(|connector_id| McpToolApprovalKey {
            server: server.to_string(),
            connector_id: connector_id.to_string(),
            tool_name: tool_name.to_string(),
        });
    if let Some(key) = approval_key.as_ref()
        && mcp_tool_approval_is_remembered(sess, key).await
    {
        return Some(McpToolApprovalDecision::Accept);
    }

    let question_id = format!("{MCP_TOOL_APPROVAL_QUESTION_ID_PREFIX}_{call_id}");
    let question = build_mcp_tool_approval_question(
        question_id.clone(),
        tool_name,
        metadata.tool_title.as_deref(),
        metadata.connector_name.as_deref(),
        &metadata.annotations,
        approval_key.is_some(),
    );
    let args = RequestUserInputArgs {
        questions: vec![question],
    };
    let response = sess
        .request_user_input(turn_context, call_id.to_string(), args)
        .await;
    let decision = parse_mcp_tool_approval_response(response, &question_id);
    if matches!(decision, McpToolApprovalDecision::AcceptAndRemember)
        && let Some(key) = approval_key
    {
        remember_mcp_tool_approval(sess, key).await;
    }
    Some(decision)
}

fn is_full_access_mode(turn_context: &TurnContext) -> bool {
    matches!(turn_context.approval_policy, AskForApproval::Never)
        && matches!(
            turn_context.sandbox_policy,
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
            tool_info
                .tool
                .annotations
                .map(|annotations| McpToolApprovalMetadata {
                    annotations,
                    connector_id: tool_info.connector_id,
                    connector_name: tool_info.connector_name,
                    tool_title: tool_info.tool.title,
                })
        } else {
            None
        }
    })
}

fn build_mcp_tool_approval_question(
    question_id: String,
    tool_name: &str,
    tool_title: Option<&str>,
    connector_name: Option<&str>,
    annotations: &ToolAnnotations,
    allow_remember_option: bool,
) -> RequestUserInputQuestion {
    let destructive = annotations.destructive_hint == Some(true);
    let open_world = annotations.open_world_hint == Some(true);
    let reason = match (destructive, open_world) {
        (true, true) => "may modify data and access external systems",
        (true, false) => "may modify or delete data",
        (false, true) => "may access external systems",
        (false, false) => "may have side effects",
    };

    let tool_label = tool_title.unwrap_or(tool_name);
    let app_label = connector_name
        .map(|name| format!("The {name} app"))
        .unwrap_or_else(|| "This app".to_string());
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

async fn mcp_tool_approval_is_remembered(sess: &Session, key: &McpToolApprovalKey) -> bool {
    let store = sess.services.tool_approvals.lock().await;
    matches!(store.get(key), Some(ReviewDecision::ApprovedForSession))
}

async fn remember_mcp_tool_approval(sess: &Session, key: McpToolApprovalKey) {
    let mut store = sess.services.tool_approvals.lock().await;
    store.put(key, ReviewDecision::ApprovedForSession);
}

fn requires_mcp_tool_approval(annotations: &ToolAnnotations) -> bool {
    annotations.read_only_hint == Some(false)
        && (annotations.destructive_hint == Some(true) || annotations.open_world_hint == Some(true))
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
    use codex_protocol::models::ContentItem;
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
    fn approval_not_required_when_read_only_true() {
        let annotations = annotations(Some(true), Some(true), Some(true));
        assert_eq!(requires_mcp_tool_approval(&annotations), false);
    }

    #[test]
    fn collect_claude_code_context_prefers_last_summary_and_only_keeps_messages_after_it() {
        let summary_prefix = format!("{}\n", compact::SUMMARY_PREFIX);
        let first_summary = format!("{summary_prefix}summary one");
        let second_summary = format!("{summary_prefix}summary two");

        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "before summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: first_summary,
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "after first summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: second_summary,
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "after second summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];

        let collected = collect_claude_code_context_from_history(&items);
        assert_eq!(collected.summary, Some("summary two".to_string()));
        assert_eq!(
            collected.recent,
            vec![("user".to_string(), "after second summary".to_string())]
        );
    }

    #[test]
    fn truncate_claude_code_context_appends_notice() {
        let long = "a".repeat(CLAUDE_CODE_MAX_CONTEXT_BYTES + 10);
        let truncated = truncate_claude_code_context(long);
        assert_eq!(truncated.len() <= CLAUDE_CODE_MAX_CONTEXT_BYTES, true);
        assert_eq!(
            truncated.starts_with(CLAUDE_CODE_CONTEXT_TRUNCATION_NOTICE),
            true
        );
    }
}
