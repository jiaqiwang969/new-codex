use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;

/// Runtime capability digest that teaches Smart Access what the local security
/// stack can actually enforce.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SecurityCapabilitySnapshot {
    pub protected_zones: Vec<AbsolutePathBuf>,
    pub sensitive_zones: Vec<AbsolutePathBuf>,
    pub sensitive_export_allow_zones: Vec<AbsolutePathBuf>,
    pub exec_exfil_tool_blocklist: Vec<String>,
    pub trusted_tools: Vec<String>,
    pub trusted_tool_identities: Vec<String>,
    pub taint_ttl_seconds: u64,
    pub read_gate_enabled: bool,
    pub transfer_gate_enabled: bool,
    pub exec_gate_enabled: bool,
    pub allow_vcs_metadata_in_ai_context: bool,
    pub allow_git_merge_pull_in_ai_context: bool,
}

/// Narrow scope attached to predicted effects, permits, and runtime mismatch
/// reports.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SecurityPermitScope {
    pub target_path: Option<AbsolutePathBuf>,
    pub source_path: Option<AbsolutePathBuf>,
    pub destination_path: Option<AbsolutePathBuf>,
    pub tool_name: Option<String>,
    pub process_name: Option<String>,
    pub trusted_identity: Option<String>,
    pub recursive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictedEffectKind {
    ProtectedDelete,
    ProtectedMoveOut,
    SensitiveRead,
    SensitiveTransferOut,
    TaintWriteOut,
    ExecExfilTool,
    TrustedIdentityMismatch,
}

/// Guardian's structured effect prediction for one anticipated security impact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredictedEffect {
    pub kind: PredictedEffectKind,
    pub scope: SecurityPermitScope,
    pub confidence: u8,
    pub why: String,
}

/// Scoped permit issued by the Security Host for a single predicted effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityPermit {
    pub id: String,
    pub kind: PredictedEffectKind,
    pub scope: SecurityPermitScope,
    pub issued_at: i64,
    pub expires_at: i64,
    pub issuer: String,
    pub risk_score: u8,
    pub justification: String,
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMismatchClassification {
    TrueRisk,
    Underpredicted,
    PolicyDrift,
}

/// Structured record explaining why runtime-observed behavior diverged from the
/// issued permit or Guardian's prediction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityMismatch {
    pub permit_id: Option<String>,
    pub predicted_effects: Vec<PredictedEffect>,
    pub actual_kind: PredictedEffectKind,
    pub actual_reason_code: String,
    pub actual_scope: SecurityPermitScope,
    pub classification: SecurityMismatchClassification,
    pub process_name: Option<String>,
    pub ancestor_name: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecurityArbitrationDecision {
    AllowWithPermit {
        permits: Vec<SecurityPermit>,
    },
    AllowWithAmendedPermit {
        permits: Vec<SecurityPermit>,
        rationale: String,
    },
    EscalateToHuman {
        risk_score: u8,
        rationale: String,
    },
    Deny {
        risk_score: u8,
        rationale: String,
    },
    DowngradeToDefault {
        rationale: String,
    },
}

