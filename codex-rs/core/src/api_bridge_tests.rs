use super::*;
use base64::Engine;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn map_api_error_maps_server_overloaded() {
    let err = map_api_error(ApiError::ServerOverloaded);
    assert!(matches!(err, CodexErr::ServerOverloaded));
}

#[test]
fn map_api_error_maps_server_overloaded_from_503_body() {
    let body = serde_json::json!({
        "error": {
            "code": "server_is_overloaded"
        }
    })
    .to_string();
    let err = map_api_error(ApiError::Transport(TransportError::Http {
        status: http::StatusCode::SERVICE_UNAVAILABLE,
        url: Some("http://example.com/v1/responses".to_string()),
        headers: None,
        body: Some(body),
    }));

    assert!(matches!(err, CodexErr::ServerOverloaded));
}

#[test]
fn map_api_error_maps_usage_limit_limit_name_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACTIVE_LIMIT_HEADER,
        http::HeaderValue::from_static("codex_other"),
    );
    headers.insert(
        "x-codex-other-limit-name",
        http::HeaderValue::from_static("codex_other"),
    );
    let body = serde_json::json!({
        "error": {
            "type": "usage_limit_reached",
            "plan_type": "pro",
        }
    })
    .to_string();
    let err = map_api_error(ApiError::Transport(TransportError::Http {
        status: http::StatusCode::TOO_MANY_REQUESTS,
        url: Some("http://example.com/v1/responses".to_string()),
        headers: Some(headers),
        body: Some(body),
    }));

    let CodexErr::UsageLimitReached(usage_limit) = err else {
        panic!("expected CodexErr::UsageLimitReached, got {err:?}");
    };
    assert_eq!(
        usage_limit
            .rate_limits
            .as_ref()
            .and_then(|snapshot| snapshot.limit_name.as_deref()),
        Some("codex_other")
    );
}

#[test]
fn map_api_error_does_not_fallback_limit_name_to_limit_id() {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACTIVE_LIMIT_HEADER,
        http::HeaderValue::from_static("codex_other"),
    );
    let body = serde_json::json!({
        "error": {
            "type": "usage_limit_reached",
            "plan_type": "pro",
        }
    })
    .to_string();
    let err = map_api_error(ApiError::Transport(TransportError::Http {
        status: http::StatusCode::TOO_MANY_REQUESTS,
        url: Some("http://example.com/v1/responses".to_string()),
        headers: Some(headers),
        body: Some(body),
    }));

    let CodexErr::UsageLimitReached(usage_limit) = err else {
        panic!("expected CodexErr::UsageLimitReached, got {err:?}");
    };
    assert_eq!(
        usage_limit
            .rate_limits
            .as_ref()
            .and_then(|snapshot| snapshot.limit_name.as_deref()),
        None
    );
}

