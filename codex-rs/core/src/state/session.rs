//! Session-wide mutable state.

use codex_protocol::models::PermissionProfile;
use codex_protocol::models::ResponseItem;
use codex_sandboxing::policy_transforms::merge_permission_profiles;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use crate::codex::PreviousTurnSettings;
use crate::codex::SessionConfiguration;
use crate::context_manager::ContextManager;
use crate::gemini_types::GeminiAspectRatio;
use crate::gemini_types::GeminiImageSize;
use crate::model_provider_info::ModelProviderAccount;
use crate::protocol::RateLimitSnapshot;
use crate::protocol::TokenUsage;
use crate::protocol::TokenUsageInfo;
use crate::session_startup_prewarm::SessionStartupPrewarmHandle;
use crate::truncate::TruncationPolicy;
use codex_protocol::protocol::TurnContextItem;

#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderPoolRuntimeState {
    cooldowns: HashMap<ModelProviderAccount, Instant>,
}

impl ProviderPoolRuntimeState {
    fn cooldown_until(&mut self, account: &ModelProviderAccount, now: Instant) -> Option<Instant> {
        match self.cooldowns.get(account).copied() {
            Some(until) if until > now => Some(until),
            Some(_) => {
                self.cooldowns.remove(account);
                None
            }
            None => None,
        }
    }

    fn mark_cooling(
        &mut self,
        account: ModelProviderAccount,
        now: Instant,
        cooldown: Duration,
    ) -> Instant {
        let until = now + cooldown;
        self.cooldowns.insert(account, until);
        until
    }
}

/// Persistent, session-scoped state previously stored directly on `Session`.
pub(crate) struct SessionState {
    pub(crate) session_configuration: SessionConfiguration,
    pub(crate) history: ContextManager,
    pub(crate) latest_rate_limits: Option<RateLimitSnapshot>,
    pub(crate) server_reasoning_included: bool,
    pub(crate) dependency_env: HashMap<String, String>,
    pub(crate) mcp_dependency_prompted: HashSet<String>,
    /// Settings used by the latest regular user turn, used for turn-to-turn
    /// model/realtime handling on subsequent regular turns (including full-context
    /// reinjection after resume or `/compact`).
    previous_turn_settings: Option<PreviousTurnSettings>,
    /// Startup prewarmed session prepared during session initialization.
    pub(crate) startup_prewarm: Option<SessionStartupPrewarmHandle>,
    pub(crate) active_mcp_tool_selection: Option<Vec<String>>,
    auto_model_sub_selection: Option<String>,
    auto_model_sub_calibration_attempted: bool,
    last_model_sub_calibration_models: Vec<String>,
    last_model_sub_calibration_recommended_for_session: Option<String>,
    provider_pool_runtime: HashMap<String, ProviderPoolRuntimeState>,
    active_reference_images: Vec<String>,
    image_size: Option<GeminiImageSize>,
    aspect_ratio: Option<GeminiAspectRatio>,
    pub(crate) active_connector_selection: HashSet<String>,
    pub(crate) pending_session_start_source: Option<codex_hooks::SessionStartSource>,
    granted_permissions: Option<PermissionProfile>,
}

impl SessionState {
    /// Create a new session state mirroring previous `State::default()` semantics.
    pub(crate) fn new(session_configuration: SessionConfiguration) -> Self {
        let history = ContextManager::new();
        Self {
            session_configuration,
            history,
            latest_rate_limits: None,
            server_reasoning_included: false,
            dependency_env: HashMap::new(),
            mcp_dependency_prompted: HashSet::new(),
            previous_turn_settings: None,
            startup_prewarm: None,
            active_mcp_tool_selection: None,
            auto_model_sub_selection: None,
            auto_model_sub_calibration_attempted: false,
            last_model_sub_calibration_models: Vec::new(),
            last_model_sub_calibration_recommended_for_session: None,
            provider_pool_runtime: HashMap::new(),
            active_reference_images: Vec::new(),
            image_size: None,
            aspect_ratio: None,
            active_connector_selection: HashSet::new(),
            pending_session_start_source: None,
            granted_permissions: None,
        }
    }

    // History helpers
    pub(crate) fn record_items<I>(&mut self, items: I, policy: TruncationPolicy)
    where
        I: IntoIterator,
        I::Item: std::ops::Deref<Target = ResponseItem>,
    {
        self.history.record_items(items, policy);
    }

    pub(crate) fn previous_turn_settings(&self) -> Option<PreviousTurnSettings> {
        self.previous_turn_settings.clone()
    }
    pub(crate) fn set_previous_turn_settings(
        &mut self,
        previous_turn_settings: Option<PreviousTurnSettings>,
    ) {
        self.previous_turn_settings = previous_turn_settings;
    }

