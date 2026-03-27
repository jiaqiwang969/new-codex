use std::collections::HashMap;
use std::collections::HashSet;

use crate::model_compat::is_anthropic_model_slug;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::is_grok_model_slug;
use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
use crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID;
use crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID;
use crate::model_provider_info::GEMINI_PROVIDER_ID;
use crate::model_provider_info::GEMMA_PROVIDER_ID;
use crate::model_provider_info::GROK_PROVIDER_ID;
use crate::model_provider_info::ModelProviderAccount;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::WireApi;

pub(crate) fn provider_id_for_model_slug(model_slug: &str) -> Option<&'static str> {
    if model_slug.starts_with("antigravity/claude-")
        || model_slug.starts_with("antigravity-anthropic/")
    {
        Some(ANTIGRAVITY_ANTHROPIC_PROVIDER_ID)
    } else if model_slug.starts_with("antigravity/")
        || model_slug.starts_with("antigravity-gemini/")
    {
        Some(ANTIGRAVITY_GEMINI_PROVIDER_ID)
    } else if is_gemma_model_slug(model_slug) {
        Some(GEMMA_PROVIDER_ID)
    } else if model_slug.starts_with("gemini-") {
        Some(GEMINI_PROVIDER_ID)
    } else if is_anthropic_model_slug(model_slug) {
        Some(ANTHROPIC_PROVIDER_ID)
    } else if is_grok_model_slug(model_slug) {
        Some(GROK_PROVIDER_ID)
    } else {
        None
    }
}

pub(crate) fn provider_matches_builtin_family(
    provider: &ModelProviderInfo,
    provider_id: &str,
) -> bool {
    match provider_id {
        GEMINI_PROVIDER_ID => {
            provider.wire_api == WireApi::Gemini && !provider.is_antigravity_gemini()
        }
        GEMMA_PROVIDER_ID => {
            provider.is_gemma()
                || (provider.wire_api == WireApi::Gemini
                    && !provider.is_gemini()
                    && !provider.is_antigravity_gemini())
        }
        ANTHROPIC_PROVIDER_ID => {
            provider.wire_api == WireApi::Anthropic && !provider.is_antigravity_anthropic()
        }
        ANTIGRAVITY_GEMINI_PROVIDER_ID => provider.is_antigravity_gemini(),
        ANTIGRAVITY_ANTHROPIC_PROVIDER_ID => provider.is_antigravity_anthropic(),
        GROK_PROVIDER_ID => provider.is_grok(),
        _ => false,
    }
}

pub(crate) fn providers_match_ignoring_active_account(
    left: &ModelProviderInfo,
    right: &ModelProviderInfo,
) -> bool {
    let mut normalized_left = left.clone();
    if !normalized_left.account_pool.is_empty() {
        normalized_left.base_url = None;
        normalized_left.env_key = None;
    }
    let mut normalized_right = right.clone();
    if !normalized_right.account_pool.is_empty() {
        normalized_right.base_url = None;
        normalized_right.env_key = None;
    }
    normalized_left == normalized_right
}

fn pick_preferred_provider_id(mut ids: Vec<String>) -> String {
    if ids.len() == 1 {
        return ids.remove(0);
    }

    ids.sort();
    if let Some(openai_id) = ids.iter().find(|id| id.as_str() == OPENAI_PROVIDER_ID) {
        return openai_id.clone();
    }
    ids.remove(0)
}

pub(crate) fn resolve_provider_id_for_provider(
    providers: &HashMap<String, ModelProviderInfo>,
    provider: &ModelProviderInfo,
    fallback_provider_id: &str,
) -> String {
    if let Some(candidate) = providers.get(fallback_provider_id)
        && providers_match_ignoring_active_account(candidate, provider)
    {
        return fallback_provider_id.to_string();
    }

    let identity_matches = providers
        .iter()
        .filter_map(|(id, candidate)| {
            providers_match_ignoring_active_account(candidate, provider).then_some(id.clone())
        })
        .collect::<Vec<_>>();
    if !identity_matches.is_empty() {
        return pick_preferred_provider_id(identity_matches);
    }

    if let Some(candidate) = providers.get(fallback_provider_id)
        && candidate.name == provider.name
        && candidate.wire_api == provider.wire_api
    {
        return fallback_provider_id.to_string();
    }

    let name_matches = providers
        .iter()
        .filter_map(|(id, candidate)| {
            (candidate.name == provider.name && candidate.wire_api == provider.wire_api)
                .then_some(id.clone())
        })
        .collect::<Vec<_>>();
    if !name_matches.is_empty() {
        return pick_preferred_provider_id(name_matches);
    }

    if provider.wire_api == WireApi::Responses
        && let Some(openai_provider) = providers.get(OPENAI_PROVIDER_ID)
        && openai_provider.wire_api == WireApi::Responses
    {
        return OPENAI_PROVIDER_ID.to_string();
    }

    fallback_provider_id.to_string()
}

