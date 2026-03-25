use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;

use crate::Hook;
use crate::HookEvent;
use crate::HookPayload;
use crate::HookResult;
use crate::command_from_argv;

/// Legacy notify payload appended as the final argv argument for backward compatibility.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum UserNotification {
    #[serde(rename_all = "kebab-case")]
    AgentTurnComplete {
        thread_id: String,
        turn_id: String,
        cwd: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client: Option<String>,
        input_messages: Vec<String>,
        last_assistant_message: Option<String>,
    },
    #[serde(rename_all = "kebab-case")]
    McpToolCallComplete {
        thread_id: String,
        turn_id: String,
        call_id: String,
        server: String,
        tool_name: String,
        duration_ms: u64,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
        provider_name: String,
        model_slug: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_name: Option<String>,
    },
}

fn mcp_tool_call_status_label(status: &crate::HookEventMcpToolCallStatus) -> &'static str {
    match status {
        crate::HookEventMcpToolCallStatus::Ok => "ok",
        crate::HookEventMcpToolCallStatus::ToolError => "tool-error",
        crate::HookEventMcpToolCallStatus::TransportError => "transport-error",
        crate::HookEventMcpToolCallStatus::Declined => "declined",
        crate::HookEventMcpToolCallStatus::Cancelled => "cancelled",
    }
}

pub fn legacy_notify_json(payload: &HookPayload) -> Result<String, serde_json::Error> {
    match &payload.hook_event {
        HookEvent::AfterAgent { event } => {
            serde_json::to_string(&UserNotification::AgentTurnComplete {
                thread_id: event.thread_id.to_string(),
                turn_id: event.turn_id.clone(),
                cwd: payload.cwd.display().to_string(),
                client: payload.client.clone(),
                input_messages: event.input_messages.clone(),
                last_assistant_message: event.last_assistant_message.clone(),
            })
        }
        HookEvent::AfterMcpToolCall { event } => {
            serde_json::to_string(&UserNotification::McpToolCallComplete {
                thread_id: event.thread_id.to_string(),
                turn_id: event.turn_id.clone(),
                call_id: event.call_id.clone(),
                server: event.server.clone(),
                tool_name: event.tool_name.clone(),
                duration_ms: event.duration_ms,
                status: mcp_tool_call_status_label(&event.status).to_string(),
                error_message: event.error_message.clone(),
                provider_name: event.provider_name.clone(),
                model_slug: event.model_slug.clone(),
                agent_name: event.agent_name.clone(),
            })
        }
        HookEvent::AfterToolUse { .. } => Err(serde_json::Error::io(std::io::Error::other(
            "legacy notify payload is only supported for after_agent and after_mcp_tool_call",
        ))),
    }
}

pub fn notify_hook(argv: Vec<String>) -> Hook {
    let argv = Arc::new(argv);
    Hook {
        name: "legacy_notify".to_string(),
        func: Arc::new(move |payload: &HookPayload| {
            let argv = Arc::clone(&argv);
            Box::pin(async move {
                let mut command = match command_from_argv(&argv) {
                    Some(command) => command,
                    None => return HookResult::Success,
                };
                if let Ok(notify_payload) = legacy_notify_json(payload) {
                    command.arg(notify_payload);
                }

                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match command.spawn() {
                    Ok(_) => HookResult::Success,
                    Err(err) => HookResult::FailedContinue(err.into()),
                }
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use serde_json::json;
    use std::path::Path;

    use super::*;
    use crate::HookEventAfterAgent;
    use crate::HookEventAfterMcpToolCall;
    use crate::HookEventMcpToolCallStatus;

    fn expected_notification_json() -> Value {
        json!({
            "type": "agent-turn-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "cwd": "/Users/example/project",
            "client": "codex-tui",
            "input-messages": ["Rename `foo` to `bar` and update the callsites."],
            "last-assistant-message": "Rename complete and verified `cargo build` succeeds.",
        })
    }

    #[test]
    fn test_user_notification() -> Result<()> {
        let notification = UserNotification::AgentTurnComplete {
            thread_id: "b5f6c1c2-1111-2222-3333-444455556666".to_string(),
            turn_id: "12345".to_string(),
            cwd: "/Users/example/project".to_string(),
            client: Some("codex-tui".to_string()),
            input_messages: vec!["Rename `foo` to `bar` and update the callsites.".to_string()],
            last_assistant_message: Some(
                "Rename complete and verified `cargo build` succeeds.".to_string(),
            ),
        };
        let serialized = serde_json::to_string(&notification)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        assert_eq!(actual, expected_notification_json());
        Ok(())
    }

    #[test]
    fn legacy_notify_json_matches_historical_wire_shape() -> Result<()> {
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterAgent {
                event: HookEventAfterAgent {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec![
                        "Rename `foo` to `bar` and update the callsites.".to_string(),
                    ],
                    last_assistant_message: Some(
                        "Rename complete and verified `cargo build` succeeds.".to_string(),
                    ),
                    provider_name: "OpenAI".to_string(),
                    model_slug: "gpt-5".to_string(),
                    memory: None,
                    memory_scope_version: None,
                    memory_scope_kind: None,
                    memory_summary_sha256: None,
                    memory_binding_key: None,
                    memory_context: None,
                },
            },
        };

        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        assert_eq!(actual, expected_notification_json());

        Ok(())
    }

    #[test]
    fn legacy_notify_json_serializes_mcp_tool_call_complete() -> Result<()> {
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterMcpToolCall {
                event: HookEventAfterMcpToolCall {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    call_id: "call-1".to_string(),
                    server: "claude-code".to_string(),
                    tool_name: "claude_code".to_string(),
                    duration_ms: 15,
                    status: HookEventMcpToolCallStatus::Ok,
                    error_message: None,
                    provider_name: "OpenAI".to_string(),
                    model_slug: "gpt-5".to_string(),
                    agent_name: Some("claude-code".to_string()),
                    memory: None,
                    memory_scope_version: None,
                    memory_scope_kind: None,
                    memory_summary_sha256: None,
                    memory_binding_key: None,
                    memory_context: None,
                },
            },
        };

        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        assert_eq!(
            actual,
            json!({
                "type": "mcp-tool-call-complete",
                "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
                "turn-id": "12345",
                "call-id": "call-1",
                "server": "claude-code",
                "tool-name": "claude_code",
                "duration-ms": 15,
                "status": "ok",
                "provider-name": "OpenAI",
                "model-slug": "gpt-5",
                "agent-name": "claude-code",
            })
        );
        Ok(())
    }
}
