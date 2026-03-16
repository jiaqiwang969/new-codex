use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_protocol::config_types::SecurityMode;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::GuardianReviewResult;
use crate::guardian::review_approval_request_detailed;
use crate::security_host::SecurityArbitrationContext;
use crate::security_host::SecurityHost;
use crate::security_types::SecurityArbitrationDecision;
use crate::security_types::SecurityCapabilitySnapshot;

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

    let review = review_approval_request_detailed(session, turn, request, retry_reason).await;
    Some(arbitrate_smart_access_review(
        session.as_ref(),
        turn.as_ref(),
        review,
    ))
}

fn arbitrate_smart_access_review(
    session: &Session,
    turn: &TurnContext,
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
            rationale: review.rationale,
        };
    }

    let security_host = SecurityHost::new(build_capability_snapshot(turn));
    let arbitration = security_host.arbitrate(
        SecurityArbitrationContext {
            thread_id: session.conversation_id.to_string(),
            turn_id: turn.sub_id.clone(),
            risk_score: review.risk_score,
            rationale: review.rationale,
            issued_at: current_unix_timestamp(),
        },
        review.predicted_effects,
    );

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
