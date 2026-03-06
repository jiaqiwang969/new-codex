use chrono::DateTime;
use chrono::Utc;
use codex_api::AuthProvider as ApiAuthProvider;
use codex_api::TransportError;
use codex_api::error::ApiError;
use codex_api::rate_limits::parse_promo_message;
use codex_api::rate_limits::parse_rate_limit_for_limit;
use http::HeaderMap;
use serde::Deserialize;

use crate::auth::CodexAuth;
use crate::error::CodexErr;
use crate::error::RetryLimitReachedError;
use crate::error::UnexpectedResponseError;
use crate::error::UsageLimitReachedError;
use crate::model_provider_info::ModelProviderInfo;
use crate::token_data::PlanType;

pub(crate) fn map_api_error(err: ApiError) -> CodexErr {
    match err {
        ApiError::ContextWindowExceeded => CodexErr::ContextWindowExceeded,
        ApiError::QuotaExceeded => CodexErr::QuotaExceeded,
        ApiError::UsageNotIncluded => CodexErr::UsageNotIncluded,
        ApiError::Retryable { message, delay } => CodexErr::Stream(message, delay),
        ApiError::Stream(msg) => CodexErr::Stream(msg, None),
        ApiError::ServerOverloaded => CodexErr::ServerOverloaded,
        ApiError::Api { status, message } => CodexErr::UnexpectedStatus(UnexpectedResponseError {
            status,
            body: message,
            url: None,
            cf_ray: None,
            request_id: None,
        }),
        ApiError::InvalidRequest { message } => CodexErr::InvalidRequest(message),
        ApiError::Transport(transport) => match transport {
            TransportError::Http {
                status,
                url,
                headers,
                body,
            } => {
                let body_text = body.unwrap_or_default();

                if status == http::StatusCode::SERVICE_UNAVAILABLE
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&body_text)
                    && matches!(
                        value
                            .get("error")
                            .and_then(|error| error.get("code"))
                            .and_then(serde_json::Value::as_str),
                        Some("server_is_overloaded" | "slow_down")
                    )
                {
                    return CodexErr::ServerOverloaded;
                }

                if status == http::StatusCode::BAD_REQUEST {
                    if body_text
                        .contains("The image data you provided does not represent a valid image")
                    {
                        CodexErr::InvalidImageRequest()
                    } else {
                        CodexErr::InvalidRequest(body_text)
                    }
                } else if status == http::StatusCode::INTERNAL_SERVER_ERROR {
                    CodexErr::InternalServerError
                } else if status == http::StatusCode::TOO_MANY_REQUESTS {
                    if let Ok(err) = serde_json::from_str::<UsageErrorResponse>(&body_text) {
                        if err.error.error_type.as_deref() == Some("usage_limit_reached") {
                            let limit_id = extract_header(headers.as_ref(), ACTIVE_LIMIT_HEADER);
                            let rate_limits = headers.as_ref().and_then(|map| {
                                parse_rate_limit_for_limit(map, limit_id.as_deref())
                            });
                            let promo_message = headers.as_ref().and_then(parse_promo_message);
                            let resets_at = err
                                .error
                                .resets_at
                                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
                            return CodexErr::UsageLimitReached(UsageLimitReachedError {
                                plan_type: err.error.plan_type,
                                resets_at,
                                rate_limits: rate_limits.map(Box::new),
                                promo_message,
                            });
                        } else if err.error.error_type.as_deref() == Some("usage_not_included") {
                            return CodexErr::UsageNotIncluded;
                        }
                    }

                    CodexErr::RetryLimit(RetryLimitReachedError {
                        status,
                        request_id: extract_request_tracking_id(headers.as_ref()),
                    })
                } else {
                    CodexErr::UnexpectedStatus(UnexpectedResponseError {
                        status,
                        body: body_text,
                        url,
                        cf_ray: extract_header(headers.as_ref(), CF_RAY_HEADER),
                        request_id: extract_request_id(headers.as_ref()),
                    })
                }
            }
            TransportError::RetryLimit => CodexErr::RetryLimit(RetryLimitReachedError {
                status: http::StatusCode::INTERNAL_SERVER_ERROR,
                request_id: None,
            }),
            TransportError::Timeout => CodexErr::Timeout,
            TransportError::Network(msg) | TransportError::Build(msg) => {
                CodexErr::Stream(msg, None)
            }
        },
        ApiError::RateLimit(msg) => CodexErr::Stream(msg, None),
    }
}

