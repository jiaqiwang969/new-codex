use crate::error::CodexErr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderPoolFailoverAction {
    StayOnCurrentAccount,
    SwitchWithinRound,
    RestartFromFirstAccount,
}

pub(crate) fn should_switch_provider_account(
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
) -> bool {
    // Auth / quota errors should immediately try the next pool account.
    if matches!(
        err,
        CodexErr::EnvVar(_)
            | CodexErr::RetryLimit(_)
            | CodexErr::UsageLimitReached(_)
            | CodexErr::InvalidRequest(_)
    ) {
        return true;
    }
    if let Some(status) = err.http_status_code_value()
        && matches!(status, 400 | 401 | 403 | 429)
    {
        return true;
    }
    err.is_retryable() && retries >= max_retries
}

pub(crate) fn decide_provider_pool_failover(
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
    current_round_exhausted: bool,
    pool_switch_count: usize,
    pool_size: usize,
    max_rounds: usize,
) -> ProviderPoolFailoverAction {
    if !should_switch_provider_account(err, retries, max_retries) {
        return ProviderPoolFailoverAction::StayOnCurrentAccount;
    }
    if !current_round_exhausted {
        return ProviderPoolFailoverAction::SwitchWithinRound;
    }

    let completed_rounds = if pool_size > 0 {
        (pool_switch_count + 1) / pool_size
    } else {
        max_rounds
    };
    if completed_rounds >= max_rounds {
        ProviderPoolFailoverAction::StayOnCurrentAccount
    } else {
        ProviderPoolFailoverAction::RestartFromFirstAccount
    }
}
