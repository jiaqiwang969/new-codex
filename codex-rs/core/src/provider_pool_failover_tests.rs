use crate::error::CodexErr;
use crate::error::EnvVarError;
use crate::error::UnexpectedResponseError;
use crate::error::UsageLimitReachedError;
use crate::provider_pool_failover::ProviderPoolFailoverAction;
use crate::provider_pool_failover::decide_provider_pool_failover;
use crate::token_data::PlanType;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

#[test]
fn decide_provider_pool_failover_switches_immediately_for_auth_and_http_status_errors() {
    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::EnvVar(EnvVarError {
                var: "ANTHROPIC_API_KEY".to_string(),
                instructions: None,
            }),
            0,
            3,
            /*current_round_exhausted*/ false,
            0,
            2,
            2,
        ),
        ProviderPoolFailoverAction::SwitchWithinRound
    );

    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::UnexpectedStatus(UnexpectedResponseError {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: "quota".to_string(),
                url: None,
                cf_ray: None,
                request_id: None,
                identity_authorization_error: None,
                identity_error_code: None,
            }),
            0,
            3,
            /*current_round_exhausted*/ false,
            0,
            2,
            2,
        ),
        ProviderPoolFailoverAction::SwitchWithinRound
    );

    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::UsageLimitReached(UsageLimitReachedError {
                plan_type: Some(PlanType::Unknown("custom".to_string())),
                resets_at: None,
                rate_limits: None,
                promo_message: None,
            }),
            0,
            3,
            /*current_round_exhausted*/ false,
            0,
            2,
            2,
        ),
        ProviderPoolFailoverAction::SwitchWithinRound
    );
}

#[test]
fn decide_provider_pool_failover_only_switches_retryable_errors_after_retry_budget_is_spent() {
    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::InternalServerError,
            1,
            2,
            /*current_round_exhausted*/ false,
            0,
            2,
            2,
        ),
        ProviderPoolFailoverAction::StayOnCurrentAccount
    );

    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::InternalServerError,
            2,
            2,
            /*current_round_exhausted*/ false,
            0,
            2,
            2,
        ),
        ProviderPoolFailoverAction::SwitchWithinRound
    );
}

#[test]
fn decide_provider_pool_failover_restarts_only_when_another_round_is_available() {
    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::InternalServerError,
            2,
            2,
            /*current_round_exhausted*/ true,
            1,
            2,
            2,
        ),
        ProviderPoolFailoverAction::RestartFromFirstAccount
    );

    assert_eq!(
        decide_provider_pool_failover(
            &CodexErr::InternalServerError,
            2,
            2,
            /*current_round_exhausted*/ true,
            3,
            2,
            2,
        ),
        ProviderPoolFailoverAction::StayOnCurrentAccount
    );
}
