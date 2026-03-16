use super::SecurityArbitrationContext;
use super::SecurityHost;
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

fn base_snapshot() -> SecurityCapabilitySnapshot {
    SecurityCapabilitySnapshot {
        protected_zones: vec![AbsolutePathBuf::try_from("/Users/demo/Documents").unwrap()],
        sensitive_zones: vec![AbsolutePathBuf::try_from("/Users/demo/.ssh").unwrap()],
        sensitive_export_allow_zones: vec![AbsolutePathBuf::try_from("/tmp").unwrap()],
        exec_exfil_tool_blocklist: vec!["curl".to_string(), "scp".to_string()],
        trusted_tools: vec!["shell".to_string(), "apply_patch".to_string()],
        trusted_tool_identities: vec!["apple.codesign:Terminal".to_string()],
        taint_ttl_seconds: 900,
        read_gate_enabled: true,
        transfer_gate_enabled: true,
        exec_gate_enabled: true,
        allow_vcs_metadata_in_ai_context: true,
        allow_git_merge_pull_in_ai_context: false,
    }
}

fn base_context(risk_score: u8) -> SecurityArbitrationContext {
    SecurityArbitrationContext {
        thread_id: "thread-123".to_string(),
        turn_id: "turn-456".to_string(),
        risk_score,
        rationale: "Guardian classified the action as low risk.".to_string(),
        issued_at: 1_710_000_000,
    }
}

#[test]
fn security_host_narrow_protected_delete_allows_with_permit() {
    let host = SecurityHost::new(base_snapshot());
    let effect = PredictedEffect {
        kind: PredictedEffectKind::ProtectedDelete,
        scope: SecurityPermitScope {
            target_path: Some(
                AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
            ),
            source_path: None,
            destination_path: None,
            tool_name: Some("shell".to_string()),
            process_name: Some("rm".to_string()),
            trusted_identity: Some("apple.codesign:Terminal".to_string()),
            recursive: false,
        },
        confidence: 96,
        why: "Deletes one file under a protected zone.".to_string(),
    };

    let decision = host.arbitrate(base_context(18), vec![effect.clone()]);

    assert_eq!(
        decision,
        SecurityArbitrationDecision::AllowWithPermit {
            permits: vec![SecurityPermit {
                id: "thread-123:turn-456:0".to_string(),
                kind: PredictedEffectKind::ProtectedDelete,
                scope: effect.scope,
                issued_at: 1_710_000_000,
                expires_at: 1_710_000_120,
                issuer: "security-host".to_string(),
                risk_score: 18,
                justification: "Low-risk narrow smart-access permit.".to_string(),
                thread_id: "thread-123".to_string(),
                turn_id: "turn-456".to_string(),
            }]
        }
    );
}

#[test]
fn security_host_can_narrow_permit_scope() {
    let host = SecurityHost::new(base_snapshot());

    let decision = host.arbitrate(
        base_context(22),
        vec![PredictedEffect {
            kind: PredictedEffectKind::ProtectedDelete,
            scope: SecurityPermitScope {
                target_path: Some(
                    AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
                ),
                source_path: None,
                destination_path: None,
                tool_name: Some("shell".to_string()),
                process_name: Some("rm".to_string()),
                trusted_identity: Some("apple.codesign:Terminal".to_string()),
                recursive: true,
            },
            confidence: 91,
            why: "Deletes one file but asked for recursive scope.".to_string(),
        }],
    );

    assert_eq!(
        decision,
        SecurityArbitrationDecision::AllowWithAmendedPermit {
            permits: vec![SecurityPermit {
                id: "thread-123:turn-456:0".to_string(),
                kind: PredictedEffectKind::ProtectedDelete,
                scope: SecurityPermitScope {
                    target_path: Some(
                        AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
                    ),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("shell".to_string()),
                    process_name: Some("rm".to_string()),
                    trusted_identity: Some("apple.codesign:Terminal".to_string()),
                    recursive: false,
                },
                issued_at: 1_710_000_000,
                expires_at: 1_710_000_120,
                issuer: "security-host".to_string(),
                risk_score: 22,
                justification: "Security Host narrowed a broad scope into a precise permit."
                    .to_string(),
                thread_id: "thread-123".to_string(),
                turn_id: "turn-456".to_string(),
            }],
            rationale: "Security Host narrowed a recursive request before approval.".to_string(),
        }
    );
}

