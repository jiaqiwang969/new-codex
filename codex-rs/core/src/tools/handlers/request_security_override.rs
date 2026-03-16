use crate::function_tool::FunctionCallError;
use crate::protocol::ReviewDecision;
use crate::sandboxing::SandboxPermissions;
use crate::security_host::SecurityArbitrationContext;
use crate::security_host::SecurityHost;
use crate::security_types::PredictedEffect;
use crate::security_types::PredictedEffectKind;
use crate::security_types::SecurityArbitrationDecision;
use crate::security_types::SecurityCapabilitySnapshot;
use crate::security_types::SecurityPermitScope;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::approvals::NetworkPolicyRuleAction;
use codex_protocol::models::FunctionCallOutputBody;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const TEMP_OVERRIDE_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
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

fn absolute_path_buf(path: &Path) -> Result<AbsolutePathBuf, FunctionCallError> {
    AbsolutePathBuf::try_from(path.to_path_buf()).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to normalize security override path: {err}"
        ))
    })
}

fn security_host_decision_for_override(
    session: &crate::codex::Session,
    turn: &crate::codex::TurnContext,
    policy: &SecurityPolicy,
    normalized_path: &Path,
    args: &RequestSecurityOverrideArgs,
) -> Result<SecurityArbitrationDecision, FunctionCallError> {
    let capability_snapshot = SecurityCapabilitySnapshot {
        protected_zones: policy
            .protected_zones
            .iter()
            .map(|zone| normalize_path_for_policy(Path::new(zone.as_str())))
            .filter_map(|zone| AbsolutePathBuf::try_from(zone).ok())
            .collect(),
        transfer_gate_enabled: turn.config.endpoint_security,
        ..Default::default()
    };
    let security_host = SecurityHost::new(capability_snapshot);
    let request_text = format!(
        "{} {}",
        args.justification.trim().to_lowercase(),
        args.reason.trim().to_lowercase()
    );
    let request_tokens = request_text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let effect = if ["move", "trash", "rename", "desktop"]
        .into_iter()
        .any(|keyword| request_tokens.contains(&keyword))
    {
        let file_name = normalized_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "override-target".to_string());
        PredictedEffect {
            kind: PredictedEffectKind::ProtectedMoveOut,
            scope: SecurityPermitScope {
                target_path: None,
                source_path: Some(absolute_path_buf(normalized_path)?),
                destination_path: Some(absolute_path_buf(Path::new("/tmp").join(file_name).as_path())?),
                tool_name: Some("request_security_override".to_string()),
                process_name: Some("mv".to_string()),
                trusted_identity: None,
                recursive: normalized_path.is_dir(),
            },
            confidence: 88,
            why: "Legacy endpoint override request anticipates moving protected data out of the zone."
                .to_string(),
        }
    } else {
        PredictedEffect {
            kind: PredictedEffectKind::ProtectedDelete,
            scope: SecurityPermitScope {
                target_path: Some(absolute_path_buf(normalized_path)?),
                source_path: None,
                destination_path: None,
                tool_name: Some("request_security_override".to_string()),
                process_name: Some("rm".to_string()),
                trusted_identity: None,
                recursive: normalized_path.is_dir(),
            },
            confidence: 92,
            why: "Legacy endpoint override request anticipates deleting a protected path."
                .to_string(),
        }
    };

    Ok(security_host.arbitrate(
        SecurityArbitrationContext {
            thread_id: session.conversation_id.to_string(),
            turn_id: turn.sub_id.clone(),
            risk_score: 35,
            rationale: format!(
                "Legacy endpoint override request for {}",
                normalized_path.display()
            ),
            issued_at: current_unix_timestamp(),
        },
        vec![effect],
    ))
}

