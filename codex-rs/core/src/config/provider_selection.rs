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
    use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
    use crate::model_provider_info::ModelProviderInfo;
    use crate::model_provider_info::OPENAI_PROVIDER_ID;
    use crate::model_provider_info::WireApi;
    use crate::model_provider_info::built_in_model_providers;
    use pretty_assertions::assert_eq;

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
}
