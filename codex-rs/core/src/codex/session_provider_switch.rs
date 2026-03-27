use super::SessionConfiguration;
use super::SessionSettingsUpdate;
use super::format_provider_switch_label;
use crate::model_compat::is_openai_model_slug;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::WireApi;
use crate::provider_routing::provider_id_for_model_slug as provider_id_for_model_family;
use crate::provider_routing::provider_matches_builtin_family;
use crate::provider_routing::resolve_provider_id_for_provider;

pub(super) fn apply_runtime_provider_switch(
    next_configuration: &mut SessionConfiguration,
    updates: &SessionSettingsUpdate,
) -> Option<String> {
    if updates.collaboration_mode.is_none() {
        return None;
    }

    // Auto-switch provider when the model family changes between
    // known provider families and default OpenAI-compatible models.
    // This ensures that `/model` switches at runtime route requests
    // to the correct API endpoint.
    let new_model = next_configuration.collaboration_mode.model();
    let target_provider_id = provider_id_for_model_family(new_model);
    let original_config = &next_configuration.original_config_do_not_use;
    // Treat provider identity changes as auto-switches, but preserve runtime
    // endpoint/account overrides when the configured provider id is unchanged.
    let provider_is_auto_switched =
        next_configuration.provider_id != original_config.model_provider_id;

    if let Some(target_provider_id) = target_provider_id {
        if provider_matches_builtin_family(&next_configuration.provider, target_provider_id) {
            return None;
        }

        // Use the merged provider map (built-in + user-defined from config.toml)
        // so that custom providers with account_pool, env_keys, etc. are preserved.
        let providers = &next_configuration
            .original_config_do_not_use
            .model_providers;
        let old_provider_id = next_configuration.provider_id.clone();
        if let Some(provider) = providers.get(target_provider_id) {
            next_configuration.provider_id = target_provider_id.to_string();
            next_configuration.provider = provider.clone();
            return Some(format_provider_switch_label(
                &old_provider_id,
                target_provider_id,
                new_model,
            ));
        }

        tracing::warn!(
            target_provider_id,
            available_providers = ?providers.keys().collect::<Vec<_>>(),
            "auto-switch: target provider not found in merged provider map"
        );
        return None;
    }

    if is_openai_model_slug(new_model) && next_configuration.provider.wire_api != WireApi::Responses
    {
        let providers = &next_configuration
            .original_config_do_not_use
            .model_providers;
        let old_provider_id = next_configuration.provider_id.clone();
        let restored_provider =
            if original_config.user_configured_provider.wire_api == WireApi::Responses {
                original_config.user_configured_provider.clone()
            } else if let Some(openai) = providers.get(OPENAI_PROVIDER_ID) {
                openai.clone()
            } else {
                original_config.user_configured_provider.clone()
            };
        next_configuration.provider_id = resolve_provider_id_for_provider(
            providers,
            &restored_provider,
            &original_config.model_provider_id,
        );
        next_configuration.provider = restored_provider;
        return Some(format_provider_switch_label(
            &old_provider_id,
            next_configuration.provider_id.as_str(),
            new_model,
        ));
    }

    if provider_is_auto_switched {
        // Switching FROM a family-specific provider back to a default
        // model family: restore the user's explicitly configured provider
        // (before auto-switching).
        let old_provider_id = next_configuration.provider_id.clone();
        let restored_provider = original_config.user_configured_provider.clone();
        next_configuration.provider_id = resolve_provider_id_for_provider(
            &original_config.model_providers,
            &restored_provider,
            &original_config.model_provider_id,
        );
        next_configuration.provider = restored_provider;
        return Some(format_provider_switch_label(
            &old_provider_id,
            next_configuration.provider_id.as_str(),
            new_model,
        ));
    }

    None
}
