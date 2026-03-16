use crate::security_types::PredictedEffect;
use crate::security_types::PredictedEffectKind;
use crate::security_types::SecurityArbitrationDecision;
use crate::security_types::SecurityCapabilitySnapshot;
use crate::security_types::SecurityMismatch;
use crate::security_types::SecurityMismatchClassification;
use crate::security_types::SecurityPermit;
use crate::security_types::SecurityPermitScope;

const DEFAULT_AUTO_PERMIT_TTL_SECONDS: i64 = 120;
const SECURITY_HOST_ISSUER: &str = "security-host";
const LOW_RISK_PERMIT_JUSTIFICATION: &str = "Low-risk narrow smart-access permit.";
const AMENDED_PERMIT_JUSTIFICATION: &str =
    "Security Host narrowed a broad scope into a precise permit.";
const AMENDED_PERMIT_RATIONALE: &str =
    "Security Host narrowed a recursive request before approval.";
const SENSITIVE_TRANSFER_ESCALATION_RATIONALE: &str =
    "Sensitive transfers require explicit human approval.";
const SENSITIVE_EFFECT_ESCALATION_RATIONALE: &str =
    "Sensitive effects require explicit human approval.";
const TRUST_DOWNGRADE_RATIONALE: &str =
    "Smart Access cannot trust the current runtime state for this effect.";
const TRUST_BOUNDARY_DENY_RATIONALE: &str =
    "Predicted effects cross a Smart Access trust boundary.";
const MISSING_EFFECTS_DENY_RATIONALE: &str =
    "Smart Access requires explicit predicted effects before issuing permits.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityArbitrationContext {
    pub thread_id: String,
    pub turn_id: String,
    pub risk_score: u8,
    pub rationale: String,
    pub issued_at: i64,
}

#[derive(Debug, Clone)]
pub struct SecurityHost {
    capability_snapshot: SecurityCapabilitySnapshot,
    permit_ttl_seconds: i64,
}

impl SecurityHost {
    pub fn new(capability_snapshot: SecurityCapabilitySnapshot) -> Self {
        Self {
            capability_snapshot,
            permit_ttl_seconds: DEFAULT_AUTO_PERMIT_TTL_SECONDS,
        }
    }

    pub fn arbitrate(
        &self,
        context: SecurityArbitrationContext,
        predicted_effects: Vec<PredictedEffect>,
    ) -> SecurityArbitrationDecision {
        if predicted_effects.is_empty() {
            return SecurityArbitrationDecision::Deny {
                risk_score: context.risk_score,
                rationale: MISSING_EFFECTS_DENY_RATIONALE.to_string(),
            };
        }

        if self.requires_trust_downgrade(predicted_effects.as_slice()) {
            return SecurityArbitrationDecision::DowngradeToDefault {
                rationale: TRUST_DOWNGRADE_RATIONALE.to_string(),
            };
        }

        if predicted_effects
            .iter()
            .any(|effect| effect.kind == PredictedEffectKind::SensitiveTransferOut)
        {
            return SecurityArbitrationDecision::EscalateToHuman {
                risk_score: context.risk_score,
                rationale: SENSITIVE_TRANSFER_ESCALATION_RATIONALE.to_string(),
            };
        }

        if predicted_effects.iter().any(|effect| {
            matches!(
                effect.kind,
                PredictedEffectKind::SensitiveRead | PredictedEffectKind::TaintWriteOut
            )
        }) {
            return SecurityArbitrationDecision::EscalateToHuman {
                risk_score: context.risk_score,
                rationale: SENSITIVE_EFFECT_ESCALATION_RATIONALE.to_string(),
            };
        }

        if predicted_effects.iter().any(|effect| {
            matches!(
                effect.kind,
                PredictedEffectKind::ExecExfilTool | PredictedEffectKind::TrustedIdentityMismatch
            )
        }) {
            return SecurityArbitrationDecision::Deny {
                risk_score: context.risk_score,
                rationale: TRUST_BOUNDARY_DENY_RATIONALE.to_string(),
            };
        }

        let mut permits = Vec::with_capacity(predicted_effects.len());
        let mut amended_scope = false;

        for (index, effect) in predicted_effects.iter().enumerate() {
            let scope = match effect.kind {
                PredictedEffectKind::ProtectedDelete => {
                    match self.validate_delete_scope(effect.scope.clone()) {
                        Some(scope) => scope,
                        None => {
                            return SecurityArbitrationDecision::Deny {
                                risk_score: context.risk_score,
                                rationale: TRUST_BOUNDARY_DENY_RATIONALE.to_string(),
                            };
                        }
                    }
                }
                PredictedEffectKind::ProtectedMoveOut => {
                    match self.validate_move_scope(effect.scope.clone()) {
                        Some(scope) => scope,
                        None => {
                            return SecurityArbitrationDecision::Deny {
                                risk_score: context.risk_score,
                                rationale: TRUST_BOUNDARY_DENY_RATIONALE.to_string(),
                            };
                        }
                    }
                }
                PredictedEffectKind::SensitiveRead
                | PredictedEffectKind::SensitiveTransferOut
                | PredictedEffectKind::TaintWriteOut
                | PredictedEffectKind::ExecExfilTool
                | PredictedEffectKind::TrustedIdentityMismatch => {
                    return SecurityArbitrationDecision::Deny {
                        risk_score: context.risk_score,
                        rationale: TRUST_BOUNDARY_DENY_RATIONALE.to_string(),
                    };
                }
            };

            let scope_was_amended = scope.recursive != effect.scope.recursive;
            if scope_was_amended {
                amended_scope = true;
            }

            let justification = if scope_was_amended {
                AMENDED_PERMIT_JUSTIFICATION
            } else {
                LOW_RISK_PERMIT_JUSTIFICATION
            };

            permits.push(SecurityPermit {
                id: format!("{}:{}:{index}", context.thread_id, context.turn_id),
                kind: effect.kind,
                scope,
                issued_at: context.issued_at,
                expires_at: context.issued_at + self.permit_ttl_seconds,
                issuer: SECURITY_HOST_ISSUER.to_string(),
                risk_score: context.risk_score,
                justification: justification.to_string(),
                thread_id: context.thread_id.clone(),
                turn_id: context.turn_id.clone(),
            });
        }

        if amended_scope {
            return SecurityArbitrationDecision::AllowWithAmendedPermit {
                permits,
                rationale: AMENDED_PERMIT_RATIONALE.to_string(),
            };
        }

        SecurityArbitrationDecision::AllowWithPermit { permits }
    }

