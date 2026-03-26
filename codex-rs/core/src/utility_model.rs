use crate::client::ModelClient;
use crate::config::Config;
use crate::model_compat::is_openai_model_slug;
use crate::model_provider_info::ModelProviderInfo;
use crate::models_manager::manager::ModelsManager;
use crate::provider_routing::preview_provider_with_first_pool_account;
use crate::provider_routing::provider_id_for_model_slug;
use crate::provider_routing::provider_matches_builtin_family;
use codex_protocol::openai_models::ModelInfo;

pub const DEFAULT_UTILITY_MODEL: &str = "gpt-5.1-codex-mini";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UtilityModelOverrides {
    pub(crate) model_sub: Option<String>,
    pub(crate) model_sub_responses: Option<String>,
    pub(crate) model_sub_responses_warning: Option<String>,
}

fn normalize_model_override(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) fn resolve_utility_model_overrides(
    model_sub: Option<String>,
    model_sub_responses: Option<String>,
) -> UtilityModelOverrides {
    let model_sub = normalize_model_override(model_sub);
    let mut model_sub_responses = normalize_model_override(model_sub_responses);
    let model_sub_responses_warning = model_sub_responses
        .as_deref()
        .filter(|model| !is_openai_model_slug(model))
        .map(|model| {
            format!(
                "Configured `model_sub_responses = \"{model}\"` is not Responses-compatible; Responses-only internal tasks will fall back to OpenAI defaults."
            )
        });
    if model_sub_responses_warning.is_some() {
        model_sub_responses = None;
    }

    UtilityModelOverrides {
        model_sub,
        model_sub_responses,
        model_sub_responses_warning,
    }
}

pub(crate) fn provider_for_model_slug(
    config: &Config,
    model_slug: &str,
) -> Option<(String, ModelProviderInfo)> {
    // For OpenAI model slugs, prefer the current configured Responses provider when available
    // (e.g. openai-custom). Otherwise fall back to the built-in OpenAI provider.
    if is_openai_model_slug(model_slug) {
        if config.model_provider.wire_api == crate::model_provider_info::WireApi::Responses {
            return Some((
                config.model_provider_id.clone(),
                config.model_provider.clone(),
            ));
        }

        // When the active provider is non-Responses (e.g. Claude leader), the user's configured
        // provider may still be a Responses provider. Prefer it so utility requests honor custom
        // providers like openai-custom.
        if config.user_configured_provider.wire_api
            == crate::model_provider_info::WireApi::Responses
        {
            let provider_id = config
                .model_providers
                .iter()
                .filter_map(|(id, candidate)| {
                    (candidate == &config.user_configured_provider).then_some(id.clone())
                })
                .min()
                .unwrap_or_else(|| "openai".to_string());
            return Some((provider_id, config.user_configured_provider.clone()));
        }

        return Some((
            "openai".to_string(),
            config.model_providers.get("openai")?.clone(),
        ));
    }

    if let Some(provider_id) = provider_id_for_model_slug(model_slug) {
        if provider_matches_builtin_family(&config.model_provider, provider_id) {
            return Some((
                config.model_provider_id.clone(),
                config.model_provider.clone(),
            ));
        }

        if provider_matches_builtin_family(&config.user_configured_provider, provider_id) {
            let provider_id = config
                .model_providers
                .iter()
                .filter_map(|(id, candidate)| {
                    (candidate == &config.user_configured_provider).then_some(id.clone())
                })
                .min()
                .unwrap_or_else(|| provider_id.to_string());
            return Some((provider_id, config.user_configured_provider.clone()));
        }

        return Some((
            provider_id.to_string(),
            config.model_providers.get(provider_id)?.clone(),
        ));
    }

    None
}

pub(crate) fn responses_utility_model_slug(config: &Config) -> &str {
    config
        .model_sub_responses
        .as_deref()
        .filter(|model| is_openai_model_slug(model))
        .or_else(|| {
            config
                .model_sub
                .as_deref()
                .filter(|model| is_openai_model_slug(model))
        })
        .unwrap_or(DEFAULT_UTILITY_MODEL)
}

