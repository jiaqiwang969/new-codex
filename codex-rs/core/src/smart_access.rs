use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::config_types::SecurityMode;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GuardianAssessmentEvent;
use codex_protocol::protocol::GuardianAssessmentStatus;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::GuardianReviewResult;
use crate::guardian::guardian_assessment_action_value;
use crate::guardian::guardian_request_id;
use crate::guardian::guardian_request_turn_id;
use crate::guardian::review_approval_request_detailed;
use crate::security_host::SecurityArbitrationContext;
use crate::security_host::SecurityHost;
use crate::security_types::PredictedEffect;
use crate::security_types::PredictedEffectKind;
use crate::security_types::SecurityArbitrationDecision;
use crate::security_types::SecurityCapabilitySnapshot;
use crate::security_types::SecurityPermit;
use crate::security_types::SecurityPermitScope;

#[derive(Debug, Clone)]
pub(crate) enum SmartAccessApprovalOutcome {
    Final(ReviewDecision),
    FallbackToHuman { rationale: String },
}

#[derive(Debug, Deserialize, Default)]
struct EndpointSecurityPolicy {
    #[serde(default)]
    protected_zones: Vec<String>,
}

pub(crate) fn is_smart_access_mode(turn: &TurnContext) -> bool {
    turn.config.security_mode == SecurityMode::SmartAccess
}

pub(crate) fn merge_human_approval_reason(
    reason: Option<String>,
    smart_access_rationale: &str,
) -> Option<String> {
    if smart_access_rationale.trim().is_empty() {
        return reason;
    }

    let smart_access_reason = format!("Smart Access escalated: {smart_access_rationale}");
    match reason {
        Some(reason) if !reason.trim().is_empty() => {
            Some(format!("{reason}\n\n{smart_access_reason}"))
        }
        _ => Some(smart_access_reason),
    }
}

pub(crate) async fn review_smart_access_request(
    session: &Arc<Session>,
    turn: &Arc<TurnContext>,
    request: GuardianApprovalRequest,
    retry_reason: Option<String>,
) -> Option<SmartAccessApprovalOutcome> {
    if !is_smart_access_mode(turn) {
        return None;
    }

    let review =
        review_approval_request_detailed(session, turn, request.clone(), retry_reason).await;
    Some(arbitrate_smart_access_review(session.as_ref(), turn.as_ref(), &request, review).await)
}

async fn arbitrate_smart_access_review(
    session: &Session,
    turn: &TurnContext,
    request: &GuardianApprovalRequest,
    review: GuardianReviewResult,
) -> SmartAccessApprovalOutcome {
    match review.decision {
        ReviewDecision::Approved
        | ReviewDecision::ApprovedExecpolicyAmendment { .. }
        | ReviewDecision::ApprovedForSession
        | ReviewDecision::NetworkPolicyAmendment { .. } => {}
        ReviewDecision::Denied | ReviewDecision::Abort => {
            return SmartAccessApprovalOutcome::Final(review.decision);
        }
    }

    if review.predicted_effects.is_empty() {
        return SmartAccessApprovalOutcome::FallbackToHuman {
            rationale: review.rationale.clone(),
        };
    }

    let security_host = SecurityHost::new(build_capability_snapshot(turn));
    let arbitration = security_host.arbitrate(
        SecurityArbitrationContext {
            thread_id: session.conversation_id.to_string(),
            turn_id: turn.sub_id.clone(),
            risk_score: review.risk_score,
            rationale: review.rationale.clone(),
            issued_at: current_unix_timestamp(),
        },
        review.predicted_effects.clone(),
    );

    if matches!(
        arbitration,
        SecurityArbitrationDecision::AllowWithPermit { .. }
            | SecurityArbitrationDecision::AllowWithAmendedPermit { .. }
            | SecurityArbitrationDecision::Deny { .. }
    ) {
        emit_smart_access_trace_event(
            session,
            turn,
            request,
            smart_access_trace_status(&arbitration),
            smart_access_trace_rationale(&arbitration),
            smart_access_trace_action(
                guardian_assessment_action_value(request),
                &review,
                &arbitration,
            ),
        )
        .await;
    }

    match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { .. }
        | SecurityArbitrationDecision::AllowWithAmendedPermit { .. } => {
            SmartAccessApprovalOutcome::Final(ReviewDecision::Approved)
        }
        SecurityArbitrationDecision::Deny { .. } => {
            SmartAccessApprovalOutcome::Final(ReviewDecision::Denied)
        }
        SecurityArbitrationDecision::EscalateToHuman { rationale, .. }
        | SecurityArbitrationDecision::DowngradeToDefault { rationale } => {
            SmartAccessApprovalOutcome::FallbackToHuman { rationale }
        }
    }
}

