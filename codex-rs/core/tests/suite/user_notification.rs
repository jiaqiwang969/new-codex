#![cfg(not(target_os = "windows"))]

use std::os::unix::fs::PermissionsExt;

use codex_hooks::EntireSummary;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::fs_wait;
use core_test_support::responses;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use responses::ev_assistant_message;
use responses::ev_completed;
use responses::sse;
use responses::start_mock_server;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_rewrites_notify_payload_for_mutating_turn() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let shell_args = serde_json::to_string(&json!({
        "command": ["/bin/sh", "-c", "printf 'after\n' > tracked.txt"],
        "timeout_ms": 2_000
    }))?;
    let summary_json = serde_json::to_string(&json!({
        "is_meaningful": true,
        "motivation": "Update tracked file via shell",
        "approach": "Used the shell tool to overwrite tracked.txt",
        "challenges": null,
        "tradeoffs": null,
        "outcome": "Tracked file changed and completion payload captured."
    }))?;

    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                responses::ev_response_created("resp-1"),
                ev_function_call("call-1", "shell", &shell_args),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "Done"),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_assistant_message("msg-2", &summary_json),
                ev_completed("resp-3"),
            ]),
        ],
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
payload_path="$(dirname "${0}")/notify.txt"
tmp_path="${payload_path}.tmp"
echo -n "${@: -1}" > "${tmp_path}"
mv "${tmp_path}" "${payload_path}""#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let test = test_codex()
        .with_model("gpt-5.1-codex")
        .with_config(move |cfg| cfg.notify = Some(vec![notify_script_str]))
        .build(&server)
        .await?;
    let cwd = test.cwd_path().to_path_buf();

    for cmd in [
        &["git", "init", "-q"][..],
        &["git", "config", "user.name", "Test User"][..],
        &["git", "config", "user.email", "test@example.com"][..],
    ] {
        let status = tokio::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .current_dir(&cwd)
            .status()
            .await?;
        assert!(status.success(), "git command should succeed: {cmd:?}");
    }
    std::fs::write(cwd.join("tracked.txt"), "before\n")?;
    let add_status = tokio::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(add_status.success());
    let commit_status = tokio::process::Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(commit_status.success());

    test.submit_turn_with_policy(
        "update the tracked file",
        codex_protocol::protocol::SandboxPolicy::DangerFullAccess,
    )
    .await?;

    let shell_output = mock
        .function_call_output_text("call-1")
        .expect("shell output");
    assert!(
        shell_output.contains("Exit code: 0"),
        "shell output: {shell_output}"
    );
    assert_eq!(
        tokio::fs::read_to_string(cwd.join("tracked.txt")).await?,
        "after\n"
    );
    assert_eq!(mock.requests().len(), 3);

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let notify_payload_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payload: Value = serde_json::from_str(&notify_payload_raw)?;
    let turn_id = payload["turn-id"].as_str().expect("turn id");

    assert_eq!(payload["type"], json!("agent-turn-complete"));
    assert_eq!(
        payload["input-messages"],
        json!([
            "Update tracked file via shell → Tracked file changed and completion payload captured.\n\nMotivation: Update tracked file via shell\nApproach: Used the shell tool to overwrite tracked.txt\nChallenges: None\nTradeoffs: None"
        ])
    );
    assert_eq!(payload["last-assistant-message"], json!("Done"));

    let saved_summary = codex_hooks::load_summary(&cwd, turn_id)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?
        .expect("saved summary");
    assert_eq!(
        saved_summary,
        EntireSummary {
            is_meaningful: true,
            motivation: Some("Update tracked file via shell".to_string()),
            approach: Some("Used the shell tool to overwrite tracked.txt".to_string()),
            challenges: None,
            tradeoffs: None,
            outcome: Some("Tracked file changed and completion payload captured.".to_string()),
        }
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_does_not_rewrite_notify_payload_for_preexisting_dirty_repo()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mock = responses::mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-1", "Done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
payload_path="$(dirname "${0}")/notify.txt"
tmp_path="${payload_path}.tmp"
echo -n "${@: -1}" > "${tmp_path}"
mv "${tmp_path}" "${payload_path}""#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let test = test_codex()
        .with_config(move |cfg| cfg.notify = Some(vec![notify_script_str]))
        .build(&server)
        .await?;
    let cwd = test.cwd_path().to_path_buf();

    for cmd in [
        &["git", "init", "-q"][..],
        &["git", "config", "user.name", "Test User"][..],
        &["git", "config", "user.email", "test@example.com"][..],
    ] {
        let status = tokio::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .current_dir(&cwd)
            .status()
            .await?;
        assert!(status.success(), "git command should succeed: {cmd:?}");
    }
    std::fs::write(cwd.join("tracked.txt"), "before\n")?;
    let add_status = tokio::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(add_status.success());
    let commit_status = tokio::process::Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(commit_status.success());
    std::fs::write(cwd.join("tracked.txt"), "dirty before turn\n")?;

    test.submit_turn("hello world").await?;

    assert_eq!(mock.requests().len(), 1);
    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let notify_payload_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payload: Value = serde_json::from_str(&notify_payload_raw)?;

    assert_eq!(payload["type"], json!("agent-turn-complete"));
    assert_eq!(payload["input-messages"], json!(["hello world"]));
    assert_eq!(payload["last-assistant-message"], json!("Done"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_does_not_rewrite_notify_payload_when_entire_summary_disabled()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let shell_args = serde_json::to_string(&json!({
        "command": ["/bin/sh", "-c", "printf 'after\n' > tracked.txt"],
        "timeout_ms": 2_000
    }))?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                responses::ev_response_created("resp-1"),
                ev_function_call("call-1", "shell", &shell_args),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "Done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
payload_path="$(dirname "${0}")/notify.txt"
tmp_path="${payload_path}.tmp"
echo -n "${@: -1}" > "${tmp_path}"
mv "${tmp_path}" "${payload_path}""#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let test = test_codex()
        .with_model("gpt-5.1-codex")
        .with_config(move |cfg| {
            cfg.notify = Some(vec![notify_script_str]);
            cfg.memories.entire_summary_enabled = false;
        })
        .build(&server)
        .await?;
    let cwd = test.cwd_path().to_path_buf();

    for cmd in [
        &["git", "init", "-q"][..],
        &["git", "config", "user.name", "Test User"][..],
        &["git", "config", "user.email", "test@example.com"][..],
    ] {
        let status = tokio::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .current_dir(&cwd)
            .status()
            .await?;
        assert!(status.success(), "git command should succeed: {cmd:?}");
    }
    std::fs::write(cwd.join("tracked.txt"), "before\n")?;
    let add_status = tokio::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(add_status.success());
    let commit_status = tokio::process::Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(&cwd)
        .status()
        .await?;
    assert!(commit_status.success());

    test.submit_turn_with_policy(
        "update the tracked file",
        codex_protocol::protocol::SandboxPolicy::DangerFullAccess,
    )
    .await?;

    assert_eq!(mock.requests().len(), 2);
    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let notify_payload_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payload: Value = serde_json::from_str(&notify_payload_raw)?;

    assert_eq!(payload["type"], json!("agent-turn-complete"));
    assert_eq!(
        payload["input-messages"],
        json!(["update the tracked file"])
    );
    assert_eq!(payload["last-assistant-message"], json!("Done"));
    assert!(
        codex_hooks::load_summary(&cwd, payload["turn-id"].as_str().expect("turn id"))
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .is_none()
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_three_requests_and_instructions() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let sse1 = sse(vec![ev_assistant_message("m1", "Done"), ev_completed("r1")]);

    responses::mount_sse_once(&server, sse1).await;

    let notify_dir = TempDir::new()?;
    // write a script to the notify that touches a file next to it
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
payload_path="$(dirname "${0}")/notify.txt"
tmp_path="${payload_path}.tmp"
echo -n "${@: -1}" > "${tmp_path}"
mv "${tmp_path}" "${payload_path}""#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |cfg| cfg.notify = Some(vec![notify_script_str]))
        .build(&server)
        .await?;

    // 1) Normal user input – should hit server once.
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello world".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await?;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // We fork the notify script, so we need to wait for it to write to the file.
    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let notify_payload_raw = tokio::fs::read_to_string(&notify_file).await?;
    let payload: Value = serde_json::from_str(&notify_payload_raw)?;

    assert_eq!(payload["type"], json!("agent-turn-complete"));
    assert_eq!(payload["input-messages"], json!(["hello world"]));
    assert_eq!(payload["last-assistant-message"], json!("Done"));

    Ok(())
}
