use super::*;
use crate::config::types::AppToolApproval;
use crate::config::types::McpServerToolConfig;
use codex_config::CONFIG_TOML_FILE;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;

#[tokio::test]
async fn load_global_mcp_servers_returns_empty_if_missing() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[tokio::test]
async fn load_global_mcp_servers_accepts_legacy_ms_field() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    std::fs::write(
        &config_path,
        r#"
[mcp_servers]
[mcp_servers.docs]
command = "echo"
startup_timeout_ms = 2500
"#,
    )?;

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let docs = servers.get("docs").expect("docs entry");
    assert_eq!(docs.startup_timeout_sec, Some(Duration::from_millis(2500)));

    Ok(())
}

#[test]
fn mcp_servers_toml_parses_per_tool_approval_overrides() {
    let config = toml::from_str::<ConfigToml>(
        r#"
[mcp_servers.docs]
command = "docs-server"
name = "Docs"

[mcp_servers.docs.tools.search]
approval_mode = "approve"
"#,
    )
    .expect("TOML deserialization should succeed");
    let tool = config
        .mcp_servers
        .get("docs")
        .and_then(|server| server.tools.get("search"))
        .expect("docs/search tool config exists");

    assert_eq!(
        tool,
        &McpServerToolConfig {
            approval_mode: Some(AppToolApproval::Approve),
        }
    );
}

#[test]
fn mcp_servers_toml_parses_legacy_flattened_per_tool_approval_overrides() {
    let config = toml::from_str::<ConfigToml>(
        r#"
[mcp_servers.docs]
command = "docs-server"

[mcp_servers.docs.search]
approval_mode = "approve"
"#,
    )
    .expect("legacy TOML deserialization should succeed");
    let tool = config
        .mcp_servers
        .get("docs")
        .and_then(|server| server.tools.get("search"))
        .expect("docs/search tool config exists");

    assert_eq!(
        tool,
        &McpServerToolConfig {
            approval_mode: Some(AppToolApproval::Approve),
        }
    );
}

#[test]
fn mcp_servers_toml_parses_tool_approval_override_for_reserved_name() {
    let config = toml::from_str::<ConfigToml>(
        r#"
[mcp_servers.docs]
command = "docs-server"

[mcp_servers.docs.tools.command]
approval_mode = "approve"
"#,
    )
    .expect("TOML deserialization should succeed");
    let tool = config
        .mcp_servers
        .get("docs")
        .and_then(|server| server.tools.get("command"))
        .expect("docs/command tool config exists");

    assert_eq!(
        tool,
        &McpServerToolConfig {
            approval_mode: Some(AppToolApproval::Approve),
        }
    );
}

#[tokio::test]
async fn load_global_mcp_servers_rejects_inline_bearer_token() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    std::fs::write(
        &config_path,
        r#"
[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token = "secret"
"#,
    )?;

    let err = load_global_mcp_servers(codex_home.path())
        .await
        .expect_err("bearer_token entries should be rejected");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("bearer_token"));
    assert!(err.to_string().contains("bearer_token_env_var"));

    Ok(())
}