pub(crate) fn normalize_account_pool_in_config_order(
    provider_id: &str,
    provider: &ModelProviderInfo,
) -> Vec<ModelProviderAccount> {
    if provider.account_pool.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    provider
        .account_pool
        .iter()
        .cloned()
        .filter_map(|account| {
            let base_url = account
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let env_key = account
                .env_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let normalized = ModelProviderAccount { base_url, env_key };
            if normalized.base_url.is_none() || normalized.env_key.is_none() {
                tracing::warn!(
                    "Skipping account entry for provider {provider_id}: missing base_url or env_key"
                );
                None
            } else if seen.insert(normalized.clone()) {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn preview_provider_with_first_pool_account(
    provider_id: &str,
    provider: &ModelProviderInfo,
) -> ModelProviderInfo {
    normalize_account_pool_in_config_order(provider_id, provider)
        .first()
        .map(|account| provider.with_account(account))
        .unwrap_or_else(|| provider.clone())
}

/// Return a human-readable label like "key 1/3" indicating which account
/// from the pool is currently active. Falls back to the env_key name when
/// there is no multi-entry pool match.
pub(crate) fn account_index_label(provider: &ModelProviderInfo) -> String {
    if let Some(current) = provider.current_account() {
        let pool = normalize_account_pool_in_config_order("", provider);
        if pool.len() > 1
            && let Some(idx) = pool.iter().position(|account| account == &current)
        {
            return format!("key {}/{}", idx + 1, pool.len());
        }
        current.env_key.unwrap_or_else(|| "<default>".to_string())
    } else {
        "<no account>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::ConfigOverrides;
    use crate::config::ConfigToml;
    use crate::config::test_config;
    use crate::config_loader::ConfigLayerStack;
    use crate::model_provider_info::ModelProviderAccount;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tempfile::TempDir;

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
    fn resolve_provider_id_for_provider_matches_pool_provider_ignoring_active_account() {
        let mut config = test_config();
        let mut openai_custom_provider = config
            .model_providers
            .get(OPENAI_PROVIDER_ID)
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

        let active_provider =
            openai_custom_provider.with_account(&openai_custom_provider.account_pool[1]);

        assert_eq!(
            resolve_provider_id_for_provider(
                &config.model_providers,
                &active_provider,
                OPENAI_PROVIDER_ID,
            ),
            "openai-custom"
        );
    }

    #[test]
    fn normalize_account_pool_in_config_order_skips_invalid_entries_and_dedupes() {
        let provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            base_url: None,
            env_key: None,
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_SKIP".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
        };

        assert_eq!(
            normalize_account_pool_in_config_order("openai-custom", &provider),
            vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ]
        );
    }

    #[test]
    fn preview_provider_with_first_pool_account_uses_first_normalized_account() {
        let provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            base_url: None,
            env_key: None,
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_SKIP".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
        };

        let preview_provider = preview_provider_with_first_pool_account("openai-custom", &provider);

        assert_eq!(
            preview_provider.current_account(),
            Some(ModelProviderAccount {
                base_url: Some("https://preferred.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            })
        );
        assert_eq!(preview_provider.account_pool, provider.account_pool);
    }

    #[test]
    fn account_index_label_uses_normalized_pool_position() {
        let provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            base_url: None,
            env_key: None,
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_SKIP".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("OPENAI_API_KEY_POOL_2".to_string()),
                },
            ],
        };

        let active_provider = provider.with_account(&provider.account_pool[3]);

        assert_eq!(account_index_label(&active_provider), "key 2/2");
    }

    #[test]
    fn account_index_label_falls_back_to_current_env_key_without_pool_position() {
        let provider = ModelProviderInfo {
            name: "OpenAI custom".to_string(),
            base_url: Some("https://preferred.example/v1".to_string()),
            env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![ModelProviderAccount {
                base_url: Some("https://preferred.example/v1".to_string()),
                env_key: Some("OPENAI_API_KEY_POOL_1".to_string()),
            }],
        };

        assert_eq!(account_index_label(&provider), "OPENAI_API_KEY_POOL_1");
    }

    #[test]
    fn account_pool_primary_entry_is_not_selected_on_config_load() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let provider_id = "openai-main".to_string();
        let provider = ModelProviderInfo {
            name: "OpenAI Main".to_string(),
            base_url: None,
            env_key: None,
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("KEY_PREFERRED".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://fallback.example/v1".to_string()),
                    env_key: Some("KEY_FALLBACK".to_string()),
                },
            ],
        };
        let mut model_providers = HashMap::new();
        model_providers.insert(provider_id.clone(), provider);

        let cfg = ConfigToml {
            model_provider: Some(provider_id),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_config_with_layer_stack(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
            ConfigLayerStack::default(),
        )?;

        assert_eq!(config.model_provider.base_url, None);
        assert_eq!(config.model_provider.env_key, None);
        assert_eq!(config.user_configured_provider.base_url, None);
        assert_eq!(config.user_configured_provider.env_key, None);
        assert_eq!(config.model_provider.account_pool.len(), 2);
        assert_eq!(config.user_configured_provider.account_pool.len(), 2);

        Ok(())
    }

    #[test]
    fn account_pool_ignores_invalid_entries_without_selecting_first_valid_entry()
    -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let provider_id = "openai-main".to_string();
        let provider = ModelProviderInfo {
            name: "OpenAI Main".to_string(),
            base_url: None,
            env_key: None,
            wire_api: WireApi::Responses,
            env_key_instructions: None,
            experimental_bearer_token: None,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: vec![
                ModelProviderAccount {
                    base_url: Some("".to_string()),
                    env_key: Some("KEY_SKIP".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://preferred.example/v1".to_string()),
                    env_key: Some("KEY_PREFERRED".to_string()),
                },
            ],
        };
        let mut model_providers = HashMap::new();
        model_providers.insert(provider_id.clone(), provider);

        let cfg = ConfigToml {
            model_provider: Some(provider_id),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_config_with_layer_stack(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
            ConfigLayerStack::default(),
        )?;

        assert_eq!(config.model_provider.base_url, None);
        assert_eq!(config.model_provider.env_key, None);
        assert_eq!(config.model_provider.account_pool.len(), 2);

        Ok(())
    }
}