fn build_capability_snapshot(turn: &TurnContext) -> SecurityCapabilitySnapshot {
    SecurityCapabilitySnapshot {
        protected_zones: load_protected_zones(turn),
        transfer_gate_enabled: turn.config.endpoint_security,
        ..Default::default()
    }
}

fn load_protected_zones(turn: &TurnContext) -> Vec<AbsolutePathBuf> {
    let policy_path = turn.config.codex_home.join("es_policy.json");
    if let Ok(contents) = fs::read_to_string(&policy_path)
        && let Ok(policy) = serde_json::from_str::<EndpointSecurityPolicy>(&contents)
    {
        let protected_zones = policy
            .protected_zones
            .into_iter()
            .filter_map(|zone| normalize_absolute_path(Path::new(zone.as_str())))
            .collect::<Vec<_>>();
        if !protected_zones.is_empty() {
            return protected_zones;
        }
    }

    normalize_absolute_path(turn.cwd.as_path())
        .into_iter()
        .collect()
}

fn normalize_absolute_path(path: &Path) -> Option<AbsolutePathBuf> {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    AbsolutePathBuf::try_from(normalized).ok()
}

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

async fn emit_smart_access_trace_event(
    session: &Session,
    turn: &TurnContext,
    request: &GuardianApprovalRequest,
    status: GuardianAssessmentStatus,
    rationale: Option<String>,
    action: JsonValue,
) {
    session
        .send_event(
            turn,
            EventMsg::GuardianAssessment(GuardianAssessmentEvent {
                id: format!("{}:smart-access", guardian_request_id(request)),
                turn_id: guardian_request_turn_id(request, &turn.sub_id).to_string(),
                status,
                risk_score: action
                    .get("smart_access")
                    .and_then(|trace| trace.get("risk_score"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|score| u8::try_from(score).ok()),
                risk_level: None,
                rationale,
                action: Some(action),
            }),
        )
        .await;
}

fn smart_access_trace_action(
    mut action: JsonValue,
    review: &GuardianReviewResult,
    arbitration: &SecurityArbitrationDecision,
) -> JsonValue {
    let smart_access = serde_json::json!({
        "risk_score": review.risk_score,
        "predicted_effects": review
            .predicted_effects
            .iter()
            .map(predicted_effect_summary)
            .collect::<Vec<_>>(),
        "decision": smart_access_decision_label(arbitration),
        "permit_summary": smart_access_permit_summary(arbitration),
        "mismatch_summary": JsonValue::Null,
    });
    attach_smart_access_trace(&mut action, smart_access);
    action
}

fn attach_smart_access_trace(action: &mut JsonValue, smart_access: JsonValue) {
    if let Some(action) = action.as_object_mut() {
        action.insert("smart_access".to_string(), smart_access);
        return;
    }

    *action = serde_json::json!({
        "action": action.clone(),
        "smart_access": smart_access,
    });
}

fn smart_access_trace_status(
    arbitration: &SecurityArbitrationDecision,
) -> GuardianAssessmentStatus {
    match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { .. }
        | SecurityArbitrationDecision::AllowWithAmendedPermit { .. } => {
            GuardianAssessmentStatus::Approved
        }
        SecurityArbitrationDecision::Deny { .. } => GuardianAssessmentStatus::Denied,
        SecurityArbitrationDecision::EscalateToHuman { .. }
        | SecurityArbitrationDecision::DowngradeToDefault { .. } => {
            GuardianAssessmentStatus::Aborted
        }
    }
}

fn smart_access_trace_rationale(arbitration: &SecurityArbitrationDecision) -> Option<String> {
    match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { .. } => None,
        SecurityArbitrationDecision::AllowWithAmendedPermit { rationale, .. }
        | SecurityArbitrationDecision::EscalateToHuman { rationale, .. }
        | SecurityArbitrationDecision::Deny { rationale, .. }
        | SecurityArbitrationDecision::DowngradeToDefault { rationale } => Some(rationale.clone()),
    }
}

fn smart_access_decision_label(arbitration: &SecurityArbitrationDecision) -> &'static str {
    match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { .. } => "allow_with_permit",
        SecurityArbitrationDecision::AllowWithAmendedPermit { .. } => "allow_with_amended_permit",
        SecurityArbitrationDecision::EscalateToHuman { .. } => "escalate_to_human",
        SecurityArbitrationDecision::Deny { .. } => "deny",
        SecurityArbitrationDecision::DowngradeToDefault { .. } => "downgrade_to_default",
    }
}

