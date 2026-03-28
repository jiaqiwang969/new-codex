use crate::config::ConfigToml;
use crate::config::Constrained;
use crate::config::profile::ConfigProfile;
use codex_features::Feature;
use codex_features::Features;
use codex_protocol::config_types::WebSearchConfig;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::protocol::SandboxPolicy;

/// Resolve the web search mode from explicit config and feature flags.
pub(super) fn resolve_web_search_mode(
    config_toml: &ConfigToml,
    config_profile: &ConfigProfile,
    features: &Features,
) -> Option<WebSearchMode> {
    if let Some(mode) = config_profile.web_search.or(config_toml.web_search) {
        return Some(mode);
    }
    if features.enabled(Feature::WebSearchCached) {
        return Some(WebSearchMode::Cached);
    }
    if features.enabled(Feature::WebSearchRequest) {
        return Some(WebSearchMode::Live);
    }
    None
}

pub(super) fn resolve_web_search_config(
    config_toml: &ConfigToml,
    config_profile: &ConfigProfile,
) -> Option<WebSearchConfig> {
    let base = config_toml
        .tools
        .as_ref()
        .and_then(|tools| tools.web_search.as_ref());
    let profile = config_profile
        .tools
        .as_ref()
        .and_then(|tools| tools.web_search.as_ref());

    match (base, profile) {
        (None, None) => None,
        (Some(base), None) => Some(base.clone().into()),
        (None, Some(profile)) => Some(profile.clone().into()),
        (Some(base), Some(profile)) => Some(base.merge(profile).into()),
    }
}

pub(crate) fn resolve_web_search_mode_for_turn(
    web_search_mode: &Constrained<WebSearchMode>,
    sandbox_policy: &SandboxPolicy,
) -> WebSearchMode {
    let preferred = web_search_mode.value();

    if matches!(sandbox_policy, SandboxPolicy::DangerFullAccess)
        && preferred != WebSearchMode::Disabled
    {
        for mode in [
            WebSearchMode::Live,
            WebSearchMode::Cached,
            WebSearchMode::Disabled,
        ] {
            if web_search_mode.can_set(&mode).is_ok() {
                return mode;
            }
        }
    } else {
        if web_search_mode.can_set(&preferred).is_ok() {
            return preferred;
        }
        for mode in [
            WebSearchMode::Cached,
            WebSearchMode::Live,
            WebSearchMode::Disabled,
        ] {
            if web_search_mode.can_set(&mode).is_ok() {
                return mode;
            }
        }
    }

    WebSearchMode::Disabled
}

#[cfg(test)]
mod tests {
    use super::resolve_web_search_mode;
    use super::resolve_web_search_mode_for_turn;
    use crate::config::ConfigToml;
    use crate::config::Constrained;
    use crate::config::ConstraintError;
    use crate::config::profile::ConfigProfile;
    use crate::config_loader::RequirementSource;
    use codex_features::Feature;
    use codex_features::Features;
    use codex_protocol::config_types::WebSearchMode;
    use codex_protocol::protocol::SandboxPolicy;
    use pretty_assertions::assert_eq;

    #[test]
    fn web_search_mode_defaults_to_none_if_unset() {
        let cfg = ConfigToml::default();
        let profile = ConfigProfile::default();
        let features = Features::with_defaults();

        assert_eq!(resolve_web_search_mode(&cfg, &profile, &features), None);
    }

    #[test]
    fn web_search_mode_prefers_profile_over_legacy_flags() {
        let cfg = ConfigToml::default();
        let profile = ConfigProfile {
            web_search: Some(WebSearchMode::Live),
            ..Default::default()
        };
        let mut features = Features::with_defaults();
        features.enable(Feature::WebSearchCached);

        assert_eq!(
            resolve_web_search_mode(&cfg, &profile, &features),
            Some(WebSearchMode::Live)
        );
    }

    #[test]
    fn web_search_mode_disabled_overrides_legacy_request() {
        let cfg = ConfigToml {
            web_search: Some(WebSearchMode::Disabled),
            ..Default::default()
        };
        let profile = ConfigProfile::default();
        let mut features = Features::with_defaults();
        features.enable(Feature::WebSearchRequest);

        assert_eq!(
            resolve_web_search_mode(&cfg, &profile, &features),
            Some(WebSearchMode::Disabled)
        );
    }

    #[test]
    fn web_search_mode_for_turn_uses_preference_for_read_only() {
        let web_search_mode = Constrained::allow_any(WebSearchMode::Cached);
        let mode = resolve_web_search_mode_for_turn(
            &web_search_mode,
            &SandboxPolicy::new_read_only_policy(),
        );

        assert_eq!(mode, WebSearchMode::Cached);
    }

    #[test]
    fn web_search_mode_for_turn_prefers_live_for_danger_full_access() {
        let web_search_mode = Constrained::allow_any(WebSearchMode::Cached);
        let mode =
            resolve_web_search_mode_for_turn(&web_search_mode, &SandboxPolicy::DangerFullAccess);

        assert_eq!(mode, WebSearchMode::Live);
    }

    #[test]
    fn web_search_mode_for_turn_respects_disabled_for_danger_full_access() {
        let web_search_mode = Constrained::allow_any(WebSearchMode::Disabled);
        let mode =
            resolve_web_search_mode_for_turn(&web_search_mode, &SandboxPolicy::DangerFullAccess);

        assert_eq!(mode, WebSearchMode::Disabled);
    }

    #[test]
    fn web_search_mode_for_turn_falls_back_when_live_is_disallowed() -> anyhow::Result<()> {
        let allowed = [WebSearchMode::Disabled, WebSearchMode::Cached];
        let web_search_mode = Constrained::new(WebSearchMode::Cached, move |candidate| {
            if allowed.contains(candidate) {
                Ok(())
            } else {
                Err(ConstraintError::InvalidValue {
                    field_name: "web_search_mode",
                    candidate: format!("{candidate:?}"),
                    allowed: format!("{allowed:?}"),
                    requirement_source: RequirementSource::Unknown,
                })
            }
        })?;
        let mode =
            resolve_web_search_mode_for_turn(&web_search_mode, &SandboxPolicy::DangerFullAccess);

        assert_eq!(mode, WebSearchMode::Cached);
        Ok(())
    }
}