pub(crate) async fn client_and_model_for_slug(
    base_client: &ModelClient,
    models_manager: &ModelsManager,
    config: &Config,
    model_slug: &str,
) -> Option<(ModelClient, ModelInfo, String)> {
    let (provider_id, provider) = provider_for_model_slug(config, model_slug)?;
    let model_info = models_manager.get_model_info(model_slug, config).await;
    let provider = preview_provider_with_first_pool_account(provider_id.as_str(), &provider);
    let model_client = base_client.clone_with_provider(provider);
    Some((model_client, model_info, provider_id))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthManager;
    use crate::auth::CodexAuth;
    use crate::client::ModelClient;
    use crate::config::test_config;
    use crate::model_provider_info::ModelProviderAccount;
    use crate::models_manager::collaboration_mode_presets::CollaborationModesConfig;
    use crate::models_manager::manager::ModelsManager;
    use codex_protocol::ThreadId;
    use codex_protocol::protocol::SessionSource;
    use pretty_assertions::assert_eq;

    #[test]
    fn provider_id_for_model_slug_routes_known_families() {
        assert_eq!(provider_id_for_model_slug("gpt-5.1-codex-mini"), None);
        assert_eq!(
            provider_id_for_model_slug("gemini-3-pro-preview"),
            Some("gemini")
        );
        assert_eq!(provider_id_for_model_slug("gemma-3n"), Some("gemma"));
        assert_eq!(
            provider_id_for_model_slug("claude-opus-4-6"),
            Some("anthropic")
        );
        assert_eq!(provider_id_for_model_slug("grok-4-latest"), Some("grok"));
        assert_eq!(provider_id_for_model_slug("unknown-model"), None);
    }

    #[test]
    fn provider_for_model_slug_defaults_to_openai_for_gpt_models() {
        let config = test_config();
        let (provider_id, provider) = provider_for_model_slug(&config, "gpt-5.1-codex-mini")
            .expect("openai provider should exist in built-in providers");
        assert_eq!(provider_id, config.model_provider_id);
        assert!(provider.is_openai());
    }

    #[test]
    fn provider_for_model_slug_prefers_user_configured_responses_provider_when_auto_switched() {
        let mut config = test_config();

        let openai_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        let openai_custom_provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            ..openai_provider
        };
        config
            .model_providers
            .insert("openai-custom".to_string(), openai_custom_provider.clone());
        config.user_configured_provider = openai_custom_provider;

        config.model_provider_id = "anthropic".to_string();
        config.model_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();

        let (provider_id, provider) = provider_for_model_slug(&config, "gpt-5.1-codex-mini")
            .expect("utility provider for OpenAI slug should exist");
        assert_eq!(provider_id, "openai-custom");
        assert_eq!(provider.name, "OpenAI custom");
        assert_eq!(
            provider.wire_api,
            crate::model_provider_info::WireApi::Responses
        );
    }

    #[test]
    fn provider_for_model_slug_keeps_logical_pool_provider_when_auto_switched() {
        let mut config = test_config();

        let mut openai_custom_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        openai_custom_provider.name = "OpenAI custom".to_string();
        openai_custom_provider.base_url = None;
        openai_custom_provider.env_key = None;
        openai_custom_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://preferred.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://fallback.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
            },
        ];

        config
            .model_providers
            .insert("openai-custom".to_string(), openai_custom_provider.clone());
        config.user_configured_provider = openai_custom_provider;

        config.model_provider_id = "anthropic".to_string();
        config.model_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();

        let (provider_id, provider) = provider_for_model_slug(&config, "gpt-5.1-codex-mini")
            .expect("utility provider for OpenAI slug should exist");
        assert_eq!(provider_id, "openai-custom");
        assert_eq!(provider.base_url, None);
        assert_eq!(provider.env_key, None);
        assert_eq!(provider.account_pool.len(), 2);
    }

    #[test]
    fn provider_for_model_slug_supports_openai_namespaced_slugs() {
        let mut config = test_config();
        config.model_provider_id = "anthropic".to_string();
        config.model_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();

        let openai_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        let openai_custom_provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            ..openai_provider
        };
        config
            .model_providers
            .insert("openai-custom".to_string(), openai_custom_provider.clone());
        config.user_configured_provider = openai_custom_provider;

        let (provider_id, provider) = provider_for_model_slug(&config, "openai/gpt-5.1-codex")
            .expect("utility provider for namespaced OpenAI slug should exist");
        assert_eq!(provider_id, "openai-custom");
        assert_eq!(provider.name, "OpenAI custom");
    }

    #[test]
    fn provider_for_model_slug_prefers_active_provider_for_claude_models() {
        let mut config = test_config();

        let anthropic_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();
        let anthropic_custom_provider = ModelProviderInfo {
            name: "Anthropic custom".to_string(),
            ..anthropic_provider
        };
        config.model_providers.insert(
            "anthropic-custom".to_string(),
            anthropic_custom_provider.clone(),
        );
        config.model_provider_id = "anthropic-custom".to_string();
        config.model_provider = anthropic_custom_provider.clone();
        config.user_configured_provider = anthropic_custom_provider;

        let (provider_id, provider) = provider_for_model_slug(&config, "claude-opus-4-6")
            .expect("utility provider for Claude slug should exist");
        assert_eq!(provider_id, "anthropic-custom");
        assert_eq!(provider.name, "Anthropic custom");
        assert_eq!(
            provider.wire_api,
            crate::model_provider_info::WireApi::Anthropic
        );
    }

    #[test]
    fn responses_utility_model_slug_uses_responses_override_then_general_then_default() {
        let mut config = test_config();
        assert_eq!(responses_utility_model_slug(&config), DEFAULT_UTILITY_MODEL);

        config.model_sub = Some("claude-sonnet-4-6".to_string());
        assert_eq!(responses_utility_model_slug(&config), DEFAULT_UTILITY_MODEL);

        config.model_sub = Some("openai/o4-mini".to_string());
        assert_eq!(responses_utility_model_slug(&config), "openai/o4-mini");

        config.model_sub_responses = Some("gpt-5.1-codex-mini".to_string());
        assert_eq!(responses_utility_model_slug(&config), "gpt-5.1-codex-mini");
    }

    #[tokio::test]
    async fn client_and_model_for_slug_starts_from_first_pool_account() {
        let auth_manager =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
        let mut config = test_config();

        let mut openai_custom_provider = config
            .model_providers
            .get("openai")
            .expect("openai provider should exist")
            .clone();
        openai_custom_provider.name = "OpenAI custom".to_string();
        openai_custom_provider.base_url = None;
        openai_custom_provider.env_key = None;
        openai_custom_provider.account_pool = vec![
            ModelProviderAccount {
                base_url: Some("https://preferred.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            },
            ModelProviderAccount {
                base_url: Some("https://fallback.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
            },
        ];
        config
            .model_providers
            .insert("openai-custom".to_string(), openai_custom_provider.clone());
        config.user_configured_provider = openai_custom_provider.clone();

        config.model_provider_id = "anthropic".to_string();
        config.model_provider = config
            .model_providers
            .get("anthropic")
            .expect("anthropic provider should exist")
            .clone();

        let models_manager = ModelsManager::new(
            config.codex_home.clone(),
            auth_manager.clone(),
            None,
            CollaborationModesConfig::default(),
        );
        let base_client = ModelClient::new(
            Some(auth_manager),
            ThreadId::default(),
            config.model_provider.clone(),
            SessionSource::Exec,
            config.model_verbosity,
            /*enable_request_compression*/ false,
            /*include_timing_metrics*/ false,
            /*beta_features_header*/ None,
        );

        let (model_client, _model_info, provider_id) =
            client_and_model_for_slug(&base_client, &models_manager, &config, "gpt-5.1-codex-mini")
                .await
                .expect("utility provider should resolve");

        assert_eq!(provider_id, "openai-custom");
        assert_eq!(
            model_client.provider_for_test().current_account(),
            Some(openai_custom_provider.account_pool[0].clone())
        );
    }
}