fn ensure_legacy_override_is_compatible(
    decision: &SecurityArbitrationDecision,
) -> Result<(), FunctionCallError> {
    match decision {
        SecurityArbitrationDecision::AllowWithPermit { .. } => Ok(()),
        SecurityArbitrationDecision::AllowWithAmendedPermit { rationale, .. } => {
            Err(FunctionCallError::RespondToModel(format!(
                "security override request needs a narrower scoped permit than the legacy override file can express: {rationale}"
            )))
        }
        SecurityArbitrationDecision::EscalateToHuman { rationale, .. } => {
            Err(FunctionCallError::RespondToModel(format!(
                "security override request requires Smart Access human escalation before issuing a legacy override: {rationale}"
            )))
        }
        SecurityArbitrationDecision::Deny { rationale, .. } => {
            Err(FunctionCallError::RespondToModel(format!(
                "security override request was denied by Security Host: {rationale}"
            )))
        }
        SecurityArbitrationDecision::DowngradeToDefault { rationale } => {
            Err(FunctionCallError::RespondToModel(format!(
                "security override request cannot be represented by the current endpoint security override flow: {rationale}"
            )))
        }
    }
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

        let security_host_decision = security_host_decision_for_override(
            session.as_ref(),
            turn.as_ref(),
            &policy,
            &normalized_path,
            &args,
        )?;
        ensure_legacy_override_is_compatible(&security_host_decision)?;

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
    use std::time::SystemTime;
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

    #[test]
    fn legacy_override_compatibility_rejects_non_direct_security_host_decisions() {
        assert_eq!(
            ensure_legacy_override_is_compatible(
                &SecurityArbitrationDecision::AllowWithAmendedPermit {
                    permits: Vec::new(),
                    rationale: "narrower scope required".to_string(),
                }
            ),
            Err(FunctionCallError::RespondToModel(
                "security override request needs a narrower scoped permit than the legacy override file can express: narrower scope required".to_string()
            ))
        );
        assert_eq!(
            ensure_legacy_override_is_compatible(&SecurityArbitrationDecision::EscalateToHuman {
                risk_score: 81,
                rationale: "manual review required".to_string(),
            }),
            Err(FunctionCallError::RespondToModel(
                "security override request requires Smart Access human escalation before issuing a legacy override: manual review required".to_string()
            ))
        );
        assert_eq!(
            ensure_legacy_override_is_compatible(&SecurityArbitrationDecision::Deny {
                risk_score: 96,
                rationale: "crosses trust boundary".to_string(),
            }),
            Err(FunctionCallError::RespondToModel(
                "security override request was denied by Security Host: crosses trust boundary"
                    .to_string()
            ))
        );
        assert_eq!(
            ensure_legacy_override_is_compatible(
                &SecurityArbitrationDecision::DowngradeToDefault {
                    rationale: "runtime gate unavailable".to_string(),
                }
            ),
            Err(FunctionCallError::RespondToModel(
                "security override request cannot be represented by the current endpoint security override flow: runtime gate unavailable".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn does_not_write_override_when_security_host_downgrades_move_request() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let target_path = turn.cwd.join("move-target.txt");
        let policy_path = turn.config.codex_home.join("es_policy.json");
        std::fs::create_dir_all(
            policy_path
                .parent()
                .expect("policy file should have parent directory"),
        )
        .expect("create policy dir");
        let initial_policy = SecurityPolicy {
            protected_zones: vec![
                normalize_path_for_policy(&turn.cwd)
                    .to_string_lossy()
                    .to_string(),
            ],
            temporary_overrides: Vec::new(),
            temporary_override_expirations: BTreeMap::new(),
        };
        std::fs::write(
            &policy_path,
            serde_json::to_string_pretty(&initial_policy).expect("serialize initial policy"),
        )
        .expect("write initial policy");

        let session = Arc::new(session);
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        let turn = Arc::new(turn);
        let invocation = invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "call-downgraded",
            json!({
                "path": target_path,
                "reason": "Move the protected file to Trash so the task can proceed.",
                "sandbox_permissions": "require_escalated",
                "justification": "Need a temporary kernel override for this move."
            }),
        );

        let err = tokio::time::timeout(
            Duration::from_secs(1),
            RequestSecurityOverrideHandler.handle(invocation),
        )
        .await
        .expect("security host downgrade should return without waiting for human approval");
        let err = match err {
            Ok(_) => panic!("downgraded override should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "security override request cannot be represented by the current endpoint security override flow: Smart Access cannot trust the current runtime state for this effect.".to_string()
            )
        );
        assert_eq!(
            serde_json::from_str::<SecurityPolicy>(
                &std::fs::read_to_string(policy_path).expect("read policy after downgrade")
            )
            .expect("parse policy after downgrade"),
            initial_policy
        );
    }

    #[tokio::test]
    async fn does_not_write_override_when_security_host_requires_narrower_scope() {
        let (session, mut turn) = make_session_and_context().await;
        turn.approval_policy
            .set(AskForApproval::OnRequest)
            .expect("approval policy should allow OnRequest in tests");
        let target_path = turn.cwd.join(format!(
            "override-dir-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&target_path).expect("create override directory");

        let policy_path = turn.config.codex_home.join("es_policy.json");
        std::fs::create_dir_all(
            policy_path
                .parent()
                .expect("policy file should have parent directory"),
        )
        .expect("create policy dir");
        let initial_policy = SecurityPolicy {
            protected_zones: vec![
                normalize_path_for_policy(&turn.cwd)
                    .to_string_lossy()
                    .to_string(),
            ],
            temporary_overrides: Vec::new(),
            temporary_override_expirations: BTreeMap::new(),
        };
        std::fs::write(
            &policy_path,
            serde_json::to_string_pretty(&initial_policy).expect("serialize initial policy"),
        )
        .expect("write initial policy");

        let session = Arc::new(session);
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        let turn = Arc::new(turn);
        let invocation = invocation(
            Arc::clone(&session),
            Arc::clone(&turn),
            "call-amended",
            json!({
                "path": target_path,
                "reason": "Delete the generated directory once verification is complete.",
                "sandbox_permissions": "require_escalated",
                "justification": "Need a temporary kernel override to clean up this directory."
            }),
        );

        let err = tokio::time::timeout(
            Duration::from_secs(1),
            RequestSecurityOverrideHandler.handle(invocation),
        )
        .await
        .expect("amended scope should return without waiting for human approval");
        let err = match err {
            Ok(_) => panic!("amended scope should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            FunctionCallError::RespondToModel(
                "security override request needs a narrower scoped permit than the legacy override file can express: Security Host narrowed a recursive request before approval.".to_string()
            )
        );
        assert_eq!(
            serde_json::from_str::<SecurityPolicy>(
                &std::fs::read_to_string(policy_path).expect("read policy after amended denial")
            )
            .expect("parse policy after amended denial"),
            initial_policy
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
