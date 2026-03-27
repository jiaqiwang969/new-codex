use crate::model_compat::is_openai_model_slug;
use crate::model_provider_info::LEGACY_OLLAMA_CHAT_PROVIDER_ID;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::OLLAMA_CHAT_PROVIDER_REMOVED_ERROR;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::WireApi;
use crate::provider_routing::provider_id_for_model_slug as provider_id_for_model_family;
use crate::provider_routing::provider_matches_builtin_family;
use std::collections::HashMap;

pub(crate) struct SelectedProvider {
    pub(crate) model_provider_id: String,
    pub(crate) model_provider: ModelProviderInfo,
    pub(crate) user_configured_provider: ModelProviderInfo,
}

pub(crate) fn select_model_provider(
    model_provider_id: String,
    model: Option<&str>,
    model_providers: &HashMap<String, ModelProviderInfo>,
) -> std::io::Result<SelectedProvider> {
    let mut model_provider_id = model_provider_id;
    let mut model_provider = model_providers
        .get(&model_provider_id)
        .ok_or_else(|| {
            let message = if model_provider_id == LEGACY_OLLAMA_CHAT_PROVIDER_ID {
                OLLAMA_CHAT_PROVIDER_REMOVED_ERROR.to_string()
            } else {
                format!("Model provider `{model_provider_id}` not found")
            };
            std::io::Error::new(std::io::ErrorKind::NotFound, message)
        })?
        .clone();

    let user_configured_provider = model_provider.clone();

    if let Some(model) = model
        && let Some(target_provider_id) = provider_id_for_model_family(model)
        && !provider_matches_builtin_family(&model_provider, target_provider_id)
        && let Some(target_provider) = model_providers.get(target_provider_id)
    {
        model_provider_id = target_provider_id.to_string();
        model_provider = target_provider.clone();
    }

    if let Some(model) = model
        && is_openai_model_slug(model)
        && model_provider.wire_api != WireApi::Responses
        && let Some(target_provider) = model_providers.get(OPENAI_PROVIDER_ID)
    {
        model_provider_id = OPENAI_PROVIDER_ID.to_string();
        model_provider = target_provider.clone();
    }

    Ok(SelectedProvider {
        model_provider_id,
        model_provider,
        user_configured_provider,
    })
}

