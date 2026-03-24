use crate::function_tool::FunctionCallError;
use crate::protocol::ReviewDecision;
use crate::sandboxing::SandboxPermissions;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::approvals::NetworkPolicyRuleAction;
use codex_protocol::models::FunctionCallOutputBody;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const TEMP_OVERRIDE_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SecurityPolicy {
    #[serde(default)]
    protected_zones: Vec<String>,
    #[serde(default)]
    temporary_overrides: Vec<String>,
    #[serde(default)]
    temporary_override_expirations: BTreeMap<String, i64>,
}

#[derive(Debug, Deserialize)]
struct RequestSecurityOverrideArgs {
    path: String,
    reason: String,
    sandbox_permissions: SandboxPermissions,
    justification: String,
}

pub struct RequestSecurityOverrideHandler;

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn normalize_path_for_policy(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    if let Some(parent) = path.parent()
        && let Ok(canonical_parent) = parent.canonicalize()
    {
        if let Some(name) = path.file_name() {
            return canonical_parent.join(name);
        }
        return canonical_parent;
    }

    path.to_path_buf()
}

fn path_is_within(path: &Path, prefix: &Path) -> bool {
    path == prefix || path.starts_with(prefix)
}

fn retain_active_overrides(policy: &mut SecurityPolicy, now: i64) {
    let expirations = policy.temporary_override_expirations.clone();
    policy.temporary_overrides.retain(|entry| {
        expirations
            .get(entry)
            .map(|expiry| *expiry > now)
            .unwrap_or(true)
    });
    policy
        .temporary_override_expirations
        .retain(|entry, expiry| *expiry > now && policy.temporary_overrides.contains(entry));
}

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
            session,
            turn,
            payload,
            call_id,
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

        if args.justification.trim().is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "justification is required when requesting a security override".to_string(),
            ));
        }
        if args.reason.trim().is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "reason is required when requesting a security override".to_string(),
            ));
        }

        if !matches!(
            turn.approval_policy.value(),
            codex_protocol::protocol::AskForApproval::OnRequest
        ) {
            let approval_policy = turn.approval_policy.value();
            return Err(FunctionCallError::RespondToModel(format!(
                "approval policy is {approval_policy:?}; reject request — you cannot ask for escalated permissions if the approval policy is {approval_policy:?}"
            )));
        }

        // 2. Absolute path validation and normalization.
        let requested_path = PathBuf::from(&args.path);
        if !requested_path.is_absolute() {
            return Err(FunctionCallError::RespondToModel(
                "path must be absolute".to_string(),
            ));
        }
        let normalized_path = normalize_path_for_policy(&requested_path);
        let path_str = normalized_path.to_string_lossy().to_string();

        let default_protected_zone = normalize_path_for_policy(&turn.cwd);
        let default_protected_zone_string = default_protected_zone.to_string_lossy().to_string();
        let policy_path = turn.config.codex_home.join("es_policy.json");
        let mut policy: SecurityPolicy = if let Ok(content) = fs::read_to_string(&policy_path) {
            serde_json::from_str(&content).unwrap_or_else(|_| SecurityPolicy {
                protected_zones: vec![default_protected_zone_string.clone()],
                temporary_overrides: Vec::new(),
                temporary_override_expirations: BTreeMap::new(),
            })
        } else {
            SecurityPolicy {
                protected_zones: vec![default_protected_zone_string.clone()],
                temporary_overrides: Vec::new(),
                temporary_override_expirations: BTreeMap::new(),
            }
        };
        if !policy.protected_zones.iter().any(|zone| {
            normalize_path_for_policy(Path::new(zone.as_str())) == default_protected_zone
        }) {
            policy
                .protected_zones
                .push(default_protected_zone_string.clone());
        }

        let in_protected_zone = policy.protected_zones.iter().any(|zone| {
            path_is_within(
                &normalized_path,
                &normalize_path_for_policy(Path::new(zone.as_str())),
            )
        });
        if !in_protected_zone {
            return Err(FunctionCallError::RespondToModel(format!(
                "requested path is not inside any protected zone: {path_str}"
            )));
        }

        let approval_reason = format!(
            "{} Reason: {}",
            args.justification.trim(),
            args.reason.trim()
        );
        let decision = session
            .request_command_approval(
                turn.as_ref(),
                call_id,
                None,
                vec![
                    "request_security_override".to_string(),
                    "--path".to_string(),
                    path_str.clone(),
                ],
                turn.cwd.clone(),
                Some(approval_reason),
                None,
                None,
                None,
                None,
            )
            .await;

        match decision {
            ReviewDecision::Approved
            | ReviewDecision::ApprovedForSession
            | ReviewDecision::ApprovedExecpolicyAmendment { .. } => {}
            ReviewDecision::NetworkPolicyAmendment {
                network_policy_amendment,
            } => {
                if matches!(
                    network_policy_amendment.action,
                    NetworkPolicyRuleAction::Deny
                ) {
                    return Err(FunctionCallError::RespondToModel(
                        "security override request was denied by the user".to_string(),
                    ));
                }
            }
            ReviewDecision::Denied => {
                return Err(FunctionCallError::RespondToModel(
                    "security override request was denied by the user".to_string(),
                ));
            }
            ReviewDecision::Abort => {
                return Err(FunctionCallError::RespondToModel(
                    "security override request was aborted by the user".to_string(),
                ));
            }
        }

        let now = current_unix_timestamp();
        retain_active_overrides(&mut policy, now);

        if !policy.temporary_overrides.contains(&path_str) {
            policy.temporary_overrides.push(path_str.clone());
        }
        policy
            .temporary_override_expirations
            .insert(path_str.clone(), now + TEMP_OVERRIDE_TTL_SECONDS);

        if let Some(parent) = policy_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "failed to create policy directory: {err}"
            )));
        }

        let policy_contents = serde_json::to_string_pretty(&policy).map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to serialize policy: {err}"))
        })?;
        if let Err(err) = fs::write(&policy_path, policy_contents) {
            return Err(FunctionCallError::RespondToModel(format!(
                "Failed to update es_policy.json: {err}"
            )));
        }

        let content = format!(
            "Success: The user approved. A temporary security override has been written for: {path_str} (expires in {TEMP_OVERRIDE_TTL_SECONDS} seconds)."
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(content),
            success: Some(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::make_session_and_context;
    use crate::protocol::AskForApproval;
    use crate::protocol::ReviewDecision;
    use crate::state::ActiveTurn;
    use crate::turn_diff_tracker::TurnDiffTracker;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    fn invocation(
        session: Arc<crate::codex::Session>,
        turn: Arc<crate::codex::TurnContext>,
        call_id: &str,
        args: serde_json::Value,
    ) -> ToolInvocation {
        ToolInvocation {
            session,
            turn,
            tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
            call_id: call_id.to_string(),
            tool_name: "request_security_override".to_string(),
            payload: ToolPayload::Function {
                arguments: args.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn rejects_relative_path() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "call-relative",
            json!({
                "path": "relative/path",
                "reason": "cleanup generated artifacts",
                "sandbox_permissions": "require_escalated",
                "justification": "Need temporary kernel override to remove files."
            }),
        );

        let err = match RequestSecurityOverrideHandler.handle(invocation).await {
            Ok(_) => panic!("relative path should be rejected"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel("path must be absolute".to_string())
        );
    }

    #[tokio::test]
    async fn rejects_empty_reason() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let target_path = turn.cwd.join("target-dir");
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "call-empty-reason",
            json!({
                "path": target_path,
                "reason": "   ",
                "sandbox_permissions": "require_escalated",
                "justification": "Need temporary kernel override to remove files."
            }),
        );

        let err = match RequestSecurityOverrideHandler.handle(invocation).await {
            Ok(_) => panic!("empty reason should be rejected"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "reason is required when requesting a security override".to_string()
            )
        );
    }

    #[tokio::test]
    async fn rejects_path_outside_protected_zone() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let outside_root = tempfile::tempdir().expect("create tempdir");
        let target_path = outside_root.path().join("outside-target");
        let invocation = invocation(
            Arc::new(session),
            Arc::new(turn),
            "call-outside",
            json!({
                "path": target_path,
                "reason": "cleanup generated artifacts",
                "sandbox_permissions": "require_escalated",
                "justification": "Need temporary kernel override to remove files."
            }),
        );

        let err = match RequestSecurityOverrideHandler.handle(invocation).await {
            Ok(_) => panic!("outside path should be rejected"),
            Err(err) => err,
        };
        let FunctionCallError::RespondToModel(message) = err else {
            panic!("expected model-visible validation error");
        };
        assert!(message.contains("requested path is not inside any protected zone"));
    }

    #[tokio::test]
    async fn returns_denied_when_user_rejects_approval() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let target_path = turn.cwd.join("target-dir");
        let session = Arc::new(session);
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        let turn = Arc::new(turn);
        let invocation = invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "call-denied",
            json!({
                "path": target_path,
                "reason": "cleanup generated artifacts",
                "sandbox_permissions": "require_escalated",
                "justification": "Need temporary kernel override to remove files."
            }),
        );
        let handler_task =
            tokio::spawn(async move { RequestSecurityOverrideHandler.handle(invocation).await });
        let notify_session = Arc::clone(&session);
        let notifier = tokio::spawn(async move {
            for _ in 0..60 {
                notify_session
                    .notify_approval("call-denied", ReviewDecision::Denied)
                    .await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let err = match tokio::time::timeout(Duration::from_secs(5), handler_task)
            .await
            .expect("handler should finish with denied approval")
            .expect("handler task should not panic")
        {
            Ok(_) => panic!("denied approval should return a model-visible error"),
            Err(err) => err,
        };
        notifier.abort();
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "security override request was denied by the user".to_string()
            )
        );
    }

    #[tokio::test]
    async fn writes_temporary_override_after_approval() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let target_path = turn.cwd.join("target-dir");
        let target_path = normalize_path_for_policy(&target_path);
        let target_path_string = target_path.to_string_lossy().to_string();

        let policy_path = turn.config.codex_home.join("es_policy.json");
        std::fs::create_dir_all(
            policy_path
                .parent()
                .expect("policy file should have parent directory"),
        )
        .expect("create policy dir");
        std::fs::write(
            &policy_path,
            r#"{"protected_zones":[],"temporary_overrides":[],"temporary_override_expirations":{}}"#,
        )
        .expect("write initial policy");

        let session = Arc::new(session);
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        let turn = Arc::new(turn);
        let invocation = invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "call-approved",
            json!({
                "path": target_path_string,
                "reason": "cleanup generated artifacts",
                "sandbox_permissions": "require_escalated",
                "justification": "Need temporary kernel override to remove files."
            }),
        );

        let handler_task =
            tokio::spawn(async move { RequestSecurityOverrideHandler.handle(invocation).await });
        let notify_session = Arc::clone(&session);
        let notifier = tokio::spawn(async move {
            for _ in 0..60 {
                notify_session
                    .notify_approval("call-approved", ReviewDecision::Approved)
                    .await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let output = tokio::time::timeout(Duration::from_secs(5), handler_task)
            .await
            .expect("handler should finish with approval")
            .expect("handler task should not panic")
            .expect("approval should succeed");
        notifier.abort();

        let ToolOutput::Function { body, success } = output else {
            panic!("expected function output");
        };
        assert_eq!(success, Some(true));
        let FunctionCallOutputBody::Text(message) = body else {
            panic!("expected plain-text output");
        };
        assert!(message.contains("expires in 900 seconds"));

        let policy: SecurityPolicy = serde_json::from_str(
            &std::fs::read_to_string(policy_path).expect("read updated policy"),
        )
        .expect("parse updated policy");
        assert!(policy.temporary_overrides.contains(&target_path_string));

        let expiry = policy
            .temporary_override_expirations
            .get(&target_path_string)
            .copied()
            .expect("expiry should be recorded");
        let now = current_unix_timestamp();
        assert!(expiry > now);
        assert!(expiry <= now + TEMP_OVERRIDE_TTL_SECONDS + 5);
    }
}
