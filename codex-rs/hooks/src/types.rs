use std::path::PathBuf;
use std::sync::Arc;

use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::MemoryLink;
use futures::future::BoxFuture;
use serde::Serialize;
use serde::Serializer;

pub type HookFn = Arc<dyn for<'a> Fn(&'a HookPayload) -> BoxFuture<'a, HookResult> + Send + Sync>;

#[derive(Debug)]
pub enum HookResult {
    /// Success: hook completed successfully.
    Success,
    /// FailedContinue: hook failed, but other subsequent hooks should still execute and the
    /// operation should continue.
    FailedContinue(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// FailedAbort: hook failed, other subsequent hooks should not execute, and the operation
    /// should be aborted.
    FailedAbort(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl HookResult {
    pub fn should_abort_operation(&self) -> bool {
        matches!(self, Self::FailedAbort(_))
    }
}

#[derive(Debug)]
pub struct HookResponse {
    pub hook_name: String,
    pub result: HookResult,
}

#[derive(Clone)]
pub struct Hook {
    pub name: String,
    pub func: HookFn,
}

impl Default for Hook {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            func: Arc::new(|_| Box::pin(async { HookResult::Success })),
        }
    }
}

impl Hook {
    pub async fn execute(&self, payload: &HookPayload) -> HookResponse {
        HookResponse {
            hook_name: self.name.clone(),
            result: (self.func)(payload).await,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct HookPayload {
    pub session_id: ThreadId,
    pub cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(serialize_with = "serialize_triggered_at")]
    pub triggered_at: DateTime<Utc>,
    pub hook_event: HookEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterAgent {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub input_messages: Vec<String>,
    pub last_assistant_message: Option<String>,
    pub provider_name: String,
    pub model_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_summary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_binding_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_context: Option<HookEventMemoryContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventMcpToolCallStatus {
    Ok,
    ToolError,
    TransportError,
    Declined,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterMcpToolCall {
    pub thread_id: ThreadId,
    pub turn_id: String,
    pub call_id: String,
    pub server: String,
    pub tool_name: String,
    pub duration_ms: u64,
    pub status: HookEventMcpToolCallStatus,
    pub error_message: Option<String>,
    pub provider_name: String,
    pub model_slug: String,
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_summary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_binding_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_context: Option<HookEventMemoryContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct HookEventMemoryContext {
    pub cwd_scope_key: String,
    pub cwd_memory_root: String,
    pub cwd_memory_summary_path: String,
    pub cwd_memory_summary_exists: bool,
    pub user_memory_root: String,
    pub user_memory_summary_path: String,
    pub user_memory_summary_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_scope_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_summary_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_summary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_summary_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_scope_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_memory_binding_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookToolKind {
    Function,
    Custom,
    LocalShell,
    Mcp,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookToolInputLocalShell {
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub sandbox_permissions: Option<SandboxPermissions>,
    pub prefix_rule: Option<Vec<String>>,
    pub justification: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "input_type", rename_all = "snake_case")]
pub enum HookToolInput {
    Function {
        arguments: String,
    },
    Custom {
        input: String,
    },
    LocalShell {
        params: HookToolInputLocalShell,
    },
    Mcp {
        server: String,
        tool: String,
        arguments: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct HookEventAfterToolUse {
    pub turn_id: String,
    pub call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryLink>,
    pub tool_name: String,
    pub tool_kind: HookToolKind,
    pub tool_input: HookToolInput,
    pub executed: bool,
    pub success: bool,
    pub duration_ms: u64,
    pub mutating: bool,
    pub sandbox: String,
    pub sandbox_policy: String,
    pub output_preview: String,
}

fn serialize_triggered_at<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum HookEvent {
    AfterAgent {
        #[serde(flatten)]
        event: HookEventAfterAgent,
    },
    AfterMcpToolCall {
        #[serde(flatten)]
        event: HookEventAfterMcpToolCall,
    },
    AfterToolUse {
        #[serde(flatten)]
        event: HookEventAfterToolUse,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeZone;
    use chrono::Utc;
    use codex_protocol::ThreadId;
    use codex_protocol::models::SandboxPermissions;
    use codex_protocol::protocol::MemoryLink;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::HookEvent;
    use super::HookEventAfterAgent;
    use super::HookEventAfterMcpToolCall;
    use super::HookEventAfterToolUse;
    use super::HookEventMcpToolCallStatus;
    use super::HookEventMemoryContext;
    use super::HookPayload;
    use super::HookToolInput;
    use super::HookToolInputLocalShell;
    use super::HookToolKind;

    #[test]
    fn hook_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let thread_id = ThreadId::new();
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterAgent {
                event: HookEventAfterAgent {
                    thread_id,
                    turn_id: "turn-1".to_string(),
                    input_messages: vec!["hello".to_string()],
                    last_assistant_message: Some("hi".to_string()),
                    provider_name: "Gemini".to_string(),
                    model_slug: "gemini-2.5-pro".to_string(),
                    memory: Some(MemoryLink {
                        scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                        scope_kind: Some("cwd".to_string()),
                        summary_sha256: Some("a".repeat(64)),
                        binding_key: Some(
                            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .to_string(),
                        ),
                    }),
                    memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    memory_scope_kind: Some("cwd".to_string()),
                    memory_summary_sha256: Some("a".repeat(64)),
                    memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                    memory_context: Some(HookEventMemoryContext {
                        cwd_scope_key: "/tmp/work".to_string(),
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

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "after_agent",
                "thread_id": thread_id.to_string(),
                "turn_id": "turn-1",
                "input_messages": ["hello"],
                "last_assistant_message": "hi",
                "provider_name": "Gemini",
                "model_slug": "gemini-2.5-pro",
                "memory": {
                    "scope_version": "cwd:aaaaaaaaaaaa",
                    "scope_kind": "cwd",
                    "summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                },
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_scope_kind": "cwd",
                "memory_summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "memory_context": {
                    "cwd_scope_key": "/tmp/work",
                    "cwd_memory_root": "/Users/example/.codex/memories/cwd-bucket/memory",
                    "cwd_memory_summary_path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                    "cwd_memory_summary_exists": true,
                    "user_memory_root": "/Users/example/.codex/memories/user/memory",
                    "user_memory_summary_path": "/Users/example/.codex/memories/user/memory/memory_summary.md",
                    "user_memory_summary_exists": false,
                    "active_scope_kind": "cwd",
                    "active_memory_root": "/Users/example/.codex/memories/cwd-bucket/memory",
                    "active_memory_summary_path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                    "active_memory_summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "active_memory_summary_bytes": 123,
                    "active_memory_scope_version": "cwd:aaaaaaaaaaaa",
                    "active_memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            },
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn mcp_hook_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let thread_id = ThreadId::new();
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterMcpToolCall {
                event: HookEventAfterMcpToolCall {
                    thread_id,
                    turn_id: "turn-1".to_string(),
                    call_id: "call-1".to_string(),
                    server: "claude-code".to_string(),
                    tool_name: "claude_code".to_string(),
                    duration_ms: 120,
                    status: HookEventMcpToolCallStatus::Ok,
                    error_message: None,
                    provider_name: "OpenAI".to_string(),
                    model_slug: "gpt-5".to_string(),
                    agent_name: Some("claude-code".to_string()),
                    memory: Some(MemoryLink {
                        scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                        scope_kind: Some("cwd".to_string()),
                        summary_sha256: Some("a".repeat(64)),
                        binding_key: Some(
                            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                .to_string(),
                        ),
                    }),
                    memory_scope_version: Some("cwd:aaaaaaaaaaaa".to_string()),
                    memory_scope_kind: Some("cwd".to_string()),
                    memory_summary_sha256: Some("a".repeat(64)),
                    memory_binding_key: Some(
                        "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_string(),
                    ),
                    memory_context: Some(HookEventMemoryContext {
                        cwd_scope_key: "/tmp/work".to_string(),
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

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "after_mcp_tool_call",
                "thread_id": thread_id.to_string(),
                "turn_id": "turn-1",
                "call_id": "call-1",
                "server": "claude-code",
                "tool_name": "claude_code",
                "duration_ms": 120,
                "status": "ok",
                "error_message": null,
                "provider_name": "OpenAI",
                "model_slug": "gpt-5",
                "agent_name": "claude-code",
                "memory": {
                    "scope_version": "cwd:aaaaaaaaaaaa",
                    "scope_kind": "cwd",
                    "summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                },
                "memory_scope_version": "cwd:aaaaaaaaaaaa",
                "memory_scope_kind": "cwd",
                "memory_summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "memory_context": {
                    "cwd_scope_key": "/tmp/work",
                    "cwd_memory_root": "/Users/example/.codex/memories/cwd-bucket/memory",
                    "cwd_memory_summary_path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                    "cwd_memory_summary_exists": true,
                    "user_memory_root": "/Users/example/.codex/memories/user/memory",
                    "user_memory_summary_path": "/Users/example/.codex/memories/user/memory/memory_summary.md",
                    "user_memory_summary_exists": false,
                    "active_scope_kind": "cwd",
                    "active_memory_root": "/Users/example/.codex/memories/cwd-bucket/memory",
                    "active_memory_summary_path": "/Users/example/.codex/memories/cwd-bucket/memory/memory_summary.md",
                    "active_memory_summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "active_memory_summary_bytes": 123,
                    "active_memory_scope_version": "cwd:aaaaaaaaaaaa",
                    "active_memory_binding_key": "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            },
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn after_tool_use_payload_serializes_stable_wire_shape() {
        let session_id = ThreadId::new();
        let payload = HookPayload {
            session_id,
            cwd: PathBuf::from("tmp"),
            client: None,
            triggered_at: Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
                .single()
                .expect("valid timestamp"),
            hook_event: HookEvent::AfterToolUse {
                event: HookEventAfterToolUse {
                    turn_id: "turn-2".to_string(),
                    call_id: "call-1".to_string(),
                    memory: Some(MemoryLink {
                        scope_version: Some("user:bbbbbbbbbbbb".to_string()),
                        scope_kind: Some("user".to_string()),
                        summary_sha256: Some("b".repeat(64)),
                        binding_key: Some(
                            "user:bbbbbbbbbbbb:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                                .to_string(),
                        ),
                    }),
                    tool_name: "local_shell".to_string(),
                    tool_kind: HookToolKind::LocalShell,
                    tool_input: HookToolInput::LocalShell {
                        params: HookToolInputLocalShell {
                            command: vec!["cargo".to_string(), "fmt".to_string()],
                            workdir: Some("codex-rs".to_string()),
                            timeout_ms: Some(60_000),
                            sandbox_permissions: Some(SandboxPermissions::UseDefault),
                            justification: None,
                            prefix_rule: None,
                        },
                    },
                    executed: true,
                    success: true,
                    duration_ms: 42,
                    mutating: true,
                    sandbox: "none".to_string(),
                    sandbox_policy: "danger-full-access".to_string(),
                    output_preview: "ok".to_string(),
                },
            },
        };

        let actual = serde_json::to_value(payload).expect("serialize hook payload");
        let expected = json!({
            "session_id": session_id.to_string(),
            "cwd": "tmp",
            "triggered_at": "2025-01-01T00:00:00Z",
            "hook_event": {
                "event_type": "after_tool_use",
                "turn_id": "turn-2",
                "call_id": "call-1",
                "memory": {
                    "scope_version": "user:bbbbbbbbbbbb",
                    "scope_kind": "user",
                    "summary_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "binding_key": "user:bbbbbbbbbbbb:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                },
                "tool_name": "local_shell",
                "tool_kind": "local_shell",
                "tool_input": {
                    "input_type": "local_shell",
                    "params": {
                        "command": ["cargo", "fmt"],
                        "workdir": "codex-rs",
                        "timeout_ms": 60000,
                        "sandbox_permissions": "use_default",
                        "justification": null,
                        "prefix_rule": null,
                    },
                },
                "executed": true,
                "success": true,
                "duration_ms": 42,
                "mutating": true,
                "sandbox": "none",
                "sandbox_policy": "danger-full-access",
                "output_preview": "ok",
            },
        });

        assert_eq!(actual, expected);
    }
}