fn smart_access_permit_summary(arbitration: &SecurityArbitrationDecision) -> Option<String> {
    match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { permits }
        | SecurityArbitrationDecision::AllowWithAmendedPermit { permits, .. } => Some(
            permits
                .iter()
                .map(security_permit_summary)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        SecurityArbitrationDecision::EscalateToHuman { .. }
        | SecurityArbitrationDecision::Deny { .. }
        | SecurityArbitrationDecision::DowngradeToDefault { .. } => None,
    }
}

fn predicted_effect_summary(effect: &PredictedEffect) -> String {
    scope_effect_summary(effect.kind, &effect.scope)
}

fn security_permit_summary(permit: &SecurityPermit) -> String {
    let ttl_seconds = permit.expires_at.saturating_sub(permit.issued_at);
    format!(
        "{} ttl={ttl_seconds}s",
        scope_effect_summary(permit.kind, &permit.scope)
    )
}

fn scope_effect_summary(kind: PredictedEffectKind, scope: &SecurityPermitScope) -> String {
    match kind {
        PredictedEffectKind::ProtectedDelete => format!(
            "protected_delete:{}",
            display_path(scope.target_path.as_ref())
        ),
        PredictedEffectKind::ProtectedMoveOut => format!(
            "protected_move_out:{} -> {}",
            display_path(scope.source_path.as_ref()),
            display_path(scope.destination_path.as_ref())
        ),
        PredictedEffectKind::SensitiveRead => format!(
            "sensitive_read:{}",
            display_path(scope.target_path.as_ref())
        ),
        PredictedEffectKind::SensitiveTransferOut => format!(
            "sensitive_transfer_out:{} -> {}",
            display_path(scope.source_path.as_ref().or(scope.target_path.as_ref())),
            display_path(scope.destination_path.as_ref())
        ),
        PredictedEffectKind::TaintWriteOut => format!(
            "taint_write_out:{}",
            display_path(
                scope
                    .destination_path
                    .as_ref()
                    .or(scope.target_path.as_ref())
            )
        ),
        PredictedEffectKind::ExecExfilTool => format!(
            "exec_exfil_tool:{}",
            scope
                .process_name
                .as_deref()
                .or(scope.tool_name.as_deref())
                .unwrap_or("<unknown>")
        ),
        PredictedEffectKind::TrustedIdentityMismatch => format!(
            "trusted_identity_mismatch:{}",
            scope.trusted_identity.as_deref().unwrap_or("<unknown>")
        ),
    }
}

fn display_path(path: Option<&AbsolutePathBuf>) -> String {
    path.map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardian::GuardianReviewResult;
    use crate::security_types::PredictedEffect;
    use crate::security_types::PredictedEffectKind;
    use crate::security_types::SecurityPermit;
    use crate::security_types::SecurityPermitScope;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn smart_access_trace_action_includes_permit_summary_and_mismatch_placeholder() {
        let target_path = AbsolutePathBuf::try_from("/tmp/demo.txt").unwrap();
        let action = json!({
            "tool": "shell",
            "command": "rm -f /tmp/demo.txt",
        });
        let review = GuardianReviewResult {
            decision: ReviewDecision::Approved,
            risk_score: 14,
            rationale: "Single-file delete stays within the protected zone.".to_string(),
            predicted_effects: vec![PredictedEffect {
                kind: PredictedEffectKind::ProtectedDelete,
                scope: SecurityPermitScope {
                    target_path: Some(target_path.clone()),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("shell".to_string()),
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                confidence: 96,
                why: "Deletes one protected file.".to_string(),
            }],
        };
        let arbitration = SecurityArbitrationDecision::AllowWithPermit {
            permits: vec![SecurityPermit {
                id: "thread-1:turn-1:0".to_string(),
                kind: PredictedEffectKind::ProtectedDelete,
                scope: SecurityPermitScope {
                    target_path: Some(target_path),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("shell".to_string()),
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                issued_at: 1_710_000_000,
                expires_at: 1_710_000_120,
                issuer: "security-host".to_string(),
                risk_score: 14,
                justification: "Low-risk narrow smart-access permit.".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
            }],
        };

        assert_eq!(
            smart_access_trace_action(action, &review, &arbitration),
            json!({
                "tool": "shell",
                "command": "rm -f /tmp/demo.txt",
                "smart_access": {
                    "risk_score": 14,
                    "predicted_effects": ["protected_delete:/tmp/demo.txt"],
                    "decision": "allow_with_permit",
                    "permit_summary": "protected_delete:/tmp/demo.txt ttl=120s",
                    "mismatch_summary": null,
                }
            })
        );
    }
}
