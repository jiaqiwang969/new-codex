use super::*;
use crate::model_compat::is_openai_model_slug;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::WireApi;
use crate::provider_routing::provider_id_for_model_slug as provider_id_for_model_family;
use crate::provider_routing::provider_matches_builtin_family;
use crate::provider_routing::resolve_provider_id_for_provider;

impl SessionConfiguration {
    pub(crate) fn apply(
        &self,
        updates: &SessionSettingsUpdate,
    ) -> ConstraintResult<(Self, Option<String>)> {
        let mut next_configuration = self.clone();
        let file_system_policy_matches_legacy = self.file_system_sandbox_policy
            == FileSystemSandboxPolicy::from_legacy_sandbox_policy(
                self.sandbox_policy.get(),
                &self.cwd,
            );
        if let Some(collaboration_mode) = updates.collaboration_mode.clone() {
            next_configuration.collaboration_mode = collaboration_mode;
        }
        if let Some(summary) = updates.reasoning_summary {
            next_configuration.model_reasoning_summary = Some(summary);
        }
        if let Some(service_tier) = updates.service_tier {
            next_configuration.service_tier = service_tier;
        }
        if let Some(personality) = updates.personality {
            next_configuration.personality = Some(personality);
        }
        if let Some(approval_policy) = updates.approval_policy {
            next_configuration.approval_policy.set(approval_policy)?;
        }
        if let Some(approvals_reviewer) = updates.approvals_reviewer {
            next_configuration.approvals_reviewer = approvals_reviewer;
        }
        let mut sandbox_policy_changed = false;
        if let Some(sandbox_policy) = updates.sandbox_policy.clone() {
            next_configuration.sandbox_policy.set(sandbox_policy)?;
            next_configuration.network_sandbox_policy =
                NetworkSandboxPolicy::from(next_configuration.sandbox_policy.get());
            sandbox_policy_changed = true;
        }
        if let Some(windows_sandbox_level) = updates.windows_sandbox_level {
            next_configuration.windows_sandbox_level = windows_sandbox_level;
        }

        let absolute_cwd = updates
            .cwd
            .as_ref()
            .map(|cwd| {
                AbsolutePathBuf::relative_to_current_dir(normalize_for_native_workdir(
                    cwd.as_path(),
                ))
                .unwrap_or_else(|e| {
                    warn!("failed to normalize update cwd: {cwd:?}: {e}");
                    self.cwd.clone()
                })
            })
            .unwrap_or_else(|| self.cwd.clone());

        let cwd_changed = absolute_cwd.as_path() != self.cwd.as_path();
        next_configuration.cwd = absolute_cwd;
        if sandbox_policy_changed || (cwd_changed && file_system_policy_matches_legacy) {
            next_configuration.file_system_sandbox_policy =
                FileSystemSandboxPolicy::from_legacy_sandbox_policy(
                    next_configuration.sandbox_policy.get(),
                    &next_configuration.cwd,
                );
        }
        if let Some(app_server_client_name) = updates.app_server_client_name.clone() {
            next_configuration.app_server_client_name = Some(app_server_client_name);
        }
        if let (Some(provider_id), Some(provider)) = (
            updates.model_provider_id.clone(),
            updates.model_provider.clone(),
        ) {
            next_configuration.provider_id = provider_id;
            next_configuration.provider = provider;
            let mut updated_config = (*next_configuration.original_config_do_not_use).clone();
            updated_config.model_provider_id = next_configuration.provider_id.clone();
            updated_config.model_provider = next_configuration.provider.clone();
            next_configuration.original_config_do_not_use = Arc::new(updated_config);
        }

        let provider_switch_label = apply_runtime_provider_switch(&mut next_configuration, updates);

        Ok((next_configuration, provider_switch_label))
    }
}

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

pub(super) fn format_provider_switch_label(
    old_provider_id: &str,
    new_provider_id: &str,
    model: &str,
) -> String {
    format!("{old_provider_id} -> {new_provider_id} (model: {model})")
}

pub(super) fn drop_provider_specific_encrypted_history_items(state: &mut SessionState) -> usize {
    let snapshot = state.clone_history();
    let original = snapshot.raw_items();
    let filtered = original
        .iter()
        .filter(|item| {
            !matches!(
                item,
                ResponseItem::Reasoning {
                    encrypted_content: Some(_),
                    ..
                } | ResponseItem::Compaction { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed_count = original.len().saturating_sub(filtered.len());
    if removed_count > 0 {
        state.replace_history(filtered, /*reference_context_item*/ None);
    }
    removed_count
}
