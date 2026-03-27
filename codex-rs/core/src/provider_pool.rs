use crate::model_provider_info::ModelProviderAccount;
use crate::model_provider_info::ModelProviderInfo;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub(crate) const CONFIG_POOL_TOML_FILE: &str = "config-pool.toml";

/// A lightweight provider entry used only in `config-pool.toml`.
/// Only `account_pool` is honored. Legacy top-level provider fields are
/// ignored so pool mode stays isolated from logical provider configuration.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PoolProviderEntry {
    #[serde(default)]
    pub(crate) account_pool: Vec<ModelProviderAccount>,
}

/// Subset of config that lives in `config-pool.toml`.
/// Only contains model providers with their pool-related entries.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigPoolToml {
    #[serde(default)]
    pub(crate) model_providers: HashMap<String, PoolProviderEntry>,
}

pub(crate) fn load_pool_config(codex_home: &Path) -> Option<ConfigPoolToml> {
    let pool_path = codex_home.join(CONFIG_POOL_TOML_FILE);
    let contents = std::fs::read_to_string(&pool_path).ok()?;
    match toml::from_str(&contents) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            tracing::warn!("failed to parse {CONFIG_POOL_TOML_FILE}: {e}");
            None
        }
    }
}

pub(crate) fn overlay_pool_config(
    model_providers: &mut HashMap<String, ModelProviderInfo>,
    pool_config: ConfigPoolToml,
) -> Vec<String> {
    let mut unknown_providers = Vec::new();

    for (key, pool_entry) in pool_config.model_providers {
        if let Some(existing) = model_providers.get_mut(&key) {
            if !pool_entry.account_pool.is_empty() {
                existing.account_pool = pool_entry.account_pool;
            }
        } else {
            unknown_providers.push(key);
        }
    }

    unknown_providers
}

#[cfg(test)]
mod tests {
    use super::CONFIG_POOL_TOML_FILE;
    use super::ConfigPoolToml;
    use super::PoolProviderEntry;
    use super::load_pool_config;
    use super::overlay_pool_config;
    use crate::model_provider_info::ModelProviderAccount;
    use crate::model_provider_info::ModelProviderInfo;
    use crate::model_provider_info::WireApi;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn provider(base_url: Option<&str>, env_key: Option<&str>) -> ModelProviderInfo {
        ModelProviderInfo {
            name: "Anthropic".to_string(),
            base_url: base_url.map(str::to_string),
            env_key: env_key.map(str::to_string),
            env_key_instructions: None,
            experimental_bearer_token: None,
            wire_api: WireApi::Anthropic,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool: Vec::new(),
        }
    }

    #[test]
    fn overlay_pool_config_replaces_account_pool_but_preserves_existing_base_fields() {
        let mut model_providers = HashMap::from([(
            "anthropic".to_string(),
            provider(Some("https://api.anthropic.com"), Some("ANTHROPIC_API_KEY")),
        )]);

        let pool_config = ConfigPoolToml {
            model_providers: HashMap::from([(
                "anthropic".to_string(),
                PoolProviderEntry {
                    account_pool: vec![
                        ModelProviderAccount {
                            base_url: Some("https://pool.example".to_string()),
                            env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
                        },
                        ModelProviderAccount {
                            base_url: Some("https://pool.example".to_string()),
                            env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
                        },
                    ],
                },
            )]),
        };

        let unknown = overlay_pool_config(&mut model_providers, pool_config);
        assert_eq!(unknown, Vec::<String>::new());

        let provider = model_providers
            .get("anthropic")
            .expect("anthropic provider should exist");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(provider.env_key.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert_eq!(
            provider.account_pool,
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
    }

    #[test]
    fn overlay_pool_config_ignores_legacy_top_level_base_fields_without_account_pool() {
        let mut model_providers = HashMap::from([(
            "anthropic".to_string(),
            provider(Some("https://api.anthropic.com"), Some("ANTHROPIC_API_KEY")),
        )]);

        let pool_config = ConfigPoolToml {
            model_providers: HashMap::from([(
                "anthropic".to_string(),
                PoolProviderEntry {
                    account_pool: Vec::new(),
                },
            )]),
        };

        let unknown = overlay_pool_config(&mut model_providers, pool_config);
        assert_eq!(unknown, Vec::<String>::new());

        let provider = model_providers
            .get("anthropic")
            .expect("anthropic provider should exist");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(provider.env_key.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert_eq!(provider.account_pool, Vec::<ModelProviderAccount>::new());
    }

    #[test]
    fn load_pool_config_reads_account_pool_only_entries() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        std::fs::write(
            codex_home.path().join(CONFIG_POOL_TOML_FILE),
            r#"
[[model_providers.anthropic.account_pool]]
base_url = "https://pool.example"
env_key = "ANTHROPIC_API_KEY_POOL_1"

[[model_providers.anthropic.account_pool]]
base_url = "https://pool.example"
env_key = "ANTHROPIC_API_KEY_POOL_2"
"#,
        )?;

        let loaded = load_pool_config(codex_home.path()).expect("pool config should load");
        assert_eq!(
            loaded,
            ConfigPoolToml {
                model_providers: HashMap::from([(
                    "anthropic".to_string(),
                    PoolProviderEntry {
                        account_pool: vec![
                            ModelProviderAccount {
                                base_url: Some("https://pool.example".to_string()),
                                env_key: Some("ANTHROPIC_API_KEY_POOL_1".to_string()),
                            },
                            ModelProviderAccount {
                                base_url: Some("https://pool.example".to_string()),
                                env_key: Some("ANTHROPIC_API_KEY_POOL_2".to_string()),
                            },
                        ],
                    },
                )]),
            }
        );

        Ok(())
    }
}
