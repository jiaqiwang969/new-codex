use crate::codex::TurnContext;
use crate::error::Result as CodexResult;
use async_trait::async_trait;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;

#[async_trait]
pub trait HarnessMiddleware: Send + Sync {
    async fn before_turn(
        &self,
        _ctx: &Arc<TurnContext>,
        input: Vec<UserInput>,
    ) -> CodexResult<Vec<UserInput>> {
        Ok(input)
    }

    async fn after_turn(
        &self,
        _ctx: &Arc<TurnContext>,
        last_agent_message: Option<String>,
    ) -> CodexResult<Option<String>> {
        Ok(last_agent_message)
    }
}
