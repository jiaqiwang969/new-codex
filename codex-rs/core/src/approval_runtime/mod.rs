mod hosted;
mod types;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use hosted::HostedApprovalRuntimeClient;
use tokio::sync::Mutex;

pub(crate) use types::PreparedRuntimeAction;
pub(crate) use types::RuntimeChildLeaseRequest;
pub(crate) use types::RuntimeDecision;
pub(crate) use types::RuntimeFinishObservation;
pub(crate) use types::RuntimeFinishRequest;
pub(crate) use types::RuntimeHealth;
pub(crate) use types::RuntimeLease;
pub(crate) use types::RuntimeLeaseKind;
pub(crate) use types::RuntimeLeaseRegistration;
pub(crate) use types::RuntimePreflight;
pub(crate) use types::RuntimePreflightRequest;

pub(crate) type SharedApprovalRuntime = Arc<dyn ApprovalRuntimeClient>;

pub(crate) struct ApprovalRuntime<Client> {
    client: Client,
}

impl<Client> ApprovalRuntime<Client> {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }
}

impl<Client> ApprovalRuntime<Client>
where
    Client: ApprovalRuntimeClient,
{
    pub(crate) async fn prepare(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<PreparedRuntimeAction> {
        let preflight = self.client.preflight(request).await?;
        Ok(PreparedRuntimeAction {
            action_id: preflight.action_id,
            decision: preflight.health.into(),
        })
    }

    pub(crate) async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeDecision> {
        let observation = self.client.finish(request).await?;
        Ok(observation.into())
    }
}

#[derive(Default)]
pub(crate) struct InMemoryApprovalRuntimeClient {
    state: Mutex<InMemoryApprovalRuntimeState>,
}

#[derive(Default)]
struct InMemoryApprovalRuntimeState {
    next_action_id: usize,
    next_lease_id: usize,
    leases: HashMap<String, InMemoryRuntimeLease>,
}

struct InMemoryRuntimeLease {
    children: HashSet<String>,
    usable: bool,
}

impl InMemoryApprovalRuntimeState {
    fn next_action_id(&mut self) -> String {
        self.next_action_id += 1;
        format!("action-{}", self.next_action_id)
    }

    fn next_lease_id(&mut self) -> String {
        self.next_lease_id += 1;
        format!("lease-{}", self.next_lease_id)
    }

    fn revoke_lease_and_descendants(&mut self, lease_id: &str) {
        let children = match self.leases.get_mut(lease_id) {
            Some(entry) => {
                entry.usable = false;
                entry.children.iter().cloned().collect::<Vec<_>>()
            }
            None => return,
        };
        for child_id in children {
            self.revoke_lease_and_descendants(child_id.as_str());
        }
    }
}

pub(crate) fn default_runtime_client(codex_home: &Path) -> SharedApprovalRuntime {
    Arc::new(HostedApprovalRuntimeClient::new(codex_home))
}

#[async_trait]
pub(crate) trait ApprovalRuntimeClient: Send + Sync {
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease>;

    async fn derive_child_lease(
        &self,
        request: RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease>;

    async fn revoke_lease(&self, lease_id: &str) -> anyhow::Result<()>;

    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight>;

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation>;
}

#[async_trait]
impl<Client> ApprovalRuntimeClient for Arc<Client>
where
    Client: ApprovalRuntimeClient + ?Sized,
{
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease> {
        self.as_ref().register_lease(request).await
    }

    async fn derive_child_lease(
        &self,
        request: RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease> {
        self.as_ref().derive_child_lease(request).await
    }

    async fn revoke_lease(&self, lease_id: &str) -> anyhow::Result<()> {
        self.as_ref().revoke_lease(lease_id).await
    }

    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight> {
        self.as_ref().preflight(request).await
    }

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation> {
        self.as_ref().finish(request).await
    }
}

#[async_trait]
impl ApprovalRuntimeClient for InMemoryApprovalRuntimeClient {
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease> {
        let mut state = self.state.lock().await;
        let lease = RuntimeLease {
            id: state.next_lease_id(),
            kind: RuntimeLeaseKind::Session,
            owner_id: request.owner_id,
            thread_id: request.thread_id,
            parent_lease_id: None,
        };
        state.leases.insert(
            lease.id.clone(),
            InMemoryRuntimeLease {
                children: HashSet::new(),
                usable: true,
            },
        );
        Ok(lease)
    }

    async fn derive_child_lease(
        &self,
        request: RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease> {
        let mut state = self.state.lock().await;
        let Some(parent) = state.leases.get(&request.parent_lease_id) else {
            anyhow::bail!("runtime parent lease {} not found", request.parent_lease_id);
        };
        if !parent.usable {
            anyhow::bail!(
                "runtime parent lease {} is no longer usable",
                request.parent_lease_id
            );
        }
        let lease_id = state.next_lease_id();
        let lease = RuntimeLease {
            id: lease_id,
            kind: RuntimeLeaseKind::ChildAgent,
            owner_id: request.child_owner_id,
            thread_id: request.thread_id,
            parent_lease_id: Some(request.parent_lease_id.clone()),
        };
        if let Some(parent) = state.leases.get_mut(&request.parent_lease_id) {
            parent.children.insert(lease.id.clone());
        }
        state.leases.insert(
            lease.id.clone(),
            InMemoryRuntimeLease {
                children: HashSet::new(),
                usable: true,
            },
        );
        Ok(lease)
    }

    async fn revoke_lease(&self, lease_id: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        state.revoke_lease_and_descendants(lease_id);
        Ok(())
    }

    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight> {
        let mut state = self.state.lock().await;
        let health = match state.leases.get(&request.lease_id) {
            Some(entry) if entry.usable => RuntimeHealth::Healthy,
            Some(_) | None => RuntimeHealth::FallbackToHuman {
                summary: format!("runtime lease {} is no longer usable", request.lease_id),
            },
        };
        let action_id = matches!(health, RuntimeHealth::Healthy).then(|| state.next_action_id());
        Ok(RuntimePreflight { health, action_id })
    }

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation> {
        let state = self.state.lock().await;
        Ok(match state.leases.get(&request.lease_id) {
            Some(entry) if entry.usable => RuntimeFinishObservation::Clean,
            Some(_) | None => RuntimeFinishObservation::FallbackToHuman {
                summary: format!("runtime lease {} is no longer usable", request.lease_id),
            },
        })
    }
}

#[cfg(test)]
mod tests;