#[cfg(test)]
mod tests {
    use super::select_model_provider;
    use crate::config::Config;
    use crate::config::ConfigOverrides;
    use crate::config::ConfigToml;
    use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
    use crate::model_provider_info::GEMINI_PROVIDER_ID;
    use crate::model_provider_info::GEMMA_PROVIDER_ID;
    use crate::model_provider_info::GROK_PROVIDER_ID;
    use crate::model_provider_info::LEGACY_OLLAMA_CHAT_PROVIDER_ID;
    use crate::model_provider_info::ModelProviderInfo;
    use crate::model_provider_info::OLLAMA_CHAT_PROVIDER_REMOVED_ERROR;
    use crate::model_provider_info::OPENAI_PROVIDER_ID;
    use crate::model_provider_info::WireApi;
    use crate::model_provider_info::built_in_model_providers;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn select_model_provider_switches_to_builtin_family_provider() -> std::io::Result<()> {
        let providers = built_in_model_providers(None);

        let selection = select_model_provider(
            OPENAI_PROVIDER_ID.to_string(),
            Some("claude-opus-4-6"),
            &providers,
        )?;

        assert_eq!(selection.model_provider_id, ANTHROPIC_PROVIDER_ID);
        assert_eq!(selection.model_provider.name, "Anthropic");
        assert_eq!(selection.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn select_model_provider_preserves_custom_family_provider() -> std::io::Result<()> {
        let custom_provider_id = "anthropic-proxy".to_string();
        let custom_provider = ModelProviderInfo {
            name: "Anthropic Proxy".to_string(),
            base_url: Some("https://example.com/anthropic".to_string()),
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
            wire_api: WireApi::Anthropic,
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
            account_pool: Vec::new(),
        };
        let mut providers = built_in_model_providers(None);
        providers.insert(custom_provider_id.clone(), custom_provider.clone());

        let selection = select_model_provider(
            custom_provider_id.clone(),
            Some("claude-sonnet-4-6"),
            &providers,
        )?;

        assert_eq!(selection.model_provider_id, custom_provider_id);
        assert_eq!(selection.model_provider, custom_provider);
        assert_eq!(selection.user_configured_provider, custom_provider);

        Ok(())
    }

    #[test]
    fn select_model_provider_switches_openai_slug_back_to_openai() -> std::io::Result<()> {
        let providers = built_in_model_providers(None);

        let selection = select_model_provider(
            ANTHROPIC_PROVIDER_ID.to_string(),
            Some("gpt-5.3-codex"),
            &providers,
        )?;

        assert_eq!(selection.model_provider_id, OPENAI_PROVIDER_ID);
        assert_eq!(selection.model_provider.name, "OpenAI");
        assert_eq!(selection.user_configured_provider.name, "Anthropic");

        Ok(())
    }

    #[test]
    fn grok_model_auto_switches_to_grok_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("grok-4-latest".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GROK_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Grok");
        assert_eq!(
            config.model_provider.env_key.as_deref(),
            Some("XAI_API_KEY")
        );
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn gemini_model_auto_switches_to_gemini_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("gemini-2.5-pro".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GEMINI_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Gemini");
        assert_eq!(
            config.model_provider.env_key.as_deref(),
            Some("GEMINI_API_KEY")
        );
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn claude_model_auto_switches_to_anthropic_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("claude-opus-4-6".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, ANTHROPIC_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Anthropic");
        assert_eq!(
            config.model_provider.env_key.as_deref(),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn gemma_model_auto_switches_to_gemma_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("gemma-3n".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GEMMA_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Gemma");
        assert_eq!(config.model_provider.env_key, None);
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn gemma_model_overrides_builtin_gemini_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("gemma-3n".to_string()),
            model_provider: Some(GEMINI_PROVIDER_ID.to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GEMMA_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Gemma");

        Ok(())
    }

    #[test]
    fn gemma_model_does_not_override_custom_gemini_providers() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let custom_provider_id = "gemini-proxy".to_string();
        let custom_gemini_provider = ModelProviderInfo {
            name: "Gemini Proxy".to_string(),
            base_url: Some("http://localhost:5001/v1beta".to_string()),
            env_key: None,
            wire_api: WireApi::Gemini,
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
            account_pool: Vec::new(),
        };

        let mut model_providers = HashMap::new();
        model_providers.insert(custom_provider_id.clone(), custom_gemini_provider.clone());

        let cfg = ConfigToml {
            model: Some("gemma-3n".to_string()),
            model_provider: Some(custom_provider_id.clone()),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, custom_provider_id);
        assert_eq!(config.model_provider, custom_gemini_provider);
        assert_eq!(config.user_configured_provider, custom_gemini_provider);

        Ok(())
    }

    #[test]
    fn gemini_model_does_not_override_custom_gemini_providers() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let custom_provider_id = "gemini-proxy".to_string();
        let custom_gemini_provider = ModelProviderInfo {
            name: "Gemini Proxy".to_string(),
            base_url: Some("https://example.com/gemini".to_string()),
            env_key: Some("GEMINI_API_KEY".to_string()),
            wire_api: WireApi::Gemini,
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
            account_pool: Vec::new(),
        };

        let mut model_providers = HashMap::new();
        model_providers.insert(custom_provider_id.clone(), custom_gemini_provider.clone());

        let cfg = ConfigToml {
            model: Some("gemini-2.5-pro".to_string()),
            model_provider: Some(custom_provider_id.clone()),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, custom_provider_id);
        assert_eq!(config.model_provider, custom_gemini_provider);
        assert_eq!(config.user_configured_provider, custom_gemini_provider);

        Ok(())
    }

    #[test]
    fn grok_model_does_not_override_custom_grok_providers() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let custom_provider_id = "grok-proxy".to_string();
        let custom_grok_provider = ModelProviderInfo {
            name: "Grok".to_string(),
            base_url: Some("https://example.com/grok".to_string()),
            env_key: Some("XAI_API_KEY".to_string()),
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
            account_pool: Vec::new(),
        };

        let mut model_providers = HashMap::new();
        model_providers.insert(custom_provider_id.clone(), custom_grok_provider.clone());

        let cfg = ConfigToml {
            model: Some("grok-4-latest".to_string()),
            model_provider: Some(custom_provider_id.clone()),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, custom_provider_id);
        assert_eq!(config.model_provider, custom_grok_provider);
        assert_eq!(config.user_configured_provider, custom_grok_provider);

        Ok(())
    }

    #[test]
    fn claude_model_does_not_override_custom_anthropic_providers() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;

        let custom_provider_id = "anthropic-proxy".to_string();
        let custom_anthropic_provider = ModelProviderInfo {
            name: "Anthropic Proxy".to_string(),
            base_url: Some("https://example.com/anthropic".to_string()),
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
            wire_api: WireApi::Anthropic,
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
            account_pool: Vec::new(),
        };

        let mut model_providers = HashMap::new();
        model_providers.insert(
            custom_provider_id.clone(),
            custom_anthropic_provider.clone(),
        );

        let cfg = ConfigToml {
            model: Some("claude-sonnet-4-6".to_string()),
            model_provider: Some(custom_provider_id.clone()),
            model_providers,
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, custom_provider_id);
        assert_eq!(config.model_provider, custom_anthropic_provider);
        assert_eq!(config.user_configured_provider, custom_anthropic_provider);

        Ok(())
    }

    #[test]
    fn gpt_model_auto_switches_to_openai_provider_when_current_provider_is_non_responses()
    -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("gpt-5.3-codex".to_string()),
            model_provider: Some("anthropic".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, "openai");
        assert_eq!(config.model_provider.name, "OpenAI");
        assert_eq!(config.user_configured_provider.name, "Anthropic");

        Ok(())
    }

    #[test]
    fn namespaced_grok_model_auto_switches_to_grok_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("xai/grok-4-latest".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GROK_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Grok");
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn namespaced_gemma_model_auto_switches_to_gemma_provider() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model: Some("google/gemma-3n".to_string()),
            model_provider: Some("openai".to_string()),
            ..Default::default()
        };

        let config = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )?;

        assert_eq!(config.model_provider_id, GEMMA_PROVIDER_ID);
        assert_eq!(config.model_provider.name, "Gemma");
        assert_eq!(config.user_configured_provider.name, "OpenAI");

        Ok(())
    }

    #[test]
    fn load_config_rejects_legacy_ollama_chat_provider_with_helpful_error() -> std::io::Result<()> {
        let codex_home = TempDir::new()?;
        let cfg = ConfigToml {
            model_provider: Some(LEGACY_OLLAMA_CHAT_PROVIDER_ID.to_string()),
            ..Default::default()
        };

        let result = Config::load_from_base_config_with_overrides(
            cfg,
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        );
        assert!(result.is_err());
        let error = result.expect_err("legacy ollama-chat provider should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            error
                .to_string()
                .contains(OLLAMA_CHAT_PROVIDER_REMOVED_ERROR)
        );

        Ok(())
    }
}
