use crate::model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::OLLAMA_OSS_PROVIDER_ID;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::built_in_model_providers;
use crate::provider_pool::load_pool_config;
use crate::provider_pool::overlay_pool_config;
use crate::provider_routing::provider_matches_builtin_family;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn validate_reserved_model_provider_ids(
    model_providers: &HashMap<String, ModelProviderInfo>,
) -> Result<(), String> {
    let mut conflicts = model_providers
        .keys()
        .filter(|key| {
            matches!(
                key.as_str(),
                OPENAI_PROVIDER_ID | OLLAMA_OSS_PROVIDER_ID | LMSTUDIO_OSS_PROVIDER_ID
            )
        })
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>();
    conflicts.sort_unstable();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "model_providers contains reserved provider IDs with dedicated config fields: {}. \
Rename your custom provider (for example, `openai-custom`).",
            conflicts.join(", ")
        ))
    }
}

pub(super) fn deserialize_model_providers<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ModelProviderInfo>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let model_providers = HashMap::<String, ModelProviderInfo>::deserialize(deserializer)?;
    validate_reserved_model_provider_ids(&model_providers).map_err(serde::de::Error::custom)?;
    Ok(model_providers)
}

pub(crate) fn build_model_providers(
    codex_home: &Path,
    openai_base_url: Option<String>,
    user_model_providers: HashMap<String, ModelProviderInfo>,
) -> HashMap<String, ModelProviderInfo> {
    let mut model_providers = built_in_model_providers(openai_base_url);

    for (key, provider) in user_model_providers {
        let provider = if let Some(existing) = model_providers.get(&key)
            && provider_matches_builtin_family(existing, &key)
        {
            existing.with_builtin_family_override(&provider)
        } else {
            provider
        };
        model_providers.insert(key, provider);
    }

    if let Some(pool_config) = load_pool_config(codex_home) {
        for key in overlay_pool_config(&mut model_providers, pool_config) {
            tracing::warn!(
                "config-pool.toml references unknown provider '{key}'; \
                 define it in config.toml first"
            );
        }
    }

    model_providers
}