#[cfg(test)]
mod tests {
    use super::PredictedEffect;
    use super::PredictedEffectKind;
    use super::SecurityArbitrationDecision;
    use super::SecurityCapabilitySnapshot;
    use super::SecurityMismatch;
    use super::SecurityMismatchClassification;
    use super::SecurityPermit;
    use super::SecurityPermitScope;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn security_types_capability_snapshot_round_trips() {
        let snapshot = SecurityCapabilitySnapshot {
            protected_zones: vec![AbsolutePathBuf::try_from("/Users/demo/Documents").unwrap()],
            sensitive_zones: vec![AbsolutePathBuf::try_from("/Users/demo/.ssh").unwrap()],
            sensitive_export_allow_zones: vec![AbsolutePathBuf::try_from("/tmp").unwrap()],
            exec_exfil_tool_blocklist: vec!["curl".to_string(), "scp".to_string()],
            trusted_tools: vec!["apply_patch".to_string(), "shell".to_string()],
            trusted_tool_identities: vec!["apple.codesign:Terminal".to_string()],
            taint_ttl_seconds: 900,
            read_gate_enabled: true,
            transfer_gate_enabled: true,
            exec_gate_enabled: true,
            allow_vcs_metadata_in_ai_context: true,
            allow_git_merge_pull_in_ai_context: false,
        };

        let json_value = json!({
            "protected_zones": ["/Users/demo/Documents"],
            "sensitive_zones": ["/Users/demo/.ssh"],
            "sensitive_export_allow_zones": ["/tmp"],
            "exec_exfil_tool_blocklist": ["curl", "scp"],
            "trusted_tools": ["apply_patch", "shell"],
            "trusted_tool_identities": ["apple.codesign:Terminal"],
            "taint_ttl_seconds": 900,
            "read_gate_enabled": true,
            "transfer_gate_enabled": true,
            "exec_gate_enabled": true,
            "allow_vcs_metadata_in_ai_context": true,
            "allow_git_merge_pull_in_ai_context": false
        });

        assert_eq!(serde_json::to_value(&snapshot).unwrap(), json_value);
        assert_eq!(
            serde_json::from_value::<SecurityCapabilitySnapshot>(json_value).unwrap(),
            snapshot
        );
    }

