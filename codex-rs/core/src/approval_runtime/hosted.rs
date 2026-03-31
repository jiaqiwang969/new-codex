use super::ApprovalRuntimeClient;
use super::RuntimeChildLeaseRequest;
use super::RuntimeFinishObservation;
use super::RuntimeFinishRequest;
use super::RuntimeHealth;
use super::RuntimeLease;
use super::RuntimeLeaseKind;
use super::RuntimeLeaseRegistration;
use super::RuntimePreflight;
use super::RuntimePreflightRequest;
use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

const HOSTED_APPROVAL_RUNTIME_SUBDIR: &str = "approval-runtime";
const HOSTED_APPROVAL_RUNTIME_STATE_FILE: &str = "runtime-state.json";
const HOSTED_APPROVAL_RUNTIME_LOCK_FILE: &str = "runtime-state.lock";
const HOSTED_APPROVAL_RUNTIME_LOCK_RETRIES: usize = 200;
const HOSTED_APPROVAL_RUNTIME_LOCK_RETRY_SLEEP: Duration = Duration::from_millis(10);
const HOSTED_APPROVAL_RUNTIME_STALE_LOCK_MAX_AGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HostedApprovalRuntimeState {
    next_action_id: u64,
    next_lease_id: u64,
    leases: BTreeMap<String, RuntimeLease>,
    #[serde(default)]
    pending_recoveries: Vec<String>,
}

#[derive(Debug)]
struct HostedApprovalRuntimeFileLock {
    lock_path: PathBuf,
    recoveries: Vec<String>,
}

impl HostedApprovalRuntimeFileLock {
    fn acquire(lock_path: &Path) -> anyhow::Result<Self> {
        let mut recoveries = Vec::new();
        for _ in 0..HOSTED_APPROVAL_RUNTIME_LOCK_RETRIES {
            match OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(lock_path)
            {
                Ok(file) => {
                    file.sync_all().with_context(|| {
                        format!(
                            "failed to sync hosted approval runtime lock {}",
                            lock_path.display()
                        )
                    })?;
                    return Ok(Self {
                        lock_path: lock_path.to_path_buf(),
                        recoveries,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if hosted_approval_runtime_lock_exceeds_age_limit(lock_path) {
                        match std::fs::remove_file(lock_path) {
                            Ok(()) => {
                                recoveries.push(
                                    "recovered stale hosted approval runtime lock using age fallback"
                                        .to_string(),
                                );
                                continue;
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                recoveries.push(
                                    "recovered stale hosted approval runtime lock using age fallback"
                                        .to_string(),
                                );
                                continue;
                            }
                            Err(err) => {
                                anyhow::bail!(
                                    "failed to clear stale hosted approval runtime lock {}: {err}",
                                    lock_path.display()
                                );
                            }
                        }
                    }
                    sleep(HOSTED_APPROVAL_RUNTIME_LOCK_RETRY_SLEEP);
                }
                Err(err) => {
                    anyhow::bail!(
                        "failed to lock hosted approval runtime state {}: {err}",
                        lock_path.display()
                    );
                }
            }
        }

        anyhow::bail!(
            "timed out acquiring hosted approval runtime lock {}",
            lock_path.display()
        );
    }
}

impl Drop for HostedApprovalRuntimeFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

#[derive(Debug)]
pub(super) struct HostedApprovalRuntimeClient {
    codex_home: PathBuf,
    process_lock: Mutex<()>,
}

impl HostedApprovalRuntimeClient {
    pub(super) fn new(codex_home: &Path) -> Self {
        Self {
            codex_home: codex_home.to_path_buf(),
            process_lock: Mutex::new(()),
        }
    }

    fn state_root(&self) -> PathBuf {
        self.codex_home.join(HOSTED_APPROVAL_RUNTIME_SUBDIR)
    }

    fn state_path(&self) -> PathBuf {
        hosted_approval_runtime_state_path(self.codex_home.as_path())
    }

    fn lock_path(&self) -> PathBuf {
        hosted_approval_runtime_lock_path(self.codex_home.as_path())
    }

    fn with_state_mut<R>(
        &self,
        f: impl FnOnce(&mut HostedApprovalRuntimeState) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        let _process_guard = self.process_lock.lock().map_err(|err| {
            anyhow::anyhow!("hosted approval runtime process lock poisoned: {err}")
        })?;
        self.ensure_state_root()?;
        let state_lock = HostedApprovalRuntimeFileLock::acquire(self.lock_path().as_path())?;
        let mut state = self.load_state()?;
        state
            .pending_recoveries
            .extend(state_lock.recoveries.iter().cloned());
        let result = f(&mut state)?;
        self.store_state(&state)?;
        Ok(result)
    }

    fn ensure_state_root(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(self.state_root()).with_context(|| {
            format!(
                "failed to create hosted approval runtime directory {}",
                self.state_root().display()
            )
        })
    }

    fn load_state(&self) -> anyhow::Result<HostedApprovalRuntimeState> {
        let state_path = self.state_path();
        if !state_path.exists() {
            return Ok(HostedApprovalRuntimeState::default());
        }

        let state_bytes = std::fs::read(&state_path).with_context(|| {
            format!(
                "failed to read hosted approval runtime state {}",
                state_path.display()
            )
        })?;
        serde_json::from_slice::<HostedApprovalRuntimeState>(&state_bytes).with_context(|| {
            format!(
                "failed to parse hosted approval runtime state {}",
                state_path.display()
            )
        })
    }

    fn store_state(&self, state: &HostedApprovalRuntimeState) -> anyhow::Result<()> {
        let state_path = self.state_path();
        let mut temp_file =
            tempfile::NamedTempFile::new_in(self.state_root()).with_context(|| {
                format!(
                    "failed to create hosted approval runtime temp file in {}",
                    self.state_root().display()
                )
            })?;
        serde_json::to_writer_pretty(temp_file.as_file_mut(), state).with_context(|| {
            format!(
                "failed to serialize hosted approval runtime state {}",
                state_path.display()
            )
        })?;
        temp_file.as_file_mut().sync_all().with_context(|| {
            format!(
                "failed to sync hosted approval runtime temp file for {}",
                state_path.display()
            )
        })?;
        if state_path.exists() {
            std::fs::remove_file(&state_path).with_context(|| {
                format!(
                    "failed to replace hosted approval runtime state {}",
                    state_path.display()
                )
            })?;
        }
        temp_file.persist(&state_path).with_context(|| {
            format!(
                "failed to persist hosted approval runtime state {}",
                state_path.display()
            )
        })?;
        Ok(())
    }
}

pub(super) fn hosted_approval_runtime_state_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join(HOSTED_APPROVAL_RUNTIME_SUBDIR)
        .join(HOSTED_APPROVAL_RUNTIME_STATE_FILE)
}

pub(super) fn hosted_approval_runtime_lock_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join(HOSTED_APPROVAL_RUNTIME_SUBDIR)
        .join(HOSTED_APPROVAL_RUNTIME_LOCK_FILE)
}

