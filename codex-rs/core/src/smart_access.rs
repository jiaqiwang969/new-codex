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
use crate::exec::ExecToolCallOutput;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::GuardianReviewResult;
use crate::guardian::guardian_assessment_action_value;
use crate::guardian::guardian_request_id;
use crate::guardian::guardian_request_turn_id;
use crate::guardian::review_approval_request_detailed;
use crate::security_host::RuntimeDeniedEffect;
use crate::security_host::SecurityArbitrationContext;
use crate::security_host::SecurityHost;
use crate::security_types::PredictedEffect;
use crate::security_types::PredictedEffectKind;
use crate::security_types::SecurityArbitrationDecision;
use crate::security_types::SecurityCapabilitySnapshot;
use crate::security_types::SecurityMismatch;
use crate::security_types::SecurityMismatchClassification;
use crate::security_types::SecurityPermit;
use crate::security_types::SecurityPermitScope;

#[derive(Debug, Clone)]
pub(crate) enum SmartAccessApprovalOutcome {
    Final(ReviewDecision),
    FallbackToHuman { rationale: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SmartAccessRuntimeContext {
    pub(crate) predicted_effects: Vec<PredictedEffect>,
    pub(crate) permits: Vec<SecurityPermit>,
}

#[derive(Debug, Clone)]
struct ParsedRuntimeMismatch {
    actual_kind: PredictedEffectKind,
    actual_scope: SecurityPermitScope,
    actual_reason_code: String,
    process_name: Option<String>,
    ancestor_name: Option<String>,
    summary: String,
}

const REASON_SENSITIVE_READ_NON_AI: &str = "SENSITIVE_READ_NON_AI";
const REASON_SENSITIVE_TRANSFER_OUT: &str = "SENSITIVE_TRANSFER_OUT";
const REASON_TAINT_WRITE_OUT: &str = "TAINT_WRITE_OUT";
const REASON_EXEC_EXFIL_TOOL: &str = "EXEC_EXFIL_TOOL";
const REASON_PROTECTED_ZONE_AI_DELETE: &str = "PROTECTED_ZONE_AI_DELETE";
const REASON_TRUST_IDENTITY_MISMATCH: &str = "TRUST_IDENTITY_MISMATCH";
const ENDPOINT_SECURITY_DAEMON_LOG_PATH: &str = "/tmp/codex-es-daemon.log";
const ENDPOINT_SECURITY_INTERVENTION_WARNING: &str = "\n\n[SYSTEM SECURITY INTERVENTION]: The background Kernel Security Daemon blocked this operation:\n";
const ENDPOINT_SECURITY_OVERRIDE_GUIDANCE: &str = "\nTo bypass this, you MUST use the `request_security_override` tool with `sandbox_permissions: \"require_escalated\"`.";

#[derive(Debug, Deserialize)]
struct EndpointSecurityPolicy {
    #[serde(default)]
    protected_zones: Vec<String>,
    #[serde(default)]
    sensitive_zones: Vec<String>,
    #[serde(default)]
    sensitive_export_allow_zones: Vec<String>,
    #[serde(default)]
    exec_exfil_tool_blocklist: Vec<String>,
    #[serde(default)]
    trusted_tools: Vec<String>,
    #[serde(default)]
    trusted_tool_identities: Vec<EndpointSecurityTrustedToolIdentity>,
    #[serde(default = "default_true")]
    read_gate_enabled: bool,
    #[serde(default = "default_true")]
    transfer_gate_enabled: bool,
    #[serde(default = "default_true")]
    exec_gate_enabled: bool,
    #[serde(default = "default_true")]
    allow_vcs_metadata_in_ai_context: bool,
    #[serde(default = "default_true")]
    allow_git_merge_pull_in_ai_context: bool,
    #[serde(default = "default_taint_ttl_seconds")]
    taint_ttl_seconds: u64,
}

impl Default for EndpointSecurityPolicy {
    fn default() -> Self {
        Self {
            protected_zones: Vec::new(),
            sensitive_zones: Vec::new(),
            sensitive_export_allow_zones: Vec::new(),
            exec_exfil_tool_blocklist: Vec::new(),
            trusted_tools: Vec::new(),
            trusted_tool_identities: Vec::new(),
            read_gate_enabled: default_true(),
            transfer_gate_enabled: default_true(),
            exec_gate_enabled: default_true(),
            allow_vcs_metadata_in_ai_context: default_true(),
            allow_git_merge_pull_in_ai_context: default_true(),
            taint_ttl_seconds: default_taint_ttl_seconds(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct EndpointSecurityTrustedToolIdentity {
    path: String,
    signing_identifier: String,
    #[serde(default)]
    team_identifier: Option<String>,
    #[serde(default)]
    cdhash: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_taint_ttl_seconds() -> u64 {
    600
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

pub(crate) async fn emit_runtime_mismatch_trace_event(
    session: &Session,
    turn: &TurnContext,
    request_id: &str,
    trace_id: &str,
    action: JsonValue,
    mismatch_summary: &str,
) {
    let runtime_context = load_runtime_context(session, request_id).await;
    let mismatch = build_runtime_mismatch(turn, runtime_context.as_ref(), mismatch_summary);
    send_smart_access_trace_event(
        session,
        turn,
        trace_id.to_string(),
        turn.sub_id.clone(),
        GuardianAssessmentStatus::Denied,
        Some("Endpoint Security blocked the operation at runtime.".to_string()),
        runtime_mismatch_trace_action(action, runtime_context.as_ref(), mismatch_summary, mismatch),
    )
    .await;
}

pub(crate) fn endpoint_security_daemon_log_size() -> u64 {
    fs::metadata(ENDPOINT_SECURITY_DAEMON_LOG_PATH)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

pub(crate) async fn maybe_record_endpoint_security_runtime_mismatch(
    session: &Session,
    turn: &TurnContext,
    request_id: &str,
    trace_id: &str,
    action: JsonValue,
    exec_output: &mut ExecToolCallOutput,
    daemon_log_offset: u64,
) {
    let smart_access_mode = is_smart_access_mode(turn);
    if exec_output.exit_code == 0 {
        if smart_access_mode {
            clear_runtime_context(session, request_id).await;
        }
        return;
    }

    let Some(new_logs) = read_endpoint_security_daemon_logs(daemon_log_offset) else {
        if smart_access_mode {
            clear_runtime_context(session, request_id).await;
        }
        return;
    };
    let Some(mismatch_summary) = endpoint_security_intervention_summary(&new_logs) else {
        if smart_access_mode {
            clear_runtime_context(session, request_id).await;
        }
        return;
    };

    append_endpoint_security_intervention_warning(exec_output, new_logs.trim());
    if smart_access_mode {
        emit_runtime_mismatch_trace_event(
            session,
            turn,
            request_id,
            trace_id,
            action,
            &mismatch_summary,
        )
        .await;
        clear_runtime_context(session, request_id).await;
    }
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
    persist_runtime_context(session, request, &review.predicted_effects, &arbitration).await;

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
    let protected_zones = fallback_protected_zones(turn);
    let policy = load_endpoint_security_policy(turn).unwrap_or_default();

    let protected_zones = {
        let zones = collect_absolute_paths(policy.protected_zones);
        if zones.is_empty() {
            protected_zones
        } else {
            zones
        }
    };

    SecurityCapabilitySnapshot {
        protected_zones,
        sensitive_zones: collect_absolute_paths(policy.sensitive_zones),
        sensitive_export_allow_zones: collect_absolute_paths(policy.sensitive_export_allow_zones),
        exec_exfil_tool_blocklist: policy.exec_exfil_tool_blocklist,
        trusted_tools: policy.trusted_tools,
        trusted_tool_identities: policy
            .trusted_tool_identities
            .into_iter()
            .map(|identity| {
                format!(
                    "{}|{}|{}|{}",
                    identity.path,
                    identity.signing_identifier,
                    identity.team_identifier.unwrap_or_else(|| "*".to_string()),
                    identity.cdhash.unwrap_or_else(|| "*".to_string())
                )
            })
            .collect(),
        taint_ttl_seconds: policy.taint_ttl_seconds,
        read_gate_enabled: turn.config.endpoint_security && policy.read_gate_enabled,
        transfer_gate_enabled: turn.config.endpoint_security && policy.transfer_gate_enabled,
        exec_gate_enabled: turn.config.endpoint_security && policy.exec_gate_enabled,
        allow_vcs_metadata_in_ai_context: policy.allow_vcs_metadata_in_ai_context,
        allow_git_merge_pull_in_ai_context: policy.allow_git_merge_pull_in_ai_context,
    }
}

fn load_endpoint_security_policy(turn: &TurnContext) -> Option<EndpointSecurityPolicy> {
    let policy_path = turn.config.codex_home.join("es_policy.json");
    fs::read_to_string(&policy_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<EndpointSecurityPolicy>(&contents).ok())
}

fn fallback_protected_zones(turn: &TurnContext) -> Vec<AbsolutePathBuf> {
    normalize_absolute_path(turn.cwd.as_path())
        .into_iter()
        .collect()
}

fn collect_absolute_paths(paths: Vec<String>) -> Vec<AbsolutePathBuf> {
    paths
        .into_iter()
        .filter_map(|path| normalize_absolute_path(Path::new(path.as_str())))
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

fn read_endpoint_security_daemon_logs(offset: u64) -> Option<String> {
    let mut file = std::fs::File::open(ENDPOINT_SECURITY_DAEMON_LOG_PATH).ok()?;
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut new_logs = String::new();
    file.read_to_string(&mut new_logs).ok()?;
    (!new_logs.trim().is_empty()).then_some(new_logs)
}

fn append_endpoint_security_intervention_warning(
    exec_output: &mut ExecToolCallOutput,
    daemon_logs: &str,
) {
    let warning = format!(
        "{ENDPOINT_SECURITY_INTERVENTION_WARNING}{daemon_logs}{ENDPOINT_SECURITY_OVERRIDE_GUIDANCE}"
    );
    exec_output.stderr.text.push_str(&warning);
    exec_output.aggregated_output.text.push_str(&warning);
}

pub(crate) fn endpoint_security_intervention_summary(new_logs: &str) -> Option<String> {
    let trimmed = new_logs.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("[AGENTSMITH DENIED]") {
        return Some(trimmed.to_string());
    }

    new_logs
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .find(|line| line.contains("Blocked") || line.contains("blocked"))
        .map(ToOwned::to_owned)
}

async fn persist_runtime_context(
    session: &Session,
    request: &GuardianApprovalRequest,
    predicted_effects: &[PredictedEffect],
    arbitration: &SecurityArbitrationDecision,
) {
    let permits = match arbitration {
        SecurityArbitrationDecision::AllowWithPermit { permits }
        | SecurityArbitrationDecision::AllowWithAmendedPermit { permits, .. } => permits.clone(),
        SecurityArbitrationDecision::EscalateToHuman { .. }
        | SecurityArbitrationDecision::Deny { .. }
        | SecurityArbitrationDecision::DowngradeToDefault { .. } => Vec::new(),
    };

    if predicted_effects.is_empty() || permits.is_empty() {
        return;
    }

    let mut store = session.services.smart_access_runtime_contexts.lock().await;
    store.insert(
        guardian_request_id(request).to_string(),
        SmartAccessRuntimeContext {
            predicted_effects: predicted_effects.to_vec(),
            permits,
        },
    );
}

async fn load_runtime_context(
    session: &Session,
    request_id: &str,
) -> Option<SmartAccessRuntimeContext> {
    let mut store = session.services.smart_access_runtime_contexts.lock().await;
    store.remove(request_id)
}

pub(crate) async fn clear_runtime_context(session: &Session, request_id: &str) {
    let mut store = session.services.smart_access_runtime_contexts.lock().await;
    store.remove(request_id);
}

fn build_runtime_mismatch(
    turn: &TurnContext,
    runtime_context: Option<&SmartAccessRuntimeContext>,
    mismatch_summary: &str,
) -> Option<SecurityMismatch> {
    let parsed = parse_runtime_mismatch(mismatch_summary)?;
    let predicted_effects = runtime_context
        .map(|context| context.predicted_effects.clone())
        .unwrap_or_default();
    let permit = runtime_context.and_then(|context| {
        select_runtime_permit(context, parsed.actual_kind, &parsed.actual_scope)
    });
    let security_host = SecurityHost::new(build_capability_snapshot(turn));
    let mut mismatch = security_host.runtime_mismatch_for_denial(
        permit,
        predicted_effects,
        RuntimeDeniedEffect {
            actual_kind: parsed.actual_kind,
            actual_scope: parsed.actual_scope.clone(),
            process_name: parsed.process_name.clone(),
            ancestor_name: parsed.ancestor_name.clone(),
            summary: parsed.summary.clone(),
        },
    );
    mismatch.actual_reason_code = parsed.actual_reason_code;
    mismatch.classification = security_host.classify_mismatch(&mismatch);
    Some(mismatch)
}

fn select_runtime_permit<'a>(
    runtime_context: &'a SmartAccessRuntimeContext,
    actual_kind: PredictedEffectKind,
    actual_scope: &SecurityPermitScope,
) -> Option<&'a SecurityPermit> {
    runtime_context
        .permits
        .iter()
        .find(|permit| {
            permit.kind == actual_kind && permit_scope_matches(&permit.scope, actual_scope)
        })
        .or_else(|| {
            runtime_context
                .permits
                .iter()
                .find(|permit| permit.kind == actual_kind)
        })
}

fn permit_scope_matches(expected: &SecurityPermitScope, actual: &SecurityPermitScope) -> bool {
    path_scope_matches(expected.target_path.as_ref(), actual.target_path.as_ref())
        && path_scope_matches(expected.source_path.as_ref(), actual.source_path.as_ref())
        && path_scope_matches(
            expected.destination_path.as_ref(),
            actual.destination_path.as_ref(),
        )
}

fn path_scope_matches(
    expected: Option<&AbsolutePathBuf>,
    actual: Option<&AbsolutePathBuf>,
) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => expected == actual,
        (None, None) => true,
        _ => false,
    }
}

fn parse_runtime_mismatch(summary: &str) -> Option<ParsedRuntimeMismatch> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return None;
    }

    let blocked_move_out = parse_blocked_move_out(trimmed);
    let blocked_delete = parse_blocked_delete(trimmed);
    let path = extract_runtime_field(trimmed, "Path:")
        .or_else(|| {
            blocked_move_out
                .as_ref()
                .map(|(source_path, _)| source_path.clone())
        })
        .or_else(|| blocked_delete.clone());
    let destination = extract_runtime_field(trimmed, "Dest:").or_else(|| {
        blocked_move_out
            .as_ref()
            .map(|(_, destination_path)| destination_path.clone())
    });
    let (process_name, ancestor_name) = extract_process_details(trimmed);
    let actual_reason_code = extract_runtime_field(trimmed, "Reason:")
        .or_else(|| find_runtime_reason_code(trimmed).map(ToOwned::to_owned))
        .or_else(|| {
            (blocked_move_out.is_some() || blocked_delete.is_some())
                .then(|| REASON_PROTECTED_ZONE_AI_DELETE.to_string())
        })?;

    let actual_kind = match actual_reason_code.as_str() {
        REASON_SENSITIVE_READ_NON_AI => PredictedEffectKind::SensitiveRead,
        REASON_SENSITIVE_TRANSFER_OUT => PredictedEffectKind::SensitiveTransferOut,
        REASON_TAINT_WRITE_OUT => PredictedEffectKind::TaintWriteOut,
        REASON_EXEC_EXFIL_TOOL => PredictedEffectKind::ExecExfilTool,
        REASON_PROTECTED_ZONE_AI_DELETE => {
            if destination.is_some() {
                PredictedEffectKind::ProtectedMoveOut
            } else {
                PredictedEffectKind::ProtectedDelete
            }
        }
        REASON_TRUST_IDENTITY_MISMATCH => PredictedEffectKind::TrustedIdentityMismatch,
        _ => return None,
    };

    let actual_scope = match actual_kind {
        PredictedEffectKind::ProtectedDelete | PredictedEffectKind::SensitiveRead => {
            SecurityPermitScope {
                target_path: path
                    .as_deref()
                    .and_then(|path| normalize_absolute_path(Path::new(path))),
                source_path: None,
                destination_path: None,
                tool_name: None,
                process_name: process_name.clone(),
                trusted_identity: None,
                recursive: false,
            }
        }
        PredictedEffectKind::ProtectedMoveOut | PredictedEffectKind::SensitiveTransferOut => {
            SecurityPermitScope {
                target_path: None,
                source_path: path
                    .as_deref()
                    .and_then(|path| normalize_absolute_path(Path::new(path))),
                destination_path: destination
                    .as_deref()
                    .and_then(|path| normalize_absolute_path(Path::new(path))),
                tool_name: None,
                process_name: process_name.clone(),
                trusted_identity: None,
                recursive: false,
            }
        }
        PredictedEffectKind::TaintWriteOut => SecurityPermitScope {
            target_path: path
                .as_deref()
                .and_then(|path| normalize_absolute_path(Path::new(path))),
            source_path: None,
            destination_path: destination
                .as_deref()
                .and_then(|path| normalize_absolute_path(Path::new(path))),
            tool_name: None,
            process_name: process_name.clone(),
            trusted_identity: None,
            recursive: false,
        },
        PredictedEffectKind::ExecExfilTool => SecurityPermitScope {
            target_path: None,
            source_path: None,
            destination_path: None,
            tool_name: None,
            process_name: process_name.clone(),
            trusted_identity: None,
            recursive: false,
        },
        PredictedEffectKind::TrustedIdentityMismatch => SecurityPermitScope {
            target_path: None,
            source_path: None,
            destination_path: None,
            tool_name: None,
            process_name: process_name.clone(),
            trusted_identity: process_name.clone(),
            recursive: false,
        },
    };

    Some(ParsedRuntimeMismatch {
        actual_kind,
        actual_scope,
        actual_reason_code,
        process_name,
        ancestor_name,
        summary: summary.to_string(),
    })
}

fn extract_runtime_field(summary: &str, prefix: &str) -> Option<String> {
    summary.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix)?.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn find_runtime_reason_code(summary: &str) -> Option<&'static str> {
    [
        REASON_SENSITIVE_READ_NON_AI,
        REASON_SENSITIVE_TRANSFER_OUT,
        REASON_TAINT_WRITE_OUT,
        REASON_EXEC_EXFIL_TOOL,
        REASON_PROTECTED_ZONE_AI_DELETE,
        REASON_TRUST_IDENTITY_MISMATCH,
    ]
    .into_iter()
    .find(|reason_code| summary.contains(reason_code))
}

fn extract_process_details(summary: &str) -> (Option<String>, Option<String>) {
    let Some(process) = extract_runtime_field(summary, "Process:") else {
        return (None, None);
    };

    if let Some((process_name, ancestor_name)) = process.split_once(" (via ") {
        let ancestor_name = ancestor_name.trim_end_matches(')');
        return (
            normalize_runtime_detail(process_name),
            normalize_runtime_detail(ancestor_name),
        );
    }

    (normalize_runtime_detail(process.as_str()), None)
}

fn normalize_runtime_detail(detail: &str) -> Option<String> {
    let trimmed = detail.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("n/a")
        || trimmed.eq_ignore_ascii_case("none")
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn parse_blocked_delete(summary: &str) -> Option<String> {
    summary.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("[Codex ES Daemon] Blocked protected delete: ")
            .or_else(|| {
                line.trim()
                    .strip_prefix("[Codex ES Daemon] Blocked physical deletion of protected path: ")
            })
            .map(|path| path.trim().to_string())
    })
}

fn parse_blocked_move_out(summary: &str) -> Option<(String, String)> {
    summary.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("[Codex ES Daemon] Blocked move out of protected zone: ")
            .and_then(|paths| {
                paths
                    .split_once(" -> ")
                    .map(|(source_path, destination_path)| {
                        (
                            source_path.trim().to_string(),
                            destination_path.trim().to_string(),
                        )
                    })
            })
    })
}

