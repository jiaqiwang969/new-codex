use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn exec_wrapper_uses_codex_exec_config_error_format() -> Result<(), Box<dyn std::error::Error>> {
    let codex_home = TempDir::new()?;
    fs::write(codex_home.path().join("config.toml"), "model = [\n")?;

    let output = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["exec", "--skip-git-repo-check", "hi"])
        .output()?;

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("Error loading config.toml:"),
        "expected codex-exec config error format, stderr: {stderr}"
    );

    Ok(())
}
