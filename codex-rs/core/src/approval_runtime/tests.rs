use super::ApprovalRuntime;
use super::ApprovalRuntimeClient;
use super::RuntimeChildLeaseRequest;
use super::RuntimeDecision;
use super::RuntimeFinishObservation;
use super::RuntimeFinishRequest;
use super::RuntimeHealth;
use super::RuntimeLease;
use super::RuntimeLeaseKind;
use super::RuntimeLeaseRegistration;
use super::RuntimePreflight;
use super::RuntimePreflightRequest;
use async_trait::async_trait;
use pretty_assertions::assert_eq;

struct FakeApprovalRuntimeClient {
    preflight: RuntimePreflight,
    finish: RuntimeFinishObservation,
}

#[async_trait]
impl ApprovalRuntimeClient for FakeApprovalRuntimeClient {
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease> {
        Ok(RuntimeLease {
            id: "lease-1".to_string(),
            kind: RuntimeLeaseKind::Session,
            owner_id: request.owner_id,
            thread_id: request.thread_id,
            parent_lease_id: None,
        })
    }

    async fn derive_child_lease(
        &self,
        request: RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease> {
        Ok(RuntimeLease {
            id: "lease-child-1".to_string(),
            kind: RuntimeLeaseKind::ChildAgent,
            owner_id: request.child_owner_id,
            thread_id: request.thread_id,
            parent_lease_id: Some(request.parent_lease_id),
        })
    }

    async fn revoke_lease(&self, _lease_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn preflight(
        &self,
        _request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight> {
        Ok(self.preflight.clone())
    }

    async fn finish(
        &self,
        _request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation> {
        Ok(self.finish.clone())
    }
}

#[tokio::test]
async fn approval_runtime_prepare_maps_healthy_preflight_to_ok() {
    let runtime = ApprovalRuntime::new(FakeApprovalRuntimeClient {
        preflight: RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-1".to_string()),
        },
        finish: RuntimeFinishObservation::Clean,
    });

    let prepared = runtime
        .prepare(&RuntimePreflightRequest {
            lease_id: "lease-1".to_string(),
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("prepare succeeds");

    assert_eq!(prepared.decision, RuntimeDecision::Ok);
}

#[tokio::test]
async fn approval_runtime_prepare_maps_recovering_preflight_to_recovery() {
    let runtime = ApprovalRuntime::new(FakeApprovalRuntimeClient {
        preflight: RuntimePreflight {
            health: RuntimeHealth::Recovery {
                summary: "stale lock recovered".to_string(),
            },
            action_id: Some("action-2".to_string()),
        },
        finish: RuntimeFinishObservation::Clean,
    });

    let prepared = runtime
        .prepare(&RuntimePreflightRequest {
            lease_id: "lease-1".to_string(),
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("prepare succeeds");

    assert_eq!(
        prepared.decision,
        RuntimeDecision::Recovery {
            summary: "stale lock recovered".to_string(),
        }
    );
}

#[tokio::test]
async fn approval_runtime_finish_maps_runtime_fallback_to_human() {
    let runtime = ApprovalRuntime::new(FakeApprovalRuntimeClient {
        preflight: RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-3".to_string()),
        },
        finish: RuntimeFinishObservation::FallbackToHuman {
            summary: "runtime unavailable".to_string(),
        },
    });

    let decision = runtime
        .finish(&RuntimeFinishRequest {
            lease_id: "lease-1".to_string(),
            action_id: Some("action-3".to_string()),
        })
        .await
        .expect("finish succeeds");

    assert_eq!(
        decision,
        RuntimeDecision::FallbackToHuman {
            summary: "runtime unavailable".to_string(),
        }
    );
}

#[tokio::test]
async fn approval_runtime_finish_maps_policy_drift() {
    let runtime = ApprovalRuntime::new(FakeApprovalRuntimeClient {
        preflight: RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-4".to_string()),
        },
        finish: RuntimeFinishObservation::PolicyDrift {
            summary: "policy epoch changed".to_string(),
        },
    });

    let decision = runtime
        .finish(&RuntimeFinishRequest {
            lease_id: "lease-1".to_string(),
            action_id: Some("action-4".to_string()),
        })
        .await
        .expect("finish succeeds");

    assert_eq!(
        decision,
        RuntimeDecision::PolicyDrift {
            summary: "policy epoch changed".to_string(),
        }
    );
}

#[tokio::test]
async fn approval_runtime_finish_maps_runtime_mismatch() {
    let runtime = ApprovalRuntime::new(FakeApprovalRuntimeClient {
        preflight: RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-5".to_string()),
        },
        finish: RuntimeFinishObservation::Mismatch {
            summary: "permit miss".to_string(),
        },
    });

    let decision = runtime
        .finish(&RuntimeFinishRequest {
            lease_id: "lease-1".to_string(),
            action_id: Some("action-5".to_string()),
        })
        .await
        .expect("finish succeeds");

    assert_eq!(
        decision,
        RuntimeDecision::Mismatch {
            summary: "permit miss".to_string(),
        }
    );
}