    pub(crate) fn clone_history(&self) -> ContextManager {
        self.history.clone()
    }

    pub(crate) fn replace_history(
        &mut self,
        items: Vec<ResponseItem>,
        reference_context_item: Option<TurnContextItem>,
    ) {
        self.history.replace(items);
        self.history
            .set_reference_context_item(reference_context_item);
    }

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        self.history.set_token_info(info);
    }

    pub(crate) fn set_reference_context_item(&mut self, item: Option<TurnContextItem>) {
        self.history.set_reference_context_item(item);
    }

    pub(crate) fn reference_context_item(&self) -> Option<TurnContextItem> {
        self.history.reference_context_item()
    }

    // Token/rate limit helpers
    pub(crate) fn update_token_info_from_usage(
        &mut self,
        usage: &TokenUsage,
        model_context_window: Option<i64>,
    ) {
        self.history.update_token_info(usage, model_context_window);
    }

    pub(crate) fn token_info(&self) -> Option<TokenUsageInfo> {
        self.history.token_info()
    }

    pub(crate) fn set_rate_limits(&mut self, snapshot: RateLimitSnapshot) {
        self.latest_rate_limits = Some(merge_rate_limit_fields(
            self.latest_rate_limits.as_ref(),
            snapshot,
        ));
    }

    pub(crate) fn token_info_and_rate_limits(
        &self,
    ) -> (Option<TokenUsageInfo>, Option<RateLimitSnapshot>) {
        (self.token_info(), self.latest_rate_limits.clone())
    }

    pub(crate) fn set_token_usage_full(&mut self, context_window: i64) {
        self.history.set_token_usage_full(context_window);
    }

    pub(crate) fn get_total_token_usage(&self, server_reasoning_included: bool) -> i64 {
        self.history
            .get_total_token_usage(server_reasoning_included)
    }

    pub(crate) fn set_server_reasoning_included(&mut self, included: bool) {
        self.server_reasoning_included = included;
    }

    pub(crate) fn server_reasoning_included(&self) -> bool {
        self.server_reasoning_included
    }

    pub(crate) fn record_mcp_dependency_prompted<I>(&mut self, names: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.mcp_dependency_prompted.extend(names);
    }

    pub(crate) fn mcp_dependency_prompted(&self) -> HashSet<String> {
        self.mcp_dependency_prompted.clone()
    }

    pub(crate) fn pool_cooldown_until(
        &mut self,
        provider_id: &str,
        account: &ModelProviderAccount,
        now: Instant,
    ) -> Option<Instant> {
        self.provider_pool_runtime
            .entry(provider_id.to_string())
            .or_default()
            .cooldown_until(account, now)
    }

    pub(crate) fn mark_pool_account_cooling(
        &mut self,
        provider_id: &str,
        account: ModelProviderAccount,
        now: Instant,
        cooldown: Duration,
    ) -> Instant {
        self.provider_pool_runtime
            .entry(provider_id.to_string())
            .or_default()
            .mark_cooling(account, now, cooldown)
    }

    pub(crate) fn set_dependency_env(&mut self, values: HashMap<String, String>) {
        for (key, value) in values {
            self.dependency_env.insert(key, value);
        }
    }

    pub(crate) fn dependency_env(&self) -> HashMap<String, String> {
        self.dependency_env.clone()
    }

    pub(crate) fn set_session_startup_prewarm(
        &mut self,
        startup_prewarm: SessionStartupPrewarmHandle,
    ) {
        self.startup_prewarm = Some(startup_prewarm);
    }

    pub(crate) fn take_session_startup_prewarm(&mut self) -> Option<SessionStartupPrewarmHandle> {
        self.startup_prewarm.take()
    }

    pub(crate) fn set_reference_images(&mut self, images: Vec<String>) {
        self.active_reference_images = images;
    }

    pub(crate) fn clear_reference_images(&mut self) {
        self.active_reference_images.clear();
    }

    pub(crate) fn reference_images(&self) -> &[String] {
        &self.active_reference_images
    }

    pub(crate) fn set_image_size(&mut self, size: Option<GeminiImageSize>) {
        self.image_size = size;
    }

    pub(crate) fn image_size(&self) -> Option<GeminiImageSize> {
        self.image_size
    }

    pub(crate) fn set_aspect_ratio(&mut self, ratio: Option<GeminiAspectRatio>) {
        self.aspect_ratio = ratio;
    }

    pub(crate) fn aspect_ratio(&self) -> Option<GeminiAspectRatio> {
        self.aspect_ratio
    }

    pub(crate) fn merge_mcp_tool_selection(&mut self, tool_names: Vec<String>) -> Vec<String> {
        if tool_names.is_empty() {
            return self.active_mcp_tool_selection.clone().unwrap_or_default();
        }

        let mut merged = self.active_mcp_tool_selection.take().unwrap_or_default();
        let mut seen: HashSet<String> = merged.iter().cloned().collect();
        for tool_name in tool_names {
            if seen.insert(tool_name.clone()) {
                merged.push(tool_name);
            }
        }

        self.active_mcp_tool_selection = Some(merged.clone());
        merged
    }

    pub(crate) fn set_mcp_tool_selection(&mut self, tool_names: Vec<String>) {
        if tool_names.is_empty() {
            self.active_mcp_tool_selection = None;
            return;
        }

        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        for tool_name in tool_names {
            if seen.insert(tool_name.clone()) {
                selected.push(tool_name);
            }
        }

        self.active_mcp_tool_selection = if selected.is_empty() {
            None
        } else {
            Some(selected)
        };
    }

    pub(crate) fn get_mcp_tool_selection(&self) -> Option<Vec<String>> {
        self.active_mcp_tool_selection.clone()
    }

    pub(crate) fn clear_mcp_tool_selection(&mut self) {
        self.active_mcp_tool_selection = None;
    }

    pub(crate) fn set_auto_model_sub_selection(&mut self, model_sub: Option<String>) {
        self.auto_model_sub_selection = model_sub;
        if self.auto_model_sub_selection.is_none() {
            self.auto_model_sub_calibration_attempted = false;
        }
    }

    pub(crate) fn get_auto_model_sub_selection(&self) -> Option<String> {
        self.auto_model_sub_selection.clone()
    }

    pub(crate) fn set_auto_model_sub_calibration_attempted(&mut self, attempted: bool) {
        self.auto_model_sub_calibration_attempted = attempted;
    }

    pub(crate) fn get_auto_model_sub_calibration_attempted(&self) -> bool {
        self.auto_model_sub_calibration_attempted
    }

    pub(crate) fn set_last_model_sub_calibration_models(&mut self, models: Vec<String>) {
        self.last_model_sub_calibration_models = models;
    }

    pub(crate) fn get_last_model_sub_calibration_models(&self) -> Vec<String> {
        self.last_model_sub_calibration_models.clone()
    }

    pub(crate) fn set_last_model_sub_calibration_recommended_for_session(
        &mut self,
        model: Option<String>,
    ) {
        self.last_model_sub_calibration_recommended_for_session = model;
    }

    pub(crate) fn get_last_model_sub_calibration_recommended_for_session(&self) -> Option<String> {
        self.last_model_sub_calibration_recommended_for_session
            .clone()
    }

    // Adds connector IDs to the active set and returns the merged selection.
    pub(crate) fn merge_connector_selection<I>(&mut self, connector_ids: I) -> HashSet<String>
    where
        I: IntoIterator<Item = String>,
    {
        self.active_connector_selection.extend(connector_ids);
        self.active_connector_selection.clone()
    }

    // Returns the current connector selection tracked on session state.
    pub(crate) fn get_connector_selection(&self) -> HashSet<String> {
        self.active_connector_selection.clone()
    }

    // Removes all currently tracked connector selections.
    pub(crate) fn clear_connector_selection(&mut self) {
        self.active_connector_selection.clear();
    }

    pub(crate) fn set_pending_session_start_source(
        &mut self,
        value: Option<codex_hooks::SessionStartSource>,
    ) {
        self.pending_session_start_source = value;
    }

    pub(crate) fn take_pending_session_start_source(
        &mut self,
    ) -> Option<codex_hooks::SessionStartSource> {
        self.pending_session_start_source.take()
    }

    pub(crate) fn record_granted_permissions(&mut self, permissions: PermissionProfile) {
        self.granted_permissions =
            merge_permission_profiles(self.granted_permissions.as_ref(), Some(&permissions));
    }

    pub(crate) fn granted_permissions(&self) -> Option<PermissionProfile> {
        self.granted_permissions.clone()
    }
}

// Sometimes new snapshots don't include credits or plan information.
// Preserve those from the previous snapshot when missing. For `limit_id`, treat
// missing values as the default `"codex"` bucket.
fn merge_rate_limit_fields(
    previous: Option<&RateLimitSnapshot>,
    mut snapshot: RateLimitSnapshot,
) -> RateLimitSnapshot {
    if snapshot.limit_id.is_none() {
        snapshot.limit_id = Some("codex".to_string());
    }
    if snapshot.credits.is_none() {
        snapshot.credits = previous.and_then(|prior| prior.credits.clone());
    }
    if snapshot.plan_type.is_none() {
        snapshot.plan_type = previous.and_then(|prior| prior.plan_type);
    }
    snapshot
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
