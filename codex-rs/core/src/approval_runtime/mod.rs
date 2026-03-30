mod types;

use async_trait::async_trait;

pub(crate) use types::PreparedRuntimeAction;
pub(crate) use types::RuntimeDecision;
pub(crate) use types::RuntimeFinishObservation;
pub(crate) use types::RuntimeFinishRequest;
pub(crate) use types::RuntimeHealth;
pub(crate) use types::RuntimePreflight;
pub(crate) use types::RuntimePreflightRequest;

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

#[async_trait]
pub(crate) trait ApprovalRuntimeClient: Send + Sync {
    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight>;

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation>;
}

#[cfg(test)]
mod tests;