fn hosted_approval_runtime_lock_exceeds_age_limit(lock_path: &Path) -> bool {
    std::fs::metadata(lock_path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified_at| modified_at.elapsed().ok())
        .is_some_and(|elapsed| elapsed > HOSTED_APPROVAL_RUNTIME_STALE_LOCK_MAX_AGE)
}

fn revoke_lease_and_descendants(state: &mut HostedApprovalRuntimeState, lease_id: &str) {
    let mut revoked_lease_ids = Vec::new();
    let mut discovered = BTreeSet::new();
    let mut pending_lease_ids = vec![lease_id.to_string()];
    let mut index = 0;
    while index < pending_lease_ids.len() {
        let current_lease_id = pending_lease_ids[index].clone();
        index += 1;
        if !discovered.insert(current_lease_id.clone()) {
            continue;
        }
        if !state.leases.contains_key(current_lease_id.as_str()) {
            continue;
        }
        revoked_lease_ids.push(current_lease_id.clone());
        let child_lease_ids = state
            .leases
            .values()
            .filter(|lease| lease.parent_lease_id.as_deref() == Some(current_lease_id.as_str()))
            .map(|lease| lease.id.clone())
            .collect::<Vec<_>>();
        pending_lease_ids.extend(child_lease_ids);
    }

    for revoked_lease_id in revoked_lease_ids {
        state.leases.remove(revoked_lease_id.as_str());
    }
}

#[async_trait]
impl ApprovalRuntimeClient for HostedApprovalRuntimeClient {
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease> {
        self.with_state_mut(|state| {
            state.next_lease_id += 1;
            let lease = RuntimeLease {
                id: format!("lease-{}", state.next_lease_id),
                kind: RuntimeLeaseKind::Session,
                owner_id: request.owner_id,
                thread_id: request.thread_id,
                parent_lease_id: None,
            };
            state.leases.insert(lease.id.clone(), lease.clone());
            Ok(lease)
        })
    }

    async fn derive_child_lease(
        &self,
        request: RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease> {
        self.with_state_mut(|state| {
            if !state.leases.contains_key(request.parent_lease_id.as_str()) {
                anyhow::bail!("runtime parent lease {} not found", request.parent_lease_id);
            }

            state.next_lease_id += 1;
            let lease = RuntimeLease {
                id: format!("lease-{}", state.next_lease_id),
                kind: RuntimeLeaseKind::ChildAgent,
                owner_id: request.child_owner_id,
                thread_id: request.thread_id,
                parent_lease_id: Some(request.parent_lease_id),
            };
            state.leases.insert(lease.id.clone(), lease.clone());
            Ok(lease)
        })
    }

    async fn revoke_lease(&self, lease_id: &str) -> anyhow::Result<()> {
        self.with_state_mut(|state| {
            revoke_lease_and_descendants(state, lease_id);
            Ok(())
        })
    }

    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight> {
        self.with_state_mut(|state| {
            if !state.leases.contains_key(&request.lease_id) {
                return Ok(RuntimePreflight {
                    health: RuntimeHealth::FallbackToHuman {
                        summary: format!("runtime lease {} is no longer usable", request.lease_id),
                    },
                    action_id: None,
                });
            }

            state.next_action_id += 1;
            let health = if state.pending_recoveries.is_empty() {
                RuntimeHealth::Healthy
            } else {
                let summary = state.pending_recoveries.join("; ");
                state.pending_recoveries.clear();
                RuntimeHealth::Recovery { summary }
            };
            Ok(RuntimePreflight {
                health,
                action_id: Some(format!("action-{}", state.next_action_id)),
            })
        })
    }

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation> {
        self.with_state_mut(|state| {
            if state.leases.contains_key(&request.lease_id) {
                return Ok(RuntimeFinishObservation::Clean);
            }

            Ok(RuntimeFinishObservation::FallbackToHuman {
                summary: format!("runtime lease {} is no longer usable", request.lease_id),
            })
        })
    }
}
