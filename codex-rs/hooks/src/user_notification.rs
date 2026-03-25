use std::process::Stdio;
use std::sync::Arc;

use codex_protocol::protocol::MemoryLink;
use serde::Serialize;
use tokio::process::Command;

use crate::Hook;
use crate::HookEvent;
use crate::HookPayload;
use crate::HookResult;
use crate::command_from_argv;

const CODEX_HOOK_EVENT: &str = "CODEX_HOOK_EVENT";
const CODEX_HOOK_THREAD_ID: &str = "CODEX_HOOK_THREAD_ID";
const CODEX_HOOK_TURN_ID: &str = "CODEX_HOOK_TURN_ID";
const CODEX_HOOK_CWD: &str = "CODEX_HOOK_CWD";
const CODEX_HOOK_PROVIDER_NAME: &str = "CODEX_HOOK_PROVIDER_NAME";
const CODEX_HOOK_MODEL_SLUG: &str = "CODEX_HOOK_MODEL_SLUG";
const CODEX_HOOK_MEMORY_SCOPE_VERSION: &str = "CODEX_HOOK_MEMORY_SCOPE_VERSION";
const CODEX_HOOK_MEMORY_SCOPE_KIND: &str = "CODEX_HOOK_MEMORY_SCOPE_KIND";
const CODEX_HOOK_MEMORY_SUMMARY_SHA256: &str = "CODEX_HOOK_MEMORY_SUMMARY_SHA256";
const CODEX_HOOK_MEMORY_BINDING_KEY: &str = "CODEX_HOOK_MEMORY_BINDING_KEY";
const CODEX_HOOK_ACTIVE_MEMORY_SCOPE_VERSION: &str = "CODEX_HOOK_ACTIVE_MEMORY_SCOPE_VERSION";
const CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY: &str = "CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY";
const CODEX_HOOK_MCP_CALL_ID: &str = "CODEX_HOOK_MCP_CALL_ID";
const CODEX_HOOK_MCP_SERVER: &str = "CODEX_HOOK_MCP_SERVER";
const CODEX_HOOK_MCP_TOOL_NAME: &str = "CODEX_HOOK_MCP_TOOL_NAME";
const CODEX_HOOK_MCP_STATUS: &str = "CODEX_HOOK_MCP_STATUS";
const CODEX_HOOK_MCP_ERROR_MESSAGE: &str = "CODEX_HOOK_MCP_ERROR_MESSAGE";
const CODEX_HOOK_AGENT_NAME: &str = "CODEX_HOOK_AGENT_NAME";

const HOOK_EVENT_AGENT_TURN_COMPLETE: &str = "agent-turn-complete";
const HOOK_EVENT_MCP_TOOL_CALL_COMPLETE: &str = "mcp-tool-call-complete";

const KNOWN_NOTIFY_ENV_VARS: &[&str] = &[
    CODEX_HOOK_EVENT,
    CODEX_HOOK_THREAD_ID,
    CODEX_HOOK_TURN_ID,
    CODEX_HOOK_CWD,
    CODEX_HOOK_PROVIDER_NAME,
    CODEX_HOOK_MODEL_SLUG,
    CODEX_HOOK_MEMORY_SCOPE_VERSION,
    CODEX_HOOK_MEMORY_SCOPE_KIND,
    CODEX_HOOK_MEMORY_SUMMARY_SHA256,
    CODEX_HOOK_MEMORY_BINDING_KEY,
    CODEX_HOOK_ACTIVE_MEMORY_SCOPE_VERSION,
    CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY,
    CODEX_HOOK_MCP_CALL_ID,
    CODEX_HOOK_MCP_SERVER,
    CODEX_HOOK_MCP_TOOL_NAME,
    CODEX_HOOK_MCP_STATUS,
    CODEX_HOOK_MCP_ERROR_MESSAGE,
    CODEX_HOOK_AGENT_NAME,
];

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

        /// Messages that the user sent to the agent to initiate the turn.
        input_messages: Vec<String>,

        /// The last message sent by the assistant in the turn.
        last_assistant_message: Option<String>,

        /// Top-level model provider handling this turn.
        provider_name: String,

        /// Top-level model selected for this turn.
        model_slug: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        memory: Option<MemoryLink>,

        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_summary_sha256: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_binding_key: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        memory_context: Option<LegacyMemoryContext>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        memory: Option<MemoryLink>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_scope_kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_summary_sha256: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_binding_key: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        memory_context: Option<LegacyMemoryContext>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