    #[test]
    fn security_types_predicted_effect_round_trips() {
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
                trusted_identity: None,
                recursive: false,
            },
            confidence: 92,
            why: "Deletes a protected file under Documents.".to_string(),
        };

        let json_value = json!({
            "kind": "protected_delete",
            "scope": {
                "target_path": "/Users/demo/Documents/report.txt",
                "source_path": null,
                "destination_path": null,
                "tool_name": "shell",
                "process_name": "rm",
                "trusted_identity": null,
                "recursive": false
            },
            "confidence": 92,
            "why": "Deletes a protected file under Documents."
        });

        assert_eq!(serde_json::to_value(&effect).unwrap(), json_value);
        assert_eq!(
            serde_json::from_value::<PredictedEffect>(json_value).unwrap(),
            effect
        );
    }

    #[test]
    fn security_types_permit_round_trips() {
        let permit = SecurityPermit {
            id: "permit-1".to_string(),
            kind: PredictedEffectKind::ProtectedMoveOut,
            scope: SecurityPermitScope {
                target_path: None,
                source_path: Some(
                    AbsolutePathBuf::try_from("/Users/demo/Documents/notes.txt").unwrap(),
                ),
                destination_path: Some(AbsolutePathBuf::try_from("/tmp/notes.txt").unwrap()),
                tool_name: Some("exec_command".to_string()),
                process_name: Some("mv".to_string()),
                trusted_identity: Some("apple.codesign:Terminal".to_string()),
                recursive: false,
            },
            issued_at: 1_710_000_000,
            expires_at: 1_710_000_120,
            issuer: "security-host".to_string(),
            risk_score: 28,
            justification: "Narrow move permit for one protected file.".to_string(),
            thread_id: "thread-123".to_string(),
            turn_id: "turn-456".to_string(),
        };

        let json_value = json!({
            "id": "permit-1",
            "kind": "protected_move_out",
            "scope": {
                "target_path": null,
                "source_path": "/Users/demo/Documents/notes.txt",
                "destination_path": "/tmp/notes.txt",
                "tool_name": "exec_command",
                "process_name": "mv",
                "trusted_identity": "apple.codesign:Terminal",
                "recursive": false
            },
            "issued_at": 1710000000,
            "expires_at": 1710000120,
            "issuer": "security-host",
            "risk_score": 28,
            "justification": "Narrow move permit for one protected file.",
            "thread_id": "thread-123",
            "turn_id": "turn-456"
        });

        assert_eq!(serde_json::to_value(&permit).unwrap(), json_value);
        assert_eq!(
            serde_json::from_value::<SecurityPermit>(json_value).unwrap(),
            permit
        );
    }

    #[test]
    fn security_types_mismatch_round_trips() {
        let mismatch = SecurityMismatch {
            permit_id: Some("permit-1".to_string()),
            predicted_effects: vec![PredictedEffect {
                kind: PredictedEffectKind::SensitiveRead,
                scope: SecurityPermitScope {
                    target_path: Some(
                        AbsolutePathBuf::try_from("/Users/demo/.ssh/id_rsa").unwrap(),
                    ),
                    source_path: None,
                    destination_path: None,
                    tool_name: Some("mcp".to_string()),
                    process_name: None,
                    trusted_identity: None,
                    recursive: false,
                },
                confidence: 67,
                why: "Reads a sensitive credential file.".to_string(),
            }],
            actual_kind: PredictedEffectKind::SensitiveTransferOut,
            actual_reason_code: "es_sensitive_transfer".to_string(),
            actual_scope: SecurityPermitScope {
                target_path: Some(AbsolutePathBuf::try_from("/Users/demo/.ssh/id_rsa").unwrap()),
                source_path: Some(AbsolutePathBuf::try_from("/Users/demo/.ssh/id_rsa").unwrap()),
                destination_path: Some(
                    AbsolutePathBuf::try_from("/Users/demo/Desktop/id_rsa").unwrap(),
                ),
                tool_name: Some("mcp".to_string()),
                process_name: Some("tar".to_string()),
                trusted_identity: Some("unsigned:python".to_string()),
                recursive: false,
            },
            classification: SecurityMismatchClassification::Underpredicted,
            process_name: Some("tar".to_string()),
            ancestor_name: Some("python".to_string()),
            summary: "Guardian predicted a sensitive read, but runtime observed transfer out."
                .to_string(),
        };

        let json_value = json!({
            "permit_id": "permit-1",
            "predicted_effects": [{
                "kind": "sensitive_read",
                "scope": {
                    "target_path": "/Users/demo/.ssh/id_rsa",
                    "source_path": null,
                    "destination_path": null,
                    "tool_name": "mcp",
                    "process_name": null,
                    "trusted_identity": null,
                    "recursive": false
                },
                "confidence": 67,
                "why": "Reads a sensitive credential file."
            }],
            "actual_kind": "sensitive_transfer_out",
            "actual_reason_code": "es_sensitive_transfer",
            "actual_scope": {
                "target_path": "/Users/demo/.ssh/id_rsa",
                "source_path": "/Users/demo/.ssh/id_rsa",
                "destination_path": "/Users/demo/Desktop/id_rsa",
                "tool_name": "mcp",
                "process_name": "tar",
                "trusted_identity": "unsigned:python",
                "recursive": false
            },
            "classification": "underpredicted",
            "process_name": "tar",
            "ancestor_name": "python",
            "summary": "Guardian predicted a sensitive read, but runtime observed transfer out."
        });

        assert_eq!(serde_json::to_value(&mismatch).unwrap(), json_value);
        assert_eq!(
            serde_json::from_value::<SecurityMismatch>(json_value).unwrap(),
            mismatch
        );
        assert_eq!(
            mismatch.classification,
            SecurityMismatchClassification::Underpredicted
        );
    }

    #[test]
    fn security_types_arbitration_decision_round_trips() {
        let decision = SecurityArbitrationDecision::EscalateToHuman {
            risk_score: 91,
            rationale: "Sensitive transfer exceeds automatic permit scope.".to_string(),
        };

        let json_value = json!({
            "kind": "escalate_to_human",
            "risk_score": 91,
            "rationale": "Sensitive transfer exceeds automatic permit scope."
        });

        assert_eq!(serde_json::to_value(&decision).unwrap(), json_value);
        assert_eq!(
            serde_json::from_value::<SecurityArbitrationDecision>(json_value).unwrap(),
            decision
        );
    }
}
