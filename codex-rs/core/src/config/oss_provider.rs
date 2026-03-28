use super::ConfigToml;
use crate::config::edit::ConfigEdit;
use crate::config::edit::ConfigEditsBuilder;
use crate::model_provider_info::LEGACY_OLLAMA_CHAT_PROVIDER_ID;
use crate::model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
use crate::model_provider_info::OLLAMA_CHAT_PROVIDER_REMOVED_ERROR;
use crate::model_provider_info::OLLAMA_OSS_PROVIDER_ID;
use std::path::Path;

/// Save the default OSS provider preference to config.toml
pub fn set_default_oss_provider(codex_home: &Path, provider: &str) -> std::io::Result<()> {
    match provider {
        LMSTUDIO_OSS_PROVIDER_ID | OLLAMA_OSS_PROVIDER_ID => {}
        LEGACY_OLLAMA_CHAT_PROVIDER_ID => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                OLLAMA_CHAT_PROVIDER_REMOVED_ERROR,
            ));
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Invalid OSS provider '{provider}'. Must be one of: {LMSTUDIO_OSS_PROVIDER_ID}, {OLLAMA_OSS_PROVIDER_ID}"
                ),
            ));
        }
    }

    let edits = [ConfigEdit::SetPath {
        segments: vec!["oss_provider".to_string()],
        value: toml_edit::value(provider),
    }];

    ConfigEditsBuilder::new(codex_home)
        .with_edits(edits)
        .apply_blocking()
        .map_err(|err| std::io::Error::other(format!("failed to persist config.toml: {err}")))
}

/// Resolves the OSS provider from CLI override, profile config, or global config.
/// Returns `None` if no provider is configured at any level.
pub fn resolve_oss_provider(
    explicit_provider: Option<&str>,
    config_toml: &ConfigToml,
    config_profile: Option<String>,
) -> Option<String> {
    if let Some(provider) = explicit_provider {
        return Some(provider.to_string());
    }

    let profile = config_toml.get_config_profile(config_profile).ok();
    if let Some(profile) = &profile
        && let Some(profile_oss_provider) = &profile.oss_provider
    {
        return Some(profile_oss_provider.clone());
    }

    config_toml.oss_provider.clone()
}

#[cfg(test)]
mod tests {
    use super::resolve_oss_provider;
    use super::set_default_oss_provider;
    use crate::config::ConfigToml;
    use crate::config::profile::ConfigProfile;
    use crate::model_provider_info::LEGACY_OLLAMA_CHAT_PROVIDER_ID;
    use crate::model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
    use crate::model_provider_info::OLLAMA_CHAT_PROVIDER_REMOVED_ERROR;
    use crate::model_provider_info::OLLAMA_OSS_PROVIDER_ID;
    use codex_config::CONFIG_TOML_FILE;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    #[test]
    fn set_default_oss_provider_persists_known_provider() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let codex_home = temp_dir.path();
        let config_path = codex_home.join(CONFIG_TOML_FILE);

        set_default_oss_provider(codex_home, OLLAMA_OSS_PROVIDER_ID)?;
        let content = std::fs::read_to_string(&config_path)?;
        assert!(content.contains("oss_provider = \"ollama\""));

        std::fs::write(&config_path, "model = \"gpt-4\"\n")?;
        set_default_oss_provider(codex_home, LMSTUDIO_OSS_PROVIDER_ID)?;
        let content = std::fs::read_to_string(&config_path)?;
        assert!(content.contains("oss_provider = \"lmstudio\""));
        assert!(content.contains("model = \"gpt-4\""));

        set_default_oss_provider(codex_home, OLLAMA_OSS_PROVIDER_ID)?;
        let content = std::fs::read_to_string(&config_path)?;
        assert!(content.contains("oss_provider = \"ollama\""));
        assert!(!content.contains("oss_provider = \"lmstudio\""));

        Ok(())
    }

    #[test]
    fn set_default_oss_provider_rejects_invalid_provider() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let codex_home = temp_dir.path();

        let result = set_default_oss_provider(codex_home, "invalid_provider");
        assert!(result.is_err());
        let error = result.expect_err("invalid provider should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("Invalid OSS provider"));
        assert!(error.to_string().contains("invalid_provider"));

        Ok(())
    }

    #[test]
    fn set_default_oss_provider_rejects_legacy_ollama_chat_provider() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let codex_home = temp_dir.path();

        let result = set_default_oss_provider(codex_home, LEGACY_OLLAMA_CHAT_PROVIDER_ID);
        assert!(result.is_err());
        let error = result.expect_err("legacy ollama-chat provider should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains(OLLAMA_CHAT_PROVIDER_REMOVED_ERROR)
        );

        Ok(())
    }

    #[test]
    fn resolve_oss_provider_prefers_explicit_override() {
        let config_toml = ConfigToml::default();
        let result = resolve_oss_provider(Some("custom-provider"), &config_toml, None);
        assert_eq!(result, Some("custom-provider".to_string()));
    }

    #[test]
    fn resolve_oss_provider_uses_profile_value() {
        let config_toml = ConfigToml {
            profiles: std::collections::HashMap::from([(
                "test-profile".to_string(),
                ConfigProfile {
                    oss_provider: Some("profile-provider".to_string()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        let result = resolve_oss_provider(None, &config_toml, Some("test-profile".to_string()));
        assert_eq!(result, Some("profile-provider".to_string()));
    }

    #[test]
    fn resolve_oss_provider_falls_back_to_global_config() {
        let config_toml = ConfigToml {
            oss_provider: Some("global-provider".to_string()),
            ..Default::default()
        };

        let result = resolve_oss_provider(None, &config_toml, None);
        assert_eq!(result, Some("global-provider".to_string()));
    }

    #[test]
    fn resolve_oss_provider_profile_falls_back_to_global_config() {
        let config_toml = ConfigToml {
            oss_provider: Some("global-provider".to_string()),
            profiles: std::collections::HashMap::from([(
                "test-profile".to_string(),
                ConfigProfile::default(),
            )]),
            ..Default::default()
        };

        let result = resolve_oss_provider(None, &config_toml, Some("test-profile".to_string()));
        assert_eq!(result, Some("global-provider".to_string()));
    }

    #[test]
    fn resolve_oss_provider_returns_none_when_unconfigured() {
        let config_toml = ConfigToml::default();
        let result = resolve_oss_provider(None, &config_toml, None);
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_oss_provider_explicit_override_beats_profile_and_global() {
        let config_toml = ConfigToml {
            oss_provider: Some("global-provider".to_string()),
            profiles: std::collections::HashMap::from([(
                "test-profile".to_string(),
                ConfigProfile {
                    oss_provider: Some("profile-provider".to_string()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        let result = resolve_oss_provider(
            Some("explicit-provider"),
            &config_toml,
            Some("test-profile".to_string()),
        );
        assert_eq!(result, Some("explicit-provider".to_string()));
    }
}