#[cfg(test)]
mod tests {
    use super::build_model_providers;
    use super::validate_reserved_model_provider_ids;
    use crate::config::Config;
    use crate::config::ConfigOverrides;
    use crate::config::ConfigToml;
    use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
    use crate::model_provider_info::ModelProviderAccount;
    use crate::model_provider_info::ModelProviderInfo;
    use crate::model_provider_info::OPENAI_PROVIDER_ID;
    use crate::model_provider_info::WireApi;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn build_model_providers_keeps_builtin_family_identity_with_account_pool() -> std::io::Result<()>
    {
        let codex_home = TempDir::new()?;
        let built_in_anthropic = crate::model_provider_info::built_in_model_providers(None)
            .remove(ANTHROPIC_PROVIDER_ID)
            .expect("anthropic provider should exist");
        let mut model_providers = HashMap::new();
        model_providers.insert(
            ANTHROPIC_PROVIDER_ID.to_string(),
            ModelProviderInfo {
                name: "Anthropic Proxy".to_string(),
                base_url: Some("https://code.ppchat.vip".to_string()),
                env_key: Some("ANTHROPIC_PROXY_API_KEY".to_string()),
                env_key_instructions: None,
                experimental_bearer_token: None,
                wire_api: WireApi::Responses,
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
                        base_url: Some("https://code.ppchat.vip".to_string()),
                        env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
                    },
                    ModelProviderAccount {
                        base_url: Some("https://code.ppchat.vip".to_string()),
                        env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
                    },
                ],
            },
        );

        let built = build_model_providers(codex_home.path(), None, model_providers);
        let anthropic = built
            .get(ANTHROPIC_PROVIDER_ID)
            .expect("anthropic provider should exist");

        assert_eq!(anthropic.name, "Anthropic");
        assert_eq!(anthropic.base_url, built_in_anthropic.base_url);
        assert_eq!(anthropic.env_key, built_in_anthropic.env_key);
        assert_eq!(
            anthropic.account_pool,
            vec![
                ModelProviderAccount {
                    base_url: Some("https://code.ppchat.vip".to_string()),
                    env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://code.ppchat.vip".to_string()),
                    env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
                },
            ]
        );

        Ok(())
    }

    #[test]
    fn build_model_providers_overlays_config_pool_account_pool() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let built_in_anthropic = crate::model_provider_info::built_in_model_providers(None)
            .remove(ANTHROPIC_PROVIDER_ID)
            .expect("anthropic provider should exist");
        std::fs::write(
            codex_home.path().join("config-pool.toml"),
            r#"
[[model_providers.anthropic.account_pool]]
base_url = "https://pool.example"
env_key = "ANTHROPIC_API_KEY_POOL_1"

[[model_providers.anthropic.account_pool]]
base_url = "https://pool.example"
env_key = "ANTHROPIC_API_KEY_POOL_2"
"#,
        )?;

        let built = build_model_providers(
            codex_home.path(),
            None,
            ConfigToml::default().model_providers,
        );
        let anthropic = built
            .get(ANTHROPIC_PROVIDER_ID)
            .expect("anthropic provider should exist");

        assert_eq!(anthropic.base_url, built_in_anthropic.base_url);
        assert_eq!(anthropic.env_key, built_in_anthropic.env_key);
        assert_eq!(
            anthropic.account_pool,
            vec![
                ModelProviderAccount {
                    base_url: Some("https://pool.example".to_string()),
                    env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://pool.example".to_string()),
                    env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
                },
            ]
        );

        Ok(())
    }

    #[test]
    fn builtin_provider_family_override_preserves_upstream_defaults() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let built_in_anthropic = crate::model_provider_info::built_in_model_providers(None)
            .remove(ANTHROPIC_PROVIDER_ID)
            .expect("built-in anthropic provider");
        let mut model_providers = HashMap::new();
        model_providers.insert(
            ANTHROPIC_PROVIDER_ID.to_string(),
            ModelProviderInfo {
                name: "Anthropic Proxy".to_string(),
                base_url: Some("https://code.ppchat.vip".to_string()),
                env_key: Some("ANTHROPIC_PROXY_API_KEY".to_string()),
                env_key_instructions: None,
                experimental_bearer_token: None,
                wire_api: WireApi::Responses,
                query_params: Some(HashMap::from([("routing".to_string(), "pool".to_string())])),
                http_headers: Some(HashMap::from([(
                    "x-custom-header".to_string(),
                    "enabled".to_string(),
                )])),
                env_http_headers: None,
                request_max_retries: Some(9),
                stream_max_retries: None,
                stream_idle_timeout_ms: None,
                websocket_connect_timeout_ms: None,
                requires_openai_auth: true,
                supports_websockets: true,
                account_pool: Vec::new(),
            },
        );

        let cfg = ConfigToml {
            model_provider: Some(ANTHROPIC_PROVIDER_ID.to_string()),
            model_providers,
            ..Default::default()
        };
        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, ANTHROPIC_PROVIDER_ID);
        assert_eq!(config.model_provider.name, built_in_anthropic.name);
        assert_eq!(
            config.model_provider.base_url.as_deref(),
            Some("https://code.ppchat.vip")
        );
        assert_eq!(
            config.model_provider.env_key.as_deref(),
            Some("ANTHROPIC_PROXY_API_KEY")
        );
        assert_eq!(config.model_provider.env_key_instructions, None);
        assert_eq!(config.model_provider.wire_api, built_in_anthropic.wire_api);
        assert_eq!(
            config.model_provider.query_params,
            Some(HashMap::from([("routing".to_string(), "pool".to_string())]))
        );
        assert_eq!(
            config.model_provider.http_headers,
            Some(HashMap::from([
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("x-custom-header".to_string(), "enabled".to_string()),
            ]))
        );
        assert_eq!(config.model_provider.request_max_retries, Some(9));
        assert_eq!(
            config.model_provider.stream_max_retries,
            built_in_anthropic.stream_max_retries
        );
        assert_eq!(
            config.model_provider.supports_websockets,
            built_in_anthropic.supports_websockets
        );

        Ok(())
    }

    #[test]
    fn builtin_provider_family_override_keeps_logical_identity_with_account_pool()
    -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let built_in_anthropic = crate::model_provider_info::built_in_model_providers(None)
            .remove(ANTHROPIC_PROVIDER_ID)
            .expect("built-in anthropic provider");
        let mut model_providers = HashMap::new();
        model_providers.insert(
            ANTHROPIC_PROVIDER_ID.to_string(),
            ModelProviderInfo {
                name: "Anthropic Proxy".to_string(),
                base_url: Some("https://code.ppchat.vip".to_string()),
                env_key: Some("ANTHROPIC_PROXY_API_KEY".to_string()),
                env_key_instructions: None,
                experimental_bearer_token: None,
                wire_api: WireApi::Responses,
                query_params: None,
                http_headers: Some(HashMap::from([(
                    "x-custom-header".to_string(),
                    "enabled".to_string(),
                )])),
                env_http_headers: None,
                request_max_retries: Some(9),
                stream_max_retries: None,
                stream_idle_timeout_ms: None,
                websocket_connect_timeout_ms: None,
                requires_openai_auth: true,
                supports_websockets: true,
                account_pool: vec![
                    ModelProviderAccount {
                        base_url: Some("https://code.ppchat.vip".to_string()),
                        env_key: Some("ANTHROPIC_POOL_1".to_string()),
                    },
                    ModelProviderAccount {
                        base_url: Some("https://code.ppchat.vip".to_string()),
                        env_key: Some("ANTHROPIC_POOL_2".to_string()),
                    },
                ],
            },
        );

        let cfg = ConfigToml {
            model_provider: Some(ANTHROPIC_PROVIDER_ID.to_string()),
            model_providers,
            ..Default::default()
        };
        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, ANTHROPIC_PROVIDER_ID);
        assert_eq!(config.model_provider.name, built_in_anthropic.name);
        assert_eq!(config.model_provider.base_url, built_in_anthropic.base_url);
        assert_eq!(config.model_provider.env_key, built_in_anthropic.env_key);
        assert_eq!(
            config.model_provider.env_key_instructions,
            built_in_anthropic.env_key_instructions
        );
        assert_eq!(config.model_provider.wire_api, built_in_anthropic.wire_api);
        assert_eq!(
            config.model_provider.http_headers,
            Some(HashMap::from([
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ("x-custom-header".to_string(), "enabled".to_string()),
            ]))
        );
        assert_eq!(config.model_provider.request_max_retries, Some(9));
        assert_eq!(
            config.model_provider.account_pool,
            vec![
                ModelProviderAccount {
                    base_url: Some("https://code.ppchat.vip".to_string()),
                    env_key: Some("ANTHROPIC_POOL_1".to_string()),
                },
                ModelProviderAccount {
                    base_url: Some("https://code.ppchat.vip".to_string()),
                    env_key: Some("ANTHROPIC_POOL_2".to_string()),
                },
            ]
        );
        assert_eq!(
            config.model_provider.supports_websockets,
            built_in_anthropic.supports_websockets
        );

        Ok(())
    }

    #[test]
    fn validate_reserved_model_provider_ids_rejects_reserved_keys() {
        let built_in_openai = crate::model_provider_info::built_in_model_providers(None)
            .remove(OPENAI_PROVIDER_ID)
            .expect("openai provider should exist");
        let model_providers = HashMap::from([(OPENAI_PROVIDER_ID.to_string(), built_in_openai)]);

        let error = validate_reserved_model_provider_ids(&model_providers)
            .expect_err("reserved provider keys should be rejected");
        assert!(
            error.contains("reserved provider IDs with dedicated config fields"),
            "unexpected error: {error}"
        );
        assert!(error.contains("`openai`"), "unexpected error: {error}");
    }

    #[test]
    fn validate_reserved_model_provider_ids_allows_custom_keys() {
        let built_in_openai = crate::model_provider_info::built_in_model_providers(None)
            .remove(OPENAI_PROVIDER_ID)
            .expect("openai provider should exist");
        let model_providers = HashMap::from([("openai-custom".to_string(), built_in_openai)]);

        assert_eq!(
            validate_reserved_model_provider_ids(&model_providers),
            Ok(())
        );
    }
}
