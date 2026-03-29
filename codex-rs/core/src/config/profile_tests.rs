use super::*;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::config::ConfigToml;
use crate::protocol::SandboxPolicy;
use codex_protocol::config_types::SandboxMode;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn profile_sandbox_mode_overrides_base() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut profiles = HashMap::new();
    profiles.insert(
        "work".to_string(),
        ConfigProfile {
            sandbox_mode: Some(SandboxMode::DangerFullAccess),
            ..Default::default()
        },
    );
    let cfg = ConfigToml {
        profiles,
        profile: Some("work".to_string()),
        sandbox_mode: Some(SandboxMode::ReadOnly),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert!(matches!(
        config.permissions.sandbox_policy.get(),
        &SandboxPolicy::DangerFullAccess
    ));

    Ok(())
}

#[test]
fn cli_override_takes_precedence_over_profile_sandbox_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut profiles = HashMap::new();
    profiles.insert(
        "work".to_string(),
        ConfigProfile {
            sandbox_mode: Some(SandboxMode::DangerFullAccess),
            ..Default::default()
        },
    );
    let cfg = ConfigToml {
        profiles,
        profile: Some("work".to_string()),
        ..Default::default()
    };

    let overrides = ConfigOverrides {
        sandbox_mode: Some(SandboxMode::WorkspaceWrite),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        overrides,
        codex_home.path().to_path_buf(),
    )?;

    if cfg!(target_os = "windows") {
        assert!(matches!(
            config.permissions.sandbox_policy.get(),
            SandboxPolicy::ReadOnly { .. }
        ));
    } else {
        assert!(matches!(
            config.permissions.sandbox_policy.get(),
            SandboxPolicy::WorkspaceWrite { .. }
        ));
    }

    Ok(())
}
