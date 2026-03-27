use super::*;
use crate::provider_pool_failover::ProviderPoolFailoverAction;
use crate::provider_pool_failover::decide_provider_pool_failover;
use crate::provider_pool_runtime::next_account_from_pool;

const PROVIDER_POOL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Wrapper around `maybe_switch_provider_account` that supports cycling through
/// the account pool multiple rounds. When all accounts in the pool have been
/// attempted in the current round, the `attempted_accounts` set is reset and a
/// new round begins — up to `max_rounds` total rounds.
pub(super) async fn try_switch_pool_account(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
    pool_switch_count: &mut usize,
    pool_size: usize,
    max_rounds: usize,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
) -> Option<Arc<TurnContext>> {
    if pool_size <= 1 {
        return None;
    }

    if matches!(
        decide_provider_pool_failover(
            err,
            retries,
            max_retries,
            /*current_round_exhausted*/ false,
            *pool_switch_count,
            pool_size,
            max_rounds,
        ),
        ProviderPoolFailoverAction::SwitchWithinRound
    ) && let Some(ctx) = maybe_switch_provider_account(
        sess,
        turn_context,
        attempted_accounts,
        /*restart_from_first*/ false,
        err,
    )
    .await
    {
        *pool_switch_count += 1;
        return Some(ctx);
    }

    if !matches!(
        decide_provider_pool_failover(
            err,
            retries,
            max_retries,
            /*current_round_exhausted*/ true,
            *pool_switch_count,
            pool_size,
            max_rounds,
        ),
        ProviderPoolFailoverAction::RestartFromFirstAccount
    ) {
        return None;
    }

    attempted_accounts.clear();
    let ctx = maybe_switch_provider_account(
        sess,
        turn_context,
        attempted_accounts,
        /*restart_from_first*/ true,
        err,
    )
    .await?;
    *pool_switch_count += 1;
    Some(ctx)
}

pub(super) async fn maybe_switch_provider_account(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
    restart_from_first: bool,
    err: &CodexErr,
) -> Option<Arc<TurnContext>> {
    let current_account = turn_context.provider.current_account()?;
    let provider_id = turn_context.config.model_provider_id.clone();
    let now = std::time::Instant::now();
    let mut session_configuration = {
        let mut state = sess.state.lock().await;
        state.mark_pool_account_cooling(
            provider_id.as_str(),
            current_account.clone(),
            now,
            PROVIDER_POOL_COOLDOWN,
        );
        state.session_configuration.clone()
    };
    session_configuration.provider_id = turn_context.config.model_provider_id.clone();
    session_configuration.provider = turn_context.config.model_provider.clone();
    session_configuration.collaboration_mode = turn_context.collaboration_mode.clone();
    session_configuration.model_reasoning_summary = Some(turn_context.reasoning_summary);
    session_configuration.developer_instructions = turn_context.developer_instructions.clone();
    session_configuration.user_instructions = turn_context.user_instructions.clone();
    session_configuration.personality = turn_context.personality;
    session_configuration.compact_prompt = turn_context.compact_prompt.clone();
    session_configuration.approval_policy = turn_context.approval_policy.clone();
    session_configuration.sandbox_policy = turn_context.sandbox_policy.clone();
    session_configuration.windows_sandbox_level = turn_context.windows_sandbox_level;
    session_configuration.cwd = turn_context.cwd.clone();
    session_configuration.original_config_do_not_use = Arc::clone(&turn_context.config);
    session_configuration.session_source = turn_context.session_source.clone();
    session_configuration.dynamic_tools = turn_context.dynamic_tools.clone();
    let next_account = next_account_from_pool(
        provider_id.as_str(),
        &turn_context.provider,
        (!restart_from_first).then_some(&current_account),
        attempted_accounts,
    )?;
    let next_provider = turn_context.provider.with_account(&next_account);
    let updated_context = sess
        .new_turn_from_resolved_provider(
            turn_context.sub_id.clone(),
            session_configuration,
            next_provider.clone(),
            Some(turn_context.final_output_json_schema.clone()),
            /*sandbox_policy_changed*/ false,
        )
        .await;
    let current_label = account_index_label(&turn_context.provider);
    let next_label = account_index_label(&next_provider);
    let cooldown_minutes = PROVIDER_POOL_COOLDOWN.as_secs() / 60;
    let action = if restart_from_first {
        format!("all keys already tried; forcing fresh probe from {next_label}")
    } else {
        format!("switching to {next_label}")
    };
    sess.notify_background_event(
        updated_context.as_ref(),
        format!(
            "Provider pool {provider_id}: {current_label} failed ({err}); cooling for {cooldown_minutes}m, {action}"
        ),
    )
    .await;

    Some(updated_context)
}