struct LegacyMemoryContext {
    cwd_scope_key: String,
    cwd_memory_root: String,
    cwd_memory_summary_path: String,
    cwd_memory_summary_exists: bool,
    user_memory_root: String,
    user_memory_summary_path: String,
    user_memory_summary_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_summary_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_summary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_summary_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_scope_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_memory_binding_key: Option<String>,
}

fn legacy_memory_context(
    memory_context: &Option<crate::HookEventMemoryContext>,
) -> Option<LegacyMemoryContext> {
    memory_context
        .as_ref()
        .map(|memory_context| LegacyMemoryContext {
            cwd_scope_key: memory_context.cwd_scope_key.clone(),
            cwd_memory_root: memory_context.cwd_memory_root.clone(),
            cwd_memory_summary_path: memory_context.cwd_memory_summary_path.clone(),
            cwd_memory_summary_exists: memory_context.cwd_memory_summary_exists,
            user_memory_root: memory_context.user_memory_root.clone(),
            user_memory_summary_path: memory_context.user_memory_summary_path.clone(),
            user_memory_summary_exists: memory_context.user_memory_summary_exists,
            active_scope_kind: memory_context.active_scope_kind.clone(),
            active_memory_root: memory_context.active_memory_root.clone(),
            active_memory_summary_path: memory_context.active_memory_summary_path.clone(),
            active_memory_summary_sha256: memory_context.active_memory_summary_sha256.clone(),
            active_memory_summary_bytes: memory_context.active_memory_summary_bytes,
            active_memory_scope_version: memory_context.active_memory_scope_version.clone(),
            active_memory_binding_key: memory_context.active_memory_binding_key.clone(),
        })
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

fn set_optional_env(command: &mut Command, key: &str, value: Option<&str>) {
    if let Some(value) = value
        && !value.is_empty()
    {
        command.env(key, value);
    } else {
        command.env_remove(key);
    }
}

fn apply_memory_env(
    command: &mut Command,
    memory_scope_version: Option<&str>,
    memory_scope_kind: Option<&str>,
    memory_summary_sha256: Option<&str>,
    memory_binding_key: Option<&str>,
    memory_context: Option<&crate::HookEventMemoryContext>,
) {
    set_optional_env(
        command,
        CODEX_HOOK_MEMORY_SCOPE_VERSION,
        memory_scope_version,
    );
    set_optional_env(command, CODEX_HOOK_MEMORY_SCOPE_KIND, memory_scope_kind);
    set_optional_env(
        command,
        CODEX_HOOK_MEMORY_SUMMARY_SHA256,
        memory_summary_sha256,
    );
    set_optional_env(command, CODEX_HOOK_MEMORY_BINDING_KEY, memory_binding_key);
    set_optional_env(
        command,
        CODEX_HOOK_ACTIVE_MEMORY_SCOPE_VERSION,
        memory_context
            .and_then(|memory_context| memory_context.active_memory_scope_version.as_deref()),
    );
    set_optional_env(
        command,
        CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY,
        memory_context
            .and_then(|memory_context| memory_context.active_memory_binding_key.as_deref()),
    );
}

fn apply_notify_env(command: &mut Command, payload: &HookPayload) -> bool {
    for key in KNOWN_NOTIFY_ENV_VARS {
        command.env_remove(key);
    }

    command.env(CODEX_HOOK_CWD, &payload.cwd);

    match &payload.hook_event {
        HookEvent::AfterAgent { event } => {
            command.env(CODEX_HOOK_EVENT, HOOK_EVENT_AGENT_TURN_COMPLETE);
            command.env(CODEX_HOOK_THREAD_ID, event.thread_id.to_string());
            command.env(CODEX_HOOK_TURN_ID, &event.turn_id);
            command.env(CODEX_HOOK_PROVIDER_NAME, &event.provider_name);
            command.env(CODEX_HOOK_MODEL_SLUG, &event.model_slug);
            apply_memory_env(
                command,
                event.memory_scope_version.as_deref(),
                event.memory_scope_kind.as_deref(),
                event.memory_summary_sha256.as_deref(),
                event.memory_binding_key.as_deref(),
                event.memory_context.as_ref(),
            );
            true
        }
        HookEvent::AfterMcpToolCall { event } => {
            command.env(CODEX_HOOK_EVENT, HOOK_EVENT_MCP_TOOL_CALL_COMPLETE);
            command.env(CODEX_HOOK_THREAD_ID, event.thread_id.to_string());
            command.env(CODEX_HOOK_TURN_ID, &event.turn_id);
            command.env(CODEX_HOOK_PROVIDER_NAME, &event.provider_name);
            command.env(CODEX_HOOK_MODEL_SLUG, &event.model_slug);
            command.env(CODEX_HOOK_MCP_CALL_ID, &event.call_id);
            command.env(CODEX_HOOK_MCP_SERVER, &event.server);
            command.env(CODEX_HOOK_MCP_TOOL_NAME, &event.tool_name);
            command.env(
                CODEX_HOOK_MCP_STATUS,
                mcp_tool_call_status_label(&event.status),
            );
            set_optional_env(
                command,
                CODEX_HOOK_MCP_ERROR_MESSAGE,
                event.error_message.as_deref(),
            );
            set_optional_env(command, CODEX_HOOK_AGENT_NAME, event.agent_name.as_deref());
            apply_memory_env(
                command,
                event.memory_scope_version.as_deref(),
                event.memory_scope_kind.as_deref(),
                event.memory_summary_sha256.as_deref(),
                event.memory_binding_key.as_deref(),
                event.memory_context.as_ref(),
            );
            true
        }
        HookEvent::AfterToolUse { .. } => false,
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
                provider_name: event.provider_name.clone(),
                model_slug: event.model_slug.clone(),
                memory: event.memory.clone(),
                memory_scope_version: event.memory_scope_version.clone(),
                memory_scope_kind: event.memory_scope_kind.clone(),
                memory_summary_sha256: event.memory_summary_sha256.clone(),
                memory_binding_key: event.memory_binding_key.clone(),
                memory_context: legacy_memory_context(&event.memory_context),
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
                memory: event.memory.clone(),
                memory_scope_version: event.memory_scope_version.clone(),
                memory_scope_kind: event.memory_scope_kind.clone(),
                memory_summary_sha256: event.memory_summary_sha256.clone(),
                memory_binding_key: event.memory_binding_key.clone(),
                memory_context: legacy_memory_context(&event.memory_context),
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
                if !apply_notify_env(&mut command, payload) {
                    return HookResult::Success;
                }
                if let Ok(notify_payload) = legacy_notify_json(payload) {
                    command.arg(notify_payload);
                }

                // Backwards-compat: match legacy notify behavior (argv + JSON arg, fire-and-forget).
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match command.spawn() {
                    Ok(mut child) => {
                        // Avoid leaving zombies around in long-running sessions.
                        tokio::spawn(async move {
                            let _ = child.wait().await;
                        });
                        HookResult::Success
                    }
                    Err(err) => HookResult::FailedContinue(err.into()),
                }
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use anyhow::Result;
    use codex_protocol::ThreadId;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::time::Duration;
    use tokio::time::sleep;
    use tokio::time::timeout;

    use super::*;

    async fn wait_for_file(path: &Path) {
        timeout(Duration::from_secs(3), async {
            while !path.exists() {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("hook output file to appear");
    }

    fn expected_notification_json() -> Value {
        json!({
            "type": "agent-turn-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "cwd": "/Users/example/project",
            "client": "codex-tui",
            "input-messages": ["Rename `foo` to `bar` and update the callsites."],
            "last-assistant-message": "Rename complete and verified `cargo build` succeeds.",
            "provider-name": "Gemini",
            "model-slug": "gemini-2.5-pro",
            "memory-scope-version": "cwd:aaaaaaaaaaaa",
            "memory-scope-kind": "cwd",
            "memory-summary-sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "memory-binding-key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "memory-context": {
                "cwd-scope-key": "/Users/example/project",
                "cwd-memory-root": "/Users/example/.codex/memories/cwd-bucket/memory",
                "cwd-memory-summary-path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                "cwd-memory-summary-exists": true,
                "user-memory-root": "/Users/example/.codex/memories/user/memory",
                "user-memory-summary-path": "/Users/example/.codex/memories/user/memory/memory_summary.md",
                "user-memory-summary-exists": false,
                "active-scope-kind": "cwd",
                "active-memory-root": "/Users/example/.codex/memories/cwd-bucket/memory",
                "active-memory-summary-path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                "active-memory-summary-sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "active-memory-summary-bytes": 123,
                "active-memory-scope-version": "cwd:aaaaaaaaaaaa",
                "active-memory-binding-key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
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
            provider_name: "Gemini".to_string(),
            model_slug: "gemini-2.5-pro".to_string(),
            memory: None,
            memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
            memory_scope_kind: Some("cwd".to_string()),
            memory_summary_sha256: Some("a".repeat(64)),
            memory_binding_key: Some(
                "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
            ),
            memory_context: Some(LegacyMemoryContext {
                cwd_scope_key: "/Users/example/project".to_string(),
                cwd_memory_root: "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                cwd_memory_summary_path:
                    "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md".to_string(),
                cwd_memory_summary_exists: true,
                user_memory_root: "/Users/example/.codex/memories/user/memory".to_string(),
                user_memory_summary_path:
                    "/Users/example/.codex/memories/user/memory/memory_summary.md".to_string(),
                user_memory_summary_exists: false,
                active_scope_kind: Some("cwd".to_string()),
                active_memory_root: Some(
                    "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                ),
                active_memory_summary_path: Some(
                    "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                        .to_string(),
                ),
                active_memory_summary_sha256: Some("a".repeat(64)),
                active_memory_summary_bytes: Some(123),
                active_memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                active_memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
            }),
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
            cwd: std::path::Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterAgent {
                event: crate::HookEventAfterAgent {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    input_messages: vec!["Rename `foo` to `bar` and update the callsites.".to_string()],
                    last_assistant_message: Some(
                        "Rename complete and verified `cargo build` succeeds.".to_string(),
                    ),
                    provider_name: "Gemini".to_string(),
                    model_slug: "gemini-2.5-pro".to_string(),
                    memory: None,
                    memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    memory_scope_kind: Some("cwd".to_string()),
                    memory_summary_sha256: Some("a".repeat(64)),
                    memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                    memory_context: Some(crate::HookEventMemoryContext {
                        cwd_scope_key: "/Users/example/project".to_string(),
                        cwd_memory_root: "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                        cwd_memory_summary_path:
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        cwd_memory_summary_exists: true,
                        user_memory_root: "/Users/example/.codex/memories/user/memory".to_string(),
                        user_memory_summary_path:
                            "/Users/example/.codex/memories/user/memory/memory_summary.md".to_string(),
                        user_memory_summary_exists: false,
                        active_scope_kind: Some("cwd".to_string()),
                        active_memory_root: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                        ),
                        active_memory_summary_path: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        ),
                        active_memory_summary_sha256: Some("a".repeat(64)),
                        active_memory_summary_bytes: Some(123),
                        active_memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                        active_memory_binding_key: Some(
                            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .to_string(),
                        ),
                    }),
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
        let hook_event = HookEvent::AfterMcpToolCall {
            event: crate::HookEventAfterMcpToolCall {
                thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                    .expect("valid thread id"),
                turn_id: "12345".to_string(),
                call_id: "call-1".to_string(),
                server: "claude-code".to_string(),
                tool_name: "claude_code".to_string(),
                duration_ms: 42,
                status: crate::HookEventMcpToolCallStatus::Ok,
                error_message: None,
                provider_name: "OpenAI".to_string(),
                model_slug: "gpt-5".to_string(),
                agent_name: Some("claude-code".to_string()),
                memory: None,
                memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                memory_scope_kind: Some("cwd".to_string()),
                memory_summary_sha256: Some("a".repeat(64)),
                memory_binding_key: Some(
                    "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ),
                memory_context: Some(crate::HookEventMemoryContext {
                    cwd_scope_key: "/Users/example/project".to_string(),
                    cwd_memory_root: "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                    cwd_memory_summary_path:
                        "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                            .to_string(),
                    cwd_memory_summary_exists: true,
                    user_memory_root: "/Users/example/.codex/memories/user/memory".to_string(),
                    user_memory_summary_path:
                        "/Users/example/.codex/memories/user/memory/memory_summary.md".to_string(),
                    user_memory_summary_exists: false,
                    active_scope_kind: Some("cwd".to_string()),
                    active_memory_root: Some(
                        "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                    ),
                    active_memory_summary_path: Some(
                        "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                            .to_string(),
                    ),
                    active_memory_summary_sha256: Some("a".repeat(64)),
                    active_memory_summary_bytes: Some(123),
                    active_memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    active_memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                }),
            },
        };

        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event,
        };
        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        let expected = json!({
            "type": "mcp-tool-call-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "call-id": "call-1",
            "server": "claude-code",
            "tool-name": "claude_code",
            "duration-ms": 42,
            "status": "ok",
            "provider-name": "OpenAI",
            "model-slug": "gpt-5",
            "agent-name": "claude-code",
            "memory-scope-version": "cwd:aaaaaaaaaaaa",
            "memory-scope-kind": "cwd",
            "memory-summary-sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "memory-binding-key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "memory-context": {
                "cwd-scope-key": "/Users/example/project",
                "cwd-memory-root": "/Users/example/.codex/memories/cwd-bucket/memory",
                "cwd-memory-summary-path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                "cwd-memory-summary-exists": true,
                "user-memory-root": "/Users/example/.codex/memories/user/memory",
                "user-memory-summary-path": "/Users/example/.codex/memories/user/memory/memory_summary.md",
                "user-memory-summary-exists": false,
                "active-scope-kind": "cwd",
                "active-memory-root": "/Users/example/.codex/memories/cwd-bucket/memory",
                "active-memory-summary-path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                "active-memory-summary-sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "active-memory-summary-bytes": 123,
                "active-memory-scope-version": "cwd:aaaaaaaaaaaa",
                "active-memory-binding-key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
        });
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn legacy_notify_json_serializes_mcp_tool_call_transport_error() -> Result<()> {
        let hook_event = HookEvent::AfterMcpToolCall {
            event: crate::HookEventAfterMcpToolCall {
                thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                    .expect("valid thread id"),
                turn_id: "12345".to_string(),
                call_id: "call-transport".to_string(),
                server: "rmcp".to_string(),
                tool_name: "not_a_real_tool".to_string(),
                duration_ms: 7,
                status: crate::HookEventMcpToolCallStatus::TransportError,
                error_message: Some("tool call error: unknown tool".to_string()),
                provider_name: "OpenAI".to_string(),
                model_slug: "gpt-5".to_string(),
                agent_name: None,
                memory: None,
                memory_scope_version: None,
                memory_scope_kind: None,
                memory_summary_sha256: None,
                memory_binding_key: None,
                memory_context: None,
            },
        };

        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event,
        };
        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        let expected = json!({
            "type": "mcp-tool-call-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "call-id": "call-transport",
            "server": "rmcp",
            "tool-name": "not_a_real_tool",
            "duration-ms": 7,
            "status": "transport-error",
            "error-message": "tool call error: unknown tool",
            "provider-name": "OpenAI",
            "model-slug": "gpt-5"
        });
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn legacy_notify_json_serializes_mcp_tool_call_tool_error() -> Result<()> {
        let hook_event = HookEvent::AfterMcpToolCall {
            event: crate::HookEventAfterMcpToolCall {
                thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                    .expect("valid thread id"),
                turn_id: "12345".to_string(),
                call_id: "call-tool-error".to_string(),
                server: "rmcp".to_string(),
                tool_name: "soft_error".to_string(),
                duration_ms: 11,
                status: crate::HookEventMcpToolCallStatus::ToolError,
                error_message: None,
                provider_name: "OpenAI".to_string(),
                model_slug: "gpt-5".to_string(),
                agent_name: None,
                memory: None,
                memory_scope_version: None,
                memory_scope_kind: None,
                memory_summary_sha256: None,
                memory_binding_key: None,
                memory_context: None,
            },
        };

        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: Path::new("/Users/example/project").to_path_buf(),
            client: Some("codex-tui".to_string()),
            triggered_at: chrono::Utc::now(),
            hook_event,
        };
        let serialized = legacy_notify_json(&payload)?;
        let actual: Value = serde_json::from_str(&serialized)?;
        let expected = json!({
            "type": "mcp-tool-call-complete",
            "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
            "turn-id": "12345",
            "call-id": "call-tool-error",
            "server": "rmcp",
            "tool-name": "soft_error",
            "duration-ms": 11,
            "status": "tool-error",
            "provider-name": "OpenAI",
            "model-slug": "gpt-5"
        });
        assert_eq!(actual, expected);

        Ok(())
    }

    #[test]
    fn legacy_notify_json_serializes_mcp_tool_call_declined_and_cancelled() -> Result<()> {
        for (status, expected_status, expected_error_message) in [
            (
                crate::HookEventMcpToolCallStatus::Declined,
                "declined",
                "user rejected MCP tool call",
            ),
            (
                crate::HookEventMcpToolCallStatus::Cancelled,
                "cancelled",
                "user cancelled MCP tool call",
            ),
        ] {
            let hook_event = HookEvent::AfterMcpToolCall {
                event: crate::HookEventAfterMcpToolCall {
                    thread_id: ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666")
                        .expect("valid thread id"),
                    turn_id: "12345".to_string(),
                    call_id: format!("call-{expected_status}"),
                    server: "codex_apps".to_string(),
                    tool_name: "dangerous_write".to_string(),
                    duration_ms: 9,
                    status,
                    error_message: Some(expected_error_message.to_string()),
                    provider_name: "OpenAI".to_string(),
                    model_slug: "gpt-5".to_string(),
                    agent_name: None,
                    memory: None,
                    memory_scope_version: None,
                    memory_scope_kind: None,
                    memory_summary_sha256: None,
                    memory_binding_key: None,
                    memory_context: None,
                },
            };

            let payload = HookPayload {
                session_id: ThreadId::new(),
                cwd: Path::new("/Users/example/project").to_path_buf(),
                client: Some("codex-tui".to_string()),
                triggered_at: chrono::Utc::now(),
                hook_event,
            };
            let serialized = legacy_notify_json(&payload)?;
            let actual: Value = serde_json::from_str(&serialized)?;
            let expected = json!({
                "type": "mcp-tool-call-complete",
                "thread-id": "b5f6c1c2-1111-2222-3333-444455556666",
                "turn-id": "12345",
                "call-id": format!("call-{expected_status}"),
                "server": "codex_apps",
                "tool-name": "dangerous_write",
                "duration-ms": 9,
                "status": expected_status,
                "error-message": expected_error_message,
                "provider-name": "OpenAI",
                "model-slug": "gpt-5"
            });
            assert_eq!(actual, expected);
        }

        Ok(())
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn notify_hook_sets_agent_event_env_vars() -> Result<()> {
        let tempdir = tempdir()?;
        let output_path = tempdir.path().join("agent-env.txt");
        let output_path_arg = output_path.display().to_string();
        let hook = notify_hook(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n' \"$CODEX_HOOK_EVENT\" \"$CODEX_HOOK_THREAD_ID\" \"$CODEX_HOOK_TURN_ID\" \"$CODEX_HOOK_PROVIDER_NAME\" \"$CODEX_HOOK_MODEL_SLUG\" \"${CODEX_HOOK_MEMORY_BINDING_KEY:-<unset>}\" \"${CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY:-<unset>}\" \"${CODEX_HOOK_MCP_STATUS:-<unset>}\" > \"$1\"".to_string(),
            "sh".to_string(),
            output_path_arg.clone(),
        ]);
        let thread_id =
            ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666").expect("valid thread id");
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from("/Users/example/project"),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterAgent {
                event: crate::HookEventAfterAgent {
                    thread_id,
                    turn_id: "turn-1".to_string(),
                    input_messages: vec!["hi".to_string()],
                    last_assistant_message: Some("done".to_string()),
                    provider_name: "Gemini".to_string(),
                    model_slug: "gemini-2.5-pro".to_string(),
                    memory: None,
                    memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    memory_scope_kind: Some("cwd".to_string()),
                    memory_summary_sha256: Some("a".repeat(64)),
                    memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                    memory_context: Some(crate::HookEventMemoryContext {
                        cwd_scope_key: "/Users/example/project".to_string(),
                        cwd_memory_root: "/Users/example/.codex/memories/cwd-bucket/memory"
                            .to_string(),
                        cwd_memory_summary_path:
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        cwd_memory_summary_exists: true,
                        user_memory_root: "/Users/example/.codex/memories/user/memory".to_string(),
                        user_memory_summary_path:
                            "/Users/example/.codex/memories/user/memory/memory_summary.md"
                                .to_string(),
                        user_memory_summary_exists: false,
                        active_scope_kind: Some("cwd".to_string()),
                        active_memory_root: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                        ),
                        active_memory_summary_path: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        ),
                        active_memory_summary_sha256: Some("a".repeat(64)),
                        active_memory_summary_bytes: Some(123),
                        active_memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                        active_memory_binding_key: Some(
                            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .to_string(),
                        ),
                    }),
                },
            },
        };

        assert!(matches!(
            hook.execute(&payload).await.result,
            HookResult::Success
        ));
        wait_for_file(&output_path).await;

        let actual = std::fs::read_to_string(&output_path)?;
        let lines: Vec<String> = actual.lines().map(ToOwned::to_owned).collect();
        assert_eq!(
            lines,
            vec![
                HOOK_EVENT_AGENT_TURN_COMPLETE.to_string(),
                thread_id.to_string(),
                "turn-1".to_string(),
                "Gemini".to_string(),
                "gemini-2.5-pro".to_string(),
                "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                "<unset>".to_string(),
            ]
        );

        Ok(())
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn notify_hook_sets_mcp_event_env_vars() -> Result<()> {
        let tempdir = tempdir()?;
        let output_path = tempdir.path().join("mcp-env.txt");
        let output_path_arg = output_path.display().to_string();
        let hook = notify_hook(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n' \"$CODEX_HOOK_EVENT\" \"$CODEX_HOOK_THREAD_ID\" \"$CODEX_HOOK_TURN_ID\" \"$CODEX_HOOK_PROVIDER_NAME\" \"$CODEX_HOOK_MODEL_SLUG\" \"$CODEX_HOOK_MCP_CALL_ID\" \"$CODEX_HOOK_MCP_SERVER\" \"$CODEX_HOOK_MCP_TOOL_NAME\" \"$CODEX_HOOK_MCP_STATUS\" \"$CODEX_HOOK_MCP_ERROR_MESSAGE\" \"${CODEX_HOOK_MEMORY_BINDING_KEY:-<unset>}\" \"${CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY:-<unset>}\" > \"$1\"".to_string(),
            "sh".to_string(),
            output_path_arg.clone(),
        ]);
        let thread_id =
            ThreadId::from_string("b5f6c1c2-1111-2222-3333-444455556666").expect("valid thread id");
        let payload = HookPayload {
            session_id: ThreadId::new(),
            cwd: PathBuf::from("/Users/example/project"),
            triggered_at: chrono::Utc::now(),
            hook_event: HookEvent::AfterMcpToolCall {
                event: crate::HookEventAfterMcpToolCall {
                    thread_id,
                    turn_id: "turn-2".to_string(),
                    call_id: "call-2".to_string(),
                    server: "claude-code".to_string(),
                    tool_name: "claude_code".to_string(),
                    duration_ms: 7,
                    status: crate::HookEventMcpToolCallStatus::TransportError,
                    error_message: Some("tool call error".to_string()),
                    provider_name: "OpenAI".to_string(),
                    model_slug: "gpt-5".to_string(),
                    agent_name: Some("claude-code".to_string()),
                    memory: None,
                    memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    memory_scope_kind: Some("cwd".to_string()),
                    memory_summary_sha256: Some("a".repeat(64)),
                    memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                    memory_context: Some(crate::HookEventMemoryContext {
                        cwd_scope_key: "/Users/example/project".to_string(),
                        cwd_memory_root: "/Users/example/.codex/memories/cwd-bucket/memory"
                            .to_string(),
                        cwd_memory_summary_path:
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        cwd_memory_summary_exists: true,
                        user_memory_root: "/Users/example/.codex/memories/user/memory".to_string(),
                        user_memory_summary_path:
                            "/Users/example/.codex/memories/user/memory/memory_summary.md"
                                .to_string(),
                        user_memory_summary_exists: false,
                        active_scope_kind: Some("cwd".to_string()),
                        active_memory_root: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory".to_string(),
                        ),
                        active_memory_summary_path: Some(
                            "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md"
                                .to_string(),
                        ),
                        active_memory_summary_sha256: Some("a".repeat(64)),
                        active_memory_summary_bytes: Some(123),
                        active_memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                        active_memory_binding_key: Some(
                            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .to_string(),
                        ),
                    }),
                },
            },
        };

        assert!(matches!(
            hook.execute(&payload).await.result,
            HookResult::Success
        ));
        wait_for_file(&output_path).await;

        let actual = std::fs::read_to_string(&output_path)?;
        let lines: Vec<String> = actual.lines().map(ToOwned::to_owned).collect();
        assert_eq!(
            lines,
            vec![
                HOOK_EVENT_MCP_TOOL_CALL_COMPLETE.to_string(),
                thread_id.to_string(),
                "turn-2".to_string(),
                "OpenAI".to_string(),
                "gpt-5".to_string(),
                "call-2".to_string(),
                "claude-code".to_string(),
                "claude_code".to_string(),
                "transport-error".to_string(),
                "tool call error".to_string(),
                "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
            ]
        );

        Ok(())
    }
}
