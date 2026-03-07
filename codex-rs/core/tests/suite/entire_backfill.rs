#![cfg(not(target_os = "windows"))]

use std::path::Path;
use std::time::Duration;

use codex_hooks::EntireSummary;
use core_test_support::fs_wait;
use core_test_support::responses;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

use responses::ev_assistant_message;
use responses::ev_completed;
use responses::sse;
use responses::start_mock_server;

async fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    let status = tokio::process::Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .status()
        .await?;
    assert!(status.success(), "git command should succeed: {args:?}");
    Ok(())
}

async fn seed_entire_checkpoint(
    cwd: &Path,
    checkpoint_id: &str,
    commit_message: &str,
    with_file_change: bool,
) -> anyhow::Result<()> {
    run_git(cwd, &["git", "init", "-q"]).await?;
    run_git(cwd, &["git", "config", "user.name", "Test User"]).await?;
    run_git(cwd, &["git", "config", "user.email", "test@example.com"]).await?;

    std::fs::write(cwd.join("tracked.txt"), "before\n")?;
    run_git(cwd, &["git", "add", "tracked.txt"]).await?;
    run_git(cwd, &["git", "commit", "-qm", "initial"]).await?;

    if with_file_change {
        std::fs::write(cwd.join("tracked.txt"), "after\n")?;
        run_git(cwd, &["git", "add", "tracked.txt"]).await?;
        run_git(
            cwd,
            &[
                "git",
                "commit",
                "-qm",
                commit_message,
                "-m",
                &format!("Entire-Checkpoint: {checkpoint_id}"),
            ],
        )
        .await?;
    } else {
        run_git(
            cwd,
            &[
                "git",
                "commit",
                "--allow-empty",
                "-qm",
                commit_message,
                "-m",
                &format!("Entire-Checkpoint: {checkpoint_id}"),
            ],
        )
        .await?;
    }

    run_git(
        cwd,
        &["git", "checkout", "-qb", &format!("entire/{checkpoint_id}")],
    )
    .await?;
    std::fs::write(
        cwd.join("session.json"),
        r#"{"model_slug":"gpt-5.3-codex"}"#,
    )?;
    run_git(cwd, &["git", "add", "session.json"]).await?;
    run_git(cwd, &["git", "commit", "-qm", "Store session metadata"]).await?;
    run_git(cwd, &["git", "checkout", "-"]).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backfills_missing_recent_entire_summary_after_turn() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let summary_json = serde_json::to_string(&json!({
        "is_meaningful": true,
        "motivation": "Keep prior Entire context reusable",
        "approach": "Backfill the missing historical WHY summary after the turn",
        "challenges": null,
        "tradeoffs": null,
        "outcome": "Future turns can reuse the older checkpoint rationale"
    }))?;
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("msg-1", "Done"),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-2", &summary_json),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let test = test_codex()
        .with_model("gpt-5.1-codex")
        .build(&server)
        .await?;
    let cwd = test.cwd_path().to_path_buf();
    let checkpoint_id = "checkpoint-backfill-1";
    seed_entire_checkpoint(&cwd, checkpoint_id, "Add Entire history plumbing", true).await?;

    test.submit_turn("hello world").await?;

    let summary_path = cwd
        .join(".entire")
        .join("summaries")
        .join(format!("{checkpoint_id}.json"));
    fs_wait::wait_for_path_exists(&summary_path, Duration::from_secs(5)).await?;
    assert_eq!(mock.requests().len(), 2);

    let saved_summary = codex_hooks::load_summary(&cwd, checkpoint_id)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?
        .expect("saved summary");
    assert_eq!(
        saved_summary,
        EntireSummary {
            is_meaningful: true,
            motivation: Some("Keep prior Entire context reusable".to_string()),
            approach: Some(
                "Backfill the missing historical WHY summary after the turn".to_string()
            ),
            challenges: None,
            tradeoffs: None,
            outcome: Some("Future turns can reuse the older checkpoint rationale".to_string()),
        }
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn does_not_backfill_trivial_recent_entire_summary_after_turn() -> anyhow::Result<()> {
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

    let test = test_codex()
        .with_model("gpt-5.1-codex")
        .build(&server)
        .await?;
    let cwd = test.cwd_path().to_path_buf();
    let checkpoint_id = "checkpoint-trivial-1";
    seed_entire_checkpoint(&cwd, checkpoint_id, "hi", false).await?;

    test.submit_turn("hello world").await?;

    tokio::time::sleep(Duration::from_millis(500)).await;
    let summary_path = cwd
        .join(".entire")
        .join("summaries")
        .join(format!("{checkpoint_id}.json"));
    assert!(!summary_path.exists());
    assert_eq!(mock.requests().len(), 1);

    Ok(())
}