async fn emit_smart_access_trace_event(
    session: &Session,
    turn: &TurnContext,
    request: &GuardianApprovalRequest,
    status: GuardianAssessmentStatus,
    rationale: Option<String>,
    action: JsonValue,
) {
    send_smart_access_trace_event(
        session,
        turn,
        format!("{}:smart-access", guardian_request_id(request)),
        guardian_request_turn_id(request, &turn.sub_id).to_string(),
        status,
        rationale,
        action,
    )
    .await;
}

async fn send_smart_access_trace_event(
    session: &Session,
    turn: &TurnContext,
    id: String,
    turn_id: String,
    status: GuardianAssessmentStatus,
    rationale: Option<String>,
    action: JsonValue,
) {
    session
        .send_event(
            turn,
            EventMsg::GuardianAssessment(GuardianAssessmentEvent {
                id,
                turn_id,
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

fn runtime_mismatch_trace_action(
    mut action: JsonValue,
    runtime_context: Option<&SmartAccessRuntimeContext>,
    mismatch_summary: &str,
    mismatch: Option<SecurityMismatch>,
) -> JsonValue {
    let smart_access = serde_json::json!({
        "risk_score": JsonValue::Null,
        "predicted_effects": runtime_context
            .map(|context| {
                context
                    .predicted_effects
                    .iter()
                    .map(predicted_effect_summary)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        "decision": "runtime_mismatch",
        "permit_summary": runtime_context.and_then(runtime_context_permit_summary),
        "mismatch_summary": mismatch_summary,
        "mismatch_reason_code": mismatch.as_ref().map(|mismatch| mismatch.actual_reason_code.clone()),
        "mismatch_classification": mismatch
            .as_ref()
            .map(|mismatch| mismatch_classification_label(mismatch.classification)),
        "actual_effect": mismatch
            .as_ref()
            .map(|mismatch| scope_effect_summary(mismatch.actual_kind, &mismatch.actual_scope)),
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

fn runtime_context_permit_summary(runtime_context: &SmartAccessRuntimeContext) -> Option<String> {
    if runtime_context.permits.is_empty() {
        return None;
    }
    Some(
        runtime_context
            .permits
            .iter()
            .map(security_permit_summary)
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn mismatch_classification_label(classification: SecurityMismatchClassification) -> &'static str {
    match classification {
        SecurityMismatchClassification::TrueRisk => "true_risk",
        SecurityMismatchClassification::Underpredicted => "underpredicted",
        SecurityMismatchClassification::PolicyDrift => "policy_drift",
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
    use crate::codex::make_session_and_context;
    use crate::guardian::GuardianReviewResult;
    use crate::security_types::PredictedEffect;
    use crate::security_types::PredictedEffectKind;
    use crate::security_types::SecurityArbitrationDecision;
    use crate::security_types::SecurityCapabilitySnapshot;
    use crate::security_types::SecurityMismatch;
    use crate::security_types::SecurityMismatchClassification;
    use crate::security_types::SecurityPermit;
    use crate::security_types::SecurityPermitScope;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

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

    #[test]
    fn endpoint_security_intervention_summary_keeps_agentsmith_denial_block() {
        let summary = concat!(
            "[AGENTSMITH DENIED]\n",
            "Operation: unlink\n",
            "Reason: PROTECTED_ZONE_AI_DELETE\n",
            "Path: /tmp/demo.txt\n",
            "Zone: /tmp\n",
            "Process: rm (via codex)\n",
        );

        assert_eq!(
            endpoint_security_intervention_summary(summary),
            Some(summary.trim().to_string())
        );
    }

    #[tokio::test]
    async fn build_capability_snapshot_reads_extended_endpoint_security_policy() {
        let (_session, mut turn) = make_session_and_context().await;
        let codex_home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let protected_zone = workspace.path().join("protected");
        let sensitive_zone = workspace.path().join("sensitive");
        let export_allow_zone = workspace.path().join("exports");
        fs::create_dir_all(&protected_zone).unwrap();
        fs::create_dir_all(&sensitive_zone).unwrap();
        fs::create_dir_all(&export_allow_zone).unwrap();
        fs::write(
            codex_home.path().join("es_policy.json"),
            serde_json::to_vec_pretty(&json!({
                "protected_zones": [protected_zone],
                "sensitive_zones": [sensitive_zone],
                "sensitive_export_allow_zones": [export_allow_zone],
                "trusted_tools": ["git", "cargo"],
                "trusted_tool_identities": [{
                    "path": "/usr/bin/git",
                    "signing_identifier": "com.apple.git",
                    "team_identifier": "APPLE",
                    "cdhash": "deadbeef"
                }],
                "exec_exfil_tool_blocklist": ["curl", "scp"],
                "read_gate_enabled": true,
                "transfer_gate_enabled": false,
                "exec_gate_enabled": true,
                "taint_ttl_seconds": 42,
                "allow_vcs_metadata_in_ai_context": false,
                "allow_git_merge_pull_in_ai_context": false
            }))
            .unwrap(),
        )
        .unwrap();

        let mut config = (*turn.config).clone();
        config.codex_home = codex_home.path().to_path_buf();
        config.endpoint_security = true;
        turn.config = Arc::new(config);
        turn.cwd = workspace.path().to_path_buf();

        assert_eq!(
            build_capability_snapshot(&turn),
            SecurityCapabilitySnapshot {
                protected_zones: vec![
                    AbsolutePathBuf::try_from(protected_zone.canonicalize().unwrap()).unwrap(),
                ],
                sensitive_zones: vec![
                    AbsolutePathBuf::try_from(sensitive_zone.canonicalize().unwrap()).unwrap(),
                ],
                sensitive_export_allow_zones: vec![
                    AbsolutePathBuf::try_from(export_allow_zone.canonicalize().unwrap()).unwrap(),
                ],
                exec_exfil_tool_blocklist: vec!["curl".to_string(), "scp".to_string()],
                trusted_tools: vec!["git".to_string(), "cargo".to_string()],
                trusted_tool_identities: vec![
                    "/usr/bin/git|com.apple.git|APPLE|deadbeef".to_string()
                ],
                taint_ttl_seconds: 42,
                read_gate_enabled: true,
                transfer_gate_enabled: false,
                exec_gate_enabled: true,
                allow_vcs_metadata_in_ai_context: false,
                allow_git_merge_pull_in_ai_context: false,
            }
        );
    }

    #[tokio::test]
    async fn build_capability_snapshot_without_policy_uses_endpoint_security_defaults() {
        let (_session, mut turn) = make_session_and_context().await;
        let codex_home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let workspace_abs =
            AbsolutePathBuf::try_from(workspace.path().canonicalize().unwrap()).unwrap();

        let mut config = (*turn.config).clone();
        config.codex_home = codex_home.path().to_path_buf();
        config.endpoint_security = true;
        turn.config = Arc::new(config);
        turn.cwd = workspace.path().to_path_buf();

        assert_eq!(
            build_capability_snapshot(&turn),
            SecurityCapabilitySnapshot {
                protected_zones: vec![workspace_abs],
                taint_ttl_seconds: 600,
                read_gate_enabled: true,
                transfer_gate_enabled: true,
                exec_gate_enabled: true,
                allow_vcs_metadata_in_ai_context: true,
                allow_git_merge_pull_in_ai_context: true,
                ..Default::default()
            }
        );
    }

    #[tokio::test]
    async fn build_runtime_mismatch_parses_agentsmith_delete_denial_with_runtime_context() {
        let (_session, mut turn) = make_session_and_context().await;
        let workspace = tempdir().unwrap();
        let protected_dir = workspace.path().join("protected");
        let protected_file = protected_dir.join("report.txt");
        fs::create_dir_all(&protected_dir).unwrap();
        fs::write(&protected_file, "demo").unwrap();
        turn.cwd = workspace.path().to_path_buf();

        let protected_file_abs =
            AbsolutePathBuf::try_from(protected_file.canonicalize().unwrap()).unwrap();
        let runtime_context = SmartAccessRuntimeContext {
            predicted_effects: vec![PredictedEffect {
                kind: PredictedEffectKind::ProtectedDelete,
                scope: SecurityPermitScope {
                    target_path: Some(protected_file_abs.clone()),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("exec_command".to_string()),
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                confidence: 97,
                why: "Deletes one protected file.".to_string(),
            }],
            permits: vec![SecurityPermit {
                id: "thread-1:turn-1:0".to_string(),
                kind: PredictedEffectKind::ProtectedDelete,
                scope: SecurityPermitScope {
                    target_path: Some(protected_file_abs.clone()),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("exec_command".to_string()),
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                issued_at: 1_710_000_000,
                expires_at: 1_710_000_120,
                issuer: "security-host".to_string(),
                risk_score: 12,
                justification: "Low-risk narrow smart-access permit.".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
            }],
        };
        let summary = format!(
            "[AGENTSMITH DENIED]\nOperation: unlink\nReason: PROTECTED_ZONE_AI_DELETE\nPath: {}\nZone: {}\nProcess: rm (via codex)\n",
            protected_file.display(),
            protected_dir.display()
        );

        assert_eq!(
            build_runtime_mismatch(&turn, Some(&runtime_context), &summary),
            Some(SecurityMismatch {
                permit_id: Some("thread-1:turn-1:0".to_string()),
                predicted_effects: runtime_context.predicted_effects.clone(),
                actual_kind: PredictedEffectKind::ProtectedDelete,
                actual_reason_code: "PROTECTED_ZONE_AI_DELETE".to_string(),
                actual_scope: SecurityPermitScope {
                    target_path: Some(protected_file_abs),
                    source_path: None,
                    destination_path: None,
                    tool_name: None,
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                classification: SecurityMismatchClassification::Underpredicted,
                process_name: Some("rm".to_string()),
                ancestor_name: Some("codex".to_string()),
                summary,
            })
        );
    }

    #[tokio::test]
    async fn load_runtime_context_consumes_saved_request_context() {
        let (session, _turn) = make_session_and_context().await;
        let workspace = tempdir().unwrap();
        let protected_dir = workspace.path().join("protected");
        let protected_file = protected_dir.join("report.txt");
        fs::create_dir_all(&protected_dir).unwrap();
        fs::write(&protected_file, "demo").unwrap();

        let protected_file_abs =
            AbsolutePathBuf::try_from(protected_file.canonicalize().unwrap()).unwrap();
        let predicted_effects = vec![PredictedEffect {
            kind: PredictedEffectKind::ProtectedDelete,
            scope: SecurityPermitScope {
                target_path: Some(protected_file_abs.clone()),
                source_path: None,
                destination_path: None,
                tool_name: Some("apply_patch".to_string()),
                process_name: Some("apply_patch".to_string()),
                trusted_identity: None,
                recursive: false,
            },
            confidence: 98,
            why: "Deletes one protected file.".to_string(),
        }];
        let permits = vec![SecurityPermit {
            id: "thread-1:turn-1:0".to_string(),
            kind: PredictedEffectKind::ProtectedDelete,
            scope: SecurityPermitScope {
                target_path: Some(protected_file_abs),
                source_path: None,
                destination_path: None,
                tool_name: Some("apply_patch".to_string()),
                process_name: Some("apply_patch".to_string()),
                trusted_identity: None,
                recursive: false,
            },
            issued_at: 1_710_000_000,
            expires_at: 1_710_000_120,
            issuer: "security-host".to_string(),
            risk_score: 11,
            justification: "Low-risk narrow smart-access permit.".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        }];
        let request = GuardianApprovalRequest::ApplyPatch {
            id: "req-1".to_string(),
            cwd: workspace.path().to_path_buf(),
            files: Vec::new(),
            change_count: 1,
            patch: "*** Begin Patch\n*** End Patch\n".to_string(),
        };

        persist_runtime_context(
            &session,
            &request,
            &predicted_effects,
            &SecurityArbitrationDecision::AllowWithPermit {
                permits: permits.clone(),
            },
        )
        .await;

        let first = load_runtime_context(&session, guardian_request_id(&request))
            .await
            .expect("expected saved runtime context");
        assert_eq!(first.predicted_effects, predicted_effects);
        assert_eq!(first.permits, permits);
        assert!(
            load_runtime_context(&session, guardian_request_id(&request))
                .await
                .is_none(),
            "expected runtime context to be consumed on first load"
        );
    }

    #[tokio::test]
    async fn persist_runtime_context_skips_denied_arbitration() {
        let (session, _turn) = make_session_and_context().await;
        let workspace = tempdir().unwrap();
        let protected_file = workspace.path().join("report.txt");
        fs::write(&protected_file, "demo").unwrap();
        let protected_file_abs =
            AbsolutePathBuf::try_from(protected_file.canonicalize().unwrap()).unwrap();
        let predicted_effects = vec![PredictedEffect {
            kind: PredictedEffectKind::ProtectedDelete,
            scope: SecurityPermitScope {
                target_path: Some(protected_file_abs),
                source_path: None,
                destination_path: None,
                tool_name: Some("apply_patch".to_string()),
                process_name: Some("apply_patch".to_string()),
                trusted_identity: None,
                recursive: false,
            },
            confidence: 98,
            why: "Deletes one protected file.".to_string(),
        }];
        let request = GuardianApprovalRequest::ApplyPatch {
            id: "req-denied".to_string(),
            cwd: workspace.path().to_path_buf(),
            files: Vec::new(),
            change_count: 1,
            patch: "*** Begin Patch\n*** End Patch\n".to_string(),
        };

        persist_runtime_context(
            &session,
            &request,
            &predicted_effects,
            &SecurityArbitrationDecision::Deny {
                risk_score: 99,
                rationale: "manual review required".to_string(),
            },
        )
        .await;

        assert!(
            load_runtime_context(&session, guardian_request_id(&request))
                .await
                .is_none(),
            "denied actions should not leave behind runtime context"
        );
    }
}
