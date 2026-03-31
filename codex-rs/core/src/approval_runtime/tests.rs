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
use std::fs::File;
use std::fs::FileTimes;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

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

fn hosted_client(codex_home: &Path) -> super::hosted::HostedApprovalRuntimeClient {
    super::hosted::HostedApprovalRuntimeClient::new(codex_home)
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

#[tokio::test]
async fn hosted_runtime_persists_leases_across_client_instances() {
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let registered = hosted_client(codex_home.path())
        .register_lease(RuntimeLeaseRegistration {
            owner_id: "thread-1".to_string(),
            thread_id: "thread-1".to_string(),
        })
        .await
        .expect("register lease");

    let preflight = hosted_client(codex_home.path())
        .preflight(&RuntimePreflightRequest {
            lease_id: registered.id.clone(),
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("preflight lease");

    assert_eq!(
        preflight,
        RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-1".to_string()),
        }
    );
}

#[tokio::test]
async fn default_runtime_client_uses_hosted_backend_for_same_codex_home() {
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let registered = super::default_runtime_client(codex_home.path())
        .register_lease(RuntimeLeaseRegistration {
            owner_id: "thread-1".to_string(),
            thread_id: "thread-1".to_string(),
        })
        .await
        .expect("register lease");

    let preflight = super::default_runtime_client(codex_home.path())
        .preflight(&RuntimePreflightRequest {
            lease_id: registered.id.clone(),
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("preflight lease");

    assert_eq!(
        preflight,
        RuntimePreflight {
            health: RuntimeHealth::Healthy,
            action_id: Some("action-1".to_string()),
        }
    );
}

#[tokio::test]
async fn hosted_runtime_persists_child_parent_linkage_across_client_instances() {
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let parent = hosted_client(codex_home.path())
        .register_lease(RuntimeLeaseRegistration {
            owner_id: "thread-parent".to_string(),
            thread_id: "thread-parent".to_string(),
        })
        .await
        .expect("register parent lease");

    let child = hosted_client(codex_home.path())
        .derive_child_lease(RuntimeChildLeaseRequest {
            parent_lease_id: parent.id.clone(),
            child_owner_id: "thread-child".to_string(),
            thread_id: "thread-child".to_string(),
        })
        .await
        .expect("derive child lease");

    assert_eq!(
        child,
        RuntimeLease {
            id: "lease-2".to_string(),
            kind: RuntimeLeaseKind::ChildAgent,
            owner_id: "thread-child".to_string(),
            thread_id: "thread-child".to_string(),
            parent_lease_id: Some(parent.id),
        }
    );
}

#[tokio::test]
async fn hosted_runtime_revoking_parent_invalidates_child_preflight_and_finish() {
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let parent = hosted_client(codex_home.path())
        .register_lease(RuntimeLeaseRegistration {
            owner_id: "thread-parent".to_string(),
            thread_id: "thread-parent".to_string(),
        })
        .await
        .expect("register parent lease");
    let child = hosted_client(codex_home.path())
        .derive_child_lease(RuntimeChildLeaseRequest {
            parent_lease_id: parent.id.clone(),
            child_owner_id: "thread-child".to_string(),
            thread_id: "thread-child".to_string(),
        })
        .await
        .expect("derive child lease");

    hosted_client(codex_home.path())
        .revoke_lease(&parent.id)
        .await
        .expect("revoke parent lease");

    let preflight = hosted_client(codex_home.path())
        .preflight(&RuntimePreflightRequest {
            lease_id: child.id.clone(),
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("preflight child lease");
    let finish = hosted_client(codex_home.path())
        .finish(&RuntimeFinishRequest {
            lease_id: child.id.clone(),
            action_id: Some("action-1".to_string()),
        })
        .await
        .expect("finish child lease");

    assert_eq!(
        preflight,
        RuntimePreflight {
            health: RuntimeHealth::FallbackToHuman {
                summary: format!("runtime lease {} is no longer usable", child.id),
            },
            action_id: None,
        }
    );
    assert_eq!(
        finish,
        RuntimeFinishObservation::FallbackToHuman {
            summary: format!("runtime lease {} is no longer usable", child.id),
        }
    );
}

#[tokio::test]
async fn hosted_runtime_stale_lock_recovery_surfaces_on_next_preflight() {
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let lease = hosted_client(codex_home.path())
        .register_lease(RuntimeLeaseRegistration {
            owner_id: "thread-1".to_string(),
            thread_id: "thread-1".to_string(),
        })
        .await
        .expect("register lease");

    let lock_path = super::hosted::hosted_approval_runtime_lock_path(codex_home.path());
    std::fs::create_dir_all(
        lock_path
            .parent()
            .expect("hosted runtime lock always has a parent directory"),
    )
    .expect("create hosted runtime root");
    File::create(&lock_path).expect("create stale lock");
    File::options()
        .write(true)
        .open(&lock_path)
        .expect("open stale lock")
        .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(60)))
        .expect("age lock file");

    let preflight = hosted_client(codex_home.path())
        .preflight(&RuntimePreflightRequest {
            lease_id: lease.id,
            destructive: true,
            permit_summary: Some("protected_delete:/tmp/demo".to_string()),
        })
        .await
        .expect("preflight recovered lease");

    assert_eq!(
        preflight,
        RuntimePreflight {
            health: RuntimeHealth::Recovery {
                summary: "recovered stale hosted approval runtime lock using age fallback"
                    .to_string(),
            },
            action_id: Some("action-1".to_string()),
        }
    );
}