const ACTIVE_LIMIT_HEADER: &str = "x-codex-active-limit";
const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthCredentialsStoreMode;
    use crate::auth::AuthDotJson;
    use crate::auth::CodexAuth;
    use crate::auth::save_auth;
    use codex_app_server_protocol::AuthMode as ApiAuthMode;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    use tempfile::tempdir;

    fn unique_env_key(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        format!("{prefix}_{nanos}")
    }

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
    fn auth_provider_prefers_env_key_specific_auth_json_key_when_provider_env_key_is_missing() {
        let codex_home = tempdir().expect("create tempdir");
        let env_key = unique_env_key("CODEX_TEST_PROVIDER_AUTH_JSON_FALLBACK_KEY");
        let auth = AuthDotJson {
            auth_mode: Some(ApiAuthMode::ApiKey),
            openai_api_key: Some("sk-openai-key".to_string()),
            provider_api_keys: HashMap::from([(
                env_key.to_string(),
                "sk-provider-fallback-key".to_string(),
            )]),
            tokens: None,
            last_refresh: None,
        };
        save_auth(codex_home.path(), &auth, AuthCredentialsStoreMode::File)
            .expect("save auth json");
        let loaded_auth =
            CodexAuth::from_auth_storage(codex_home.path(), AuthCredentialsStoreMode::File)
                .expect("load auth")
                .expect("auth should exist");

        let mut provider = ModelProviderInfo::create_openai_provider();
        provider.env_key = Some(env_key);
        let core_auth = auth_provider_from_auth(Some(loaded_auth), &provider)
            .expect("fallback key from auth.json should be used");

        assert_eq!(
            core_auth.bearer_token().as_deref(),
            Some("sk-provider-fallback-key")
        );
        assert_eq!(core_auth.account_id(), None);
    }

    #[test]
    fn auth_provider_keeps_env_key_error_without_auth_fallback() {
        let codex_home = tempdir().expect("create tempdir");
        let auth = AuthDotJson {
            auth_mode: Some(ApiAuthMode::ApiKey),
            openai_api_key: Some("sk-openai-key".to_string()),
            provider_api_keys: HashMap::new(),
            tokens: None,
            last_refresh: None,
        };
        save_auth(codex_home.path(), &auth, AuthCredentialsStoreMode::File)
            .expect("save auth json");
        let loaded_auth =
            CodexAuth::from_auth_storage(codex_home.path(), AuthCredentialsStoreMode::File)
                .expect("load auth")
                .expect("auth should exist");

        let env_key = unique_env_key("CODEX_TEST_PROVIDER_AUTH_JSON_MISSING_KEY");
        let mut provider = ModelProviderInfo::create_openai_provider();
        provider.env_key = Some(env_key.clone());
        let err = match auth_provider_from_auth(Some(loaded_auth), &provider) {
            Ok(_) => {
                panic!("missing env key should still error when fallback key is absent");
            }
            Err(err) => err,
        };
        let CodexErr::EnvVar(env_error) = err else {
            panic!("expected env-var error, got {err:?}");
        };
        assert_eq!(env_error.var, env_key);
    }
}

fn extract_request_tracking_id(headers: Option<&HeaderMap>) -> Option<String> {
    extract_request_id(headers).or_else(|| extract_header(headers, CF_RAY_HEADER))
}

fn extract_request_id(headers: Option<&HeaderMap>) -> Option<String> {
    extract_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_header(headers, OAI_REQUEST_ID_HEADER))
}

fn extract_header(headers: Option<&HeaderMap>, name: &str) -> Option<String> {
    headers.and_then(|map| {
        map.get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    })
}

pub(crate) fn auth_provider_from_auth(
    auth: Option<CodexAuth>,
    provider: &ModelProviderInfo,
) -> crate::error::Result<CoreAuthProvider> {
    let provider_api_key = match provider.api_key() {
        Ok(provider_api_key) => provider_api_key,
        Err(CodexErr::EnvVar(env_var_error)) => {
            if let Some(api_key) = auth
                .as_ref()
                .and_then(|auth| auth.provider_api_key_for_env_key(env_var_error.var.as_str()))
            {
                Some(api_key)
            } else {
                return Err(CodexErr::EnvVar(env_var_error));
            }
        }
        Err(err) => return Err(err),
    };

    if let Some(api_key) = provider_api_key {
        return Ok(CoreAuthProvider {
            token: Some(api_key),
            account_id: None,
        });
    }

    if let Some(token) = provider.experimental_bearer_token.clone() {
        return Ok(CoreAuthProvider {
            token: Some(token),
            account_id: None,
        });
    }

    if let Some(auth) = auth {
        let token = auth.get_token()?;
        Ok(CoreAuthProvider {
            token: Some(token),
            account_id: auth.get_account_id(),
        })
    } else {
        Ok(CoreAuthProvider {
            token: None,
            account_id: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct UsageErrorResponse {
    error: UsageErrorBody,
}

#[derive(Debug, Deserialize)]
struct UsageErrorBody {
    #[serde(rename = "type")]
    error_type: Option<String>,
    plan_type: Option<PlanType>,
    resets_at: Option<i64>,
}

#[derive(Clone, Default)]
pub(crate) struct CoreAuthProvider {
    token: Option<String>,
    account_id: Option<String>,
}

impl ApiAuthProvider for CoreAuthProvider {
    fn bearer_token(&self) -> Option<String> {
        self.token.clone()
    }

    fn account_id(&self) -> Option<String> {
        self.account_id.clone()
    }
}