#[test]
fn map_api_error_extracts_identity_auth_details_from_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(REQUEST_ID_HEADER, http::HeaderValue::from_static("req-401"));
    headers.insert(CF_RAY_HEADER, http::HeaderValue::from_static("ray-401"));
    headers.insert(
        X_OPENAI_AUTHORIZATION_ERROR_HEADER,
        http::HeaderValue::from_static("missing_authorization_header"),
    );
    let x_error_json =
        base64::engine::general_purpose::STANDARD.encode(r#"{"error":{"code":"token_expired"}}"#);
    headers.insert(
        X_ERROR_JSON_HEADER,
        http::HeaderValue::from_str(&x_error_json).expect("valid x-error-json header"),
    );

    let err = map_api_error(ApiError::Transport(TransportError::Http {
        status: http::StatusCode::UNAUTHORIZED,
        url: Some("https://chatgpt.com/backend-api/codex/models".to_string()),
        headers: Some(headers),
        body: Some(r#"{"detail":"Unauthorized"}"#.to_string()),
    }));

    let CodexErr::UnexpectedStatus(err) = err else {
        panic!("expected CodexErr::UnexpectedStatus, got {err:?}");
    };
    assert_eq!(err.request_id.as_deref(), Some("req-401"));
    assert_eq!(err.cf_ray.as_deref(), Some("ray-401"));
    assert_eq!(
        err.identity_authorization_error.as_deref(),
        Some("missing_authorization_header")
    );
    assert_eq!(err.identity_error_code.as_deref(), Some("token_expired"));
}

#[test]
fn core_auth_provider_reports_when_auth_header_will_attach() {
    let auth = CoreAuthProvider {
        token: Some("access-token".to_string()),
        account_id: None,
    };

    assert!(auth.auth_header_attached());
    assert_eq!(auth.auth_header_name(), Some("authorization"));
}

#[test]
fn auth_provider_uses_auth_api_key_when_provider_env_key_is_missing() {
    const TEST_ENV_KEY: &str = "CODEX_TEST_PROVIDER_KEY_DO_NOT_SET";
    // SAFETY: tests in this module run in-process and this key is unique to this test.
    unsafe {
        std::env::remove_var(TEST_ENV_KEY);
    }
    let mut provider = ModelProviderInfo::create_openai_provider(/* base_url */ None);
    provider.env_key = Some(TEST_ENV_KEY.to_string());
    provider.env_key_instructions = Some("set the test key".to_string());

    let auth = Some(CodexAuth::from_api_key("xai-auth-key-from-auth-json"));
    let auth_provider = auth_provider_from_auth(auth, &provider)
        .expect("auth api key fallback should work when env key is missing");

    assert_eq!(
        auth_provider.token.as_deref(),
        Some("xai-auth-key-from-auth-json")
    );
}

#[test]
fn auth_provider_prefers_env_key_specific_auth_json_key_when_provider_env_key_is_missing() {
    const TEST_ENV_KEY: &str = "CODEX_TEST_PROVIDER_KEY_DO_NOT_SET";
    // SAFETY: tests in this module run in-process and this key is unique to this test.
    unsafe {
        std::env::remove_var(TEST_ENV_KEY);
    }
    let mut provider = ModelProviderInfo::create_openai_provider(/* base_url */ None);
    provider.env_key = Some(TEST_ENV_KEY.to_string());
    provider.env_key_instructions = Some("set the test key".to_string());

    let auth = Some(CodexAuth::from_api_key_and_env_keys_for_testing(
        "openai-fallback-key",
        HashMap::from([(
            TEST_ENV_KEY.to_string(),
            "provider-specific-key".to_string(),
        )]),
    ));
    let auth_provider = auth_provider_from_auth(auth, &provider)
        .expect("provider-specific auth fallback should work when env key is missing");

    assert_eq!(
        auth_provider.token.as_deref(),
        Some("provider-specific-key")
    );
}

#[test]
fn auth_provider_uses_selected_account_env_key_after_provider_switch() {
    const PRIMARY_ENV_KEY: &str = "CODEX_TEST_POOL_PRIMARY_KEY_DO_NOT_SET";
    const SECONDARY_ENV_KEY: &str = "CODEX_TEST_POOL_SECONDARY_KEY_DO_NOT_SET";
    // SAFETY: tests in this module run in-process and these keys are unique to this test.
    unsafe {
        std::env::remove_var(PRIMARY_ENV_KEY);
        std::env::remove_var(SECONDARY_ENV_KEY);
    }

    let mut provider = ModelProviderInfo::create_anthropic_provider();
    provider.account_pool = vec![
        crate::model_provider_info::ModelProviderAccount {
            base_url: Some("https://pool-primary.example".to_string()),
            env_key: Some(PRIMARY_ENV_KEY.to_string()),
        },
        crate::model_provider_info::ModelProviderAccount {
            base_url: Some("https://pool-secondary.example".to_string()),
            env_key: Some(SECONDARY_ENV_KEY.to_string()),
        },
    ];
    let selected_provider = provider.with_account(&provider.account_pool[1]);

    let auth = Some(CodexAuth::from_api_key_and_env_keys_for_testing(
        "openai-fallback-key",
        HashMap::from([
            (PRIMARY_ENV_KEY.to_string(), "primary-key".to_string()),
            (SECONDARY_ENV_KEY.to_string(), "secondary-key".to_string()),
        ]),
    ));
    let auth_provider = auth_provider_from_auth(auth, &selected_provider)
        .expect("selected account env key should resolve through auth fallback");

    assert_eq!(auth_provider.token.as_deref(), Some("secondary-key"));
}

#[test]
fn auth_provider_keeps_env_key_error_without_auth_fallback() {
    const TEST_ENV_KEY: &str = "CODEX_TEST_PROVIDER_KEY_DO_NOT_SET";
    // SAFETY: tests in this module run in-process and this key is unique to this test.
    unsafe {
        std::env::remove_var(TEST_ENV_KEY);
    }
    let mut provider = ModelProviderInfo::create_openai_provider(/* base_url */ None);
    provider.env_key = Some(TEST_ENV_KEY.to_string());
    provider.env_key_instructions = Some("set the test key".to_string());

    let err = match auth_provider_from_auth(None, &provider) {
        Ok(_) => panic!("missing env key should fail"),
        Err(err) => err,
    };
    let CodexErr::EnvVar(env_err) = err else {
        panic!("expected env var error when auth fallback is unavailable");
    };
    assert_eq!(env_err.var, TEST_ENV_KEY);
}