#[test]
fn security_host_sensitive_transfer_out_escalates_to_human() {
    let host = SecurityHost::new(base_snapshot());

    let decision = host.arbitrate(
        base_context(74),
        vec![PredictedEffect {
            kind: PredictedEffectKind::SensitiveTransferOut,
            scope: SecurityPermitScope {
                target_path: Some(AbsolutePathBuf::try_from("/Users/demo/.ssh/id_rsa").unwrap()),
                source_path: Some(AbsolutePathBuf::try_from("/Users/demo/.ssh/id_rsa").unwrap()),
                destination_path: Some(AbsolutePathBuf::try_from("/tmp/id_rsa").unwrap()),
                tool_name: Some("mcp".to_string()),
                process_name: Some("tar".to_string()),
                trusted_identity: None,
                recursive: false,
            },
            confidence: 88,
            why: "Moves sensitive material outside the protected source zone.".to_string(),
        }],
    );

    assert_eq!(
        decision,
        SecurityArbitrationDecision::EscalateToHuman {
            risk_score: 74,
            rationale: "Sensitive transfers require explicit human approval.".to_string(),
        }
    );
}

#[test]
fn security_host_explicit_high_risk_mismatch_is_true_risk() {
    let host = SecurityHost::new(base_snapshot());
    let mismatch = SecurityMismatch {
        permit_id: None,
        predicted_effects: Vec::new(),
        actual_kind: PredictedEffectKind::ProtectedMoveOut,
        actual_reason_code: "es_protected_move_out".to_string(),
        actual_scope: SecurityPermitScope {
            target_path: Some(
                AbsolutePathBuf::try_from("/Users/demo/Documents/secrets.txt").unwrap(),
            ),
            source_path: Some(
                AbsolutePathBuf::try_from("/Users/demo/Documents/secrets.txt").unwrap(),
            ),
            destination_path: Some(
                AbsolutePathBuf::try_from("/Users/demo/Desktop/secrets.txt").unwrap(),
            ),
            tool_name: Some("shell".to_string()),
            process_name: Some("mv".to_string()),
            trusted_identity: Some("unsigned:python".to_string()),
            recursive: false,
        },
        classification: SecurityMismatchClassification::Underpredicted,
        process_name: Some("mv".to_string()),
        ancestor_name: Some("python".to_string()),
        summary: "Protected data left the protected zone.".to_string(),
    };

    assert_eq!(
        host.classify_mismatch(&mismatch),
        SecurityMismatchClassification::TrueRisk
    );
}

#[test]
fn security_host_trust_uncertainty_downgrades_to_default() {
    let mut snapshot = base_snapshot();
    snapshot.trusted_tool_identities.clear();
    let host = SecurityHost::new(snapshot);

    let decision = host.arbitrate(
        base_context(31),
        vec![PredictedEffect {
            kind: PredictedEffectKind::ExecExfilTool,
            scope: SecurityPermitScope {
                target_path: None,
                source_path: None,
                destination_path: Some(AbsolutePathBuf::try_from("/tmp/archive.tgz").unwrap()),
                tool_name: Some("shell".to_string()),
                process_name: Some("curl".to_string()),
                trusted_identity: None,
                recursive: false,
            },
            confidence: 70,
            why: "Uses a known exfiltration tool without a trusted identity baseline.".to_string(),
        }],
    );

    assert_eq!(
        decision,
        SecurityArbitrationDecision::DowngradeToDefault {
            rationale: "Smart Access cannot trust the current runtime state for this effect."
                .to_string(),
        }
    );
}