    pub fn classify_mismatch(&self, mismatch: &SecurityMismatch) -> SecurityMismatchClassification {
        if mismatch.actual_reason_code.contains("policy_drift")
            || self.effect_requires_disabled_gate(mismatch.actual_kind)
        {
            return SecurityMismatchClassification::PolicyDrift;
        }

        if mismatch.predicted_effects.is_empty() {
            return SecurityMismatchClassification::TrueRisk;
        }

        if mismatch
            .predicted_effects
            .iter()
            .any(|effect| effect.kind == mismatch.actual_kind)
        {
            return SecurityMismatchClassification::Underpredicted;
        }

        match mismatch.actual_kind {
            PredictedEffectKind::ProtectedDelete
            | PredictedEffectKind::ProtectedMoveOut
            | PredictedEffectKind::SensitiveTransferOut
            | PredictedEffectKind::ExecExfilTool
            | PredictedEffectKind::TrustedIdentityMismatch => {
                SecurityMismatchClassification::TrueRisk
            }
            PredictedEffectKind::SensitiveRead | PredictedEffectKind::TaintWriteOut => {
                SecurityMismatchClassification::Underpredicted
            }
        }
    }

    fn requires_trust_downgrade(&self, predicted_effects: &[PredictedEffect]) -> bool {
        predicted_effects
            .iter()
            .any(|effect| self.effect_requires_disabled_gate(effect.kind))
            || (self.capability_snapshot.trusted_tool_identities.is_empty()
                && predicted_effects.iter().any(|effect| {
                    matches!(
                        effect.kind,
                        PredictedEffectKind::ExecExfilTool
                            | PredictedEffectKind::TrustedIdentityMismatch
                    )
                }))
    }

    fn effect_requires_disabled_gate(&self, kind: PredictedEffectKind) -> bool {
        match kind {
            PredictedEffectKind::ProtectedDelete => false,
            PredictedEffectKind::ProtectedMoveOut => {
                !self.capability_snapshot.transfer_gate_enabled
            }
            PredictedEffectKind::SensitiveRead => !self.capability_snapshot.read_gate_enabled,
            PredictedEffectKind::SensitiveTransferOut | PredictedEffectKind::TaintWriteOut => {
                !self.capability_snapshot.transfer_gate_enabled
            }
            PredictedEffectKind::ExecExfilTool | PredictedEffectKind::TrustedIdentityMismatch => {
                !self.capability_snapshot.exec_gate_enabled
            }
        }
    }

    fn validate_delete_scope(&self, mut scope: SecurityPermitScope) -> Option<SecurityPermitScope> {
        scope.target_path.as_ref()?;
        if scope.recursive {
            scope.recursive = false;
        }
        Some(scope)
    }

    fn validate_move_scope(&self, mut scope: SecurityPermitScope) -> Option<SecurityPermitScope> {
        if scope.source_path.is_none() || scope.destination_path.is_none() {
            return None;
        }
        if scope.recursive {
            scope.recursive = false;
        }
        Some(scope)
    }
}

#[cfg(test)]
mod tests;
