use crate::function_tool::FunctionCallError;
use crate::sandboxing::SandboxPermissions;
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::registry::{ToolHandler, ToolKind};
use async_trait::async_trait;
use codex_protocol::models::FunctionCallOutputBody;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SecurityPolicy {
    #[serde(default)]
    protected_zones: Vec<String>,
    #[serde(default)]
    temporary_overrides: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RequestSecurityOverrideArgs {
    path: String,
    reason: String,
    sandbox_permissions: SandboxPermissions,
    justification: Option<String>,
}

pub struct RequestSecurityOverrideHandler;

#[async_trait]
impl ToolHandler for RequestSecurityOverrideHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        // This modifies the kernel daemon JSON, so it requires explicit escalation.
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            turn,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "unsupported payload".to_string(),
                ));
            }
        };

        let args: RequestSecurityOverrideArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("Failed to parse arguments: {e}"))
        })?;

        // 1. Check if the AI actually asked the human for permission via `require_escalated`
        if !args.sandbox_permissions.requires_escalated_permissions() {
            return Err(FunctionCallError::RespondToModel(
                "You MUST request escalated permissions (sandbox_permissions: 'require_escalated') and provide a justification to the user to bypass the Kernel Endpoint Security Daemon.".to_string(),
            ));
        }

        // 2. Absolute Path Validation
        let requested_path = PathBuf::from(&args.path);
        let normalized_path = turn.resolve_path(Some(args.path.clone()));
        let path_str = normalized_path.to_string_lossy().to_string();

        let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        // Absolute sanity check: never let the AI un-protect the entire HOME or root
        if path_str == home_dir || path_str == "/" || path_str.starts_with("/System") || path_str == "/Users" {
            return Err(FunctionCallError::RespondToModel(
                "Kernel Daemon Rejected: Overriding security for the entire HOME directory, /Users, or /System is strictly forbidden. Request a more specific sub-directory.".to_string()
            ));
        }

        // 3. Read existing policy, append to temporary_overrides, and write back
        let policy_path = format!("{}/.codex/es_policy.json", home_dir);
        let mut policy: SecurityPolicy = if let Ok(content) = fs::read_to_string(&policy_path) {
            serde_json::from_str(&content).unwrap_or_else(|_| SecurityPolicy {
                protected_zones: vec![turn.cwd.to_string_lossy().to_string()],
                temporary_overrides: vec![],
            })
        } else {
            SecurityPolicy {
                protected_zones: vec![turn.cwd.to_string_lossy().to_string()],
                temporary_overrides: vec![],
            }
        };

        if !policy.temporary_overrides.contains(&path_str) {
            policy.temporary_overrides.push(path_str.clone());
        }

        if let Err(e) = fs::write(&policy_path, serde_json::to_string_pretty(&policy).unwrap()) {
             return Err(FunctionCallError::RespondToModel(format!("Failed to update es_policy.json: {}", e)));
        }

        let content = format!(
            "Success: The user approved. A temporary security override has been written for: {}. The macOS Kernel will now allow deletions here. Please proceed with caution.",
            path_str
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }
}
