use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::built_in_model_providers;
use crate::provider_pool::load_pool_config;
use crate::provider_pool::overlay_pool_config;
use crate::provider_routing::provider_matches_builtin_family;
use std::collections::HashMap;
use std::path::Path;

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
    use crate::config::ConfigToml;
    use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
    use crate::model_provider_info::ModelProviderAccount;
    use crate::model_provider_info::ModelProviderInfo;
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
}
