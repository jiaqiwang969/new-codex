#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeLeaseKind {
    Session,
    ChildAgent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeLeaseRegistration {
    pub(crate) owner_id: String,
    pub(crate) thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeLease {
    pub(crate) id: String,
    pub(crate) kind: RuntimeLeaseKind,
    pub(crate) owner_id: String,
    pub(crate) thread_id: String,
    pub(crate) parent_lease_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeChildLeaseRequest {
    pub(crate) parent_lease_id: String,
    pub(crate) child_owner_id: String,
    pub(crate) thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeHealth {
    Healthy,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimePreflight {
    pub(crate) health: RuntimeHealth,
    pub(crate) action_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimePreflightRequest {
    pub(crate) lease_id: String,
    pub(crate) destructive: bool,
    pub(crate) permit_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeFinishRequest {
    pub(crate) lease_id: String,
    pub(crate) action_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeFinishObservation {
    Clean,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
    Mismatch { summary: String },
    PolicyDrift { summary: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeDecision {
    Ok,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
    Mismatch { summary: String },
    PolicyDrift { summary: String },
}

impl RuntimeDecision {
    pub(crate) fn blocks_automatic_approval(&self) -> bool {
        matches!(
            self,
            Self::FallbackToHuman { .. } | Self::Mismatch { .. } | Self::PolicyDrift { .. }
        )
    }
}

impl From<RuntimeHealth> for RuntimeDecision {
    fn from(value: RuntimeHealth) -> Self {
        match value {
            RuntimeHealth::Healthy => Self::Ok,
            RuntimeHealth::Recovery { summary } => Self::Recovery { summary },
            RuntimeHealth::FallbackToHuman { summary } => Self::FallbackToHuman { summary },
        }
    }
}

impl From<RuntimeFinishObservation> for RuntimeDecision {
    fn from(value: RuntimeFinishObservation) -> Self {
        match value {
            RuntimeFinishObservation::Clean => Self::Ok,
            RuntimeFinishObservation::Recovery { summary } => Self::Recovery { summary },
            RuntimeFinishObservation::FallbackToHuman { summary } => {
                Self::FallbackToHuman { summary }
            }
            RuntimeFinishObservation::Mismatch { summary } => Self::Mismatch { summary },
            RuntimeFinishObservation::PolicyDrift { summary } => Self::PolicyDrift { summary },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedRuntimeAction {
    pub(crate) action_id: Option<String>,
    pub(crate) decision: RuntimeDecision,
}
