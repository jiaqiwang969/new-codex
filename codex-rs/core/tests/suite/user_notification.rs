#![cfg(not(target_os = "windows"))]

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

use codex_core::config::types::McpServerConfig;
use codex_core::config::types::McpServerTransportConfig;
use codex_core::features::Feature;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::request_user_input::RequestUserInputAnswer;
use codex_protocol::request_user_input::RequestUserInputResponse;
use codex_protocol::user_input::UserInput;
use core_test_support::fs_wait;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::stdio_server_bin;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use serial_test::serial;
use tempfile::TempDir;
use tokio::time::sleep;

use responses::ev_assistant_message;
use responses::ev_completed;
use responses::sse;
use responses::start_mock_server;
use std::time::Duration;

const CODEX_APPS_SERVER_NAME: &str = "codex_apps";
const MCP_TEST_ENABLE_SOFT_ERROR_TOOL: &str = "MCP_TEST_ENABLE_SOFT_ERROR_TOOL";
const MCP_TEST_ENABLE_APPROVAL_TOOL: &str = "MCP_TEST_ENABLE_APPROVAL_TOOL";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "flaky on ubuntu-24.04-arm - aarch64-unknown-linux-gnu"]
// The notify script gets far enough to create (and therefore surface) the file,
// but hasn’t flushed the JSON yet. Reading an empty file produces EOF while parsing
// a value at line 1 column 0. May be caused by a slow runner.
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
echo -n "${@: -1}" > $(dirname "${0}")/notify.txt"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.txt");
    let notify_script_str = notify_script.to_str().unwrap().to_string();

    let TestCodex { codex, .. } = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |cfg| {
            cfg.features.enable(Feature::MemoryTool);
            cfg.features.disable(Feature::SearchTool);
            cfg.notify = Some(vec![notify_script_str]);
        })
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
    assert_eq!(
        payload["provider-name"]
            .as_str()
            .map(|value| !value.trim().is_empty()),
        Some(true)
    );
    assert_eq!(
        payload["model-slug"]
            .as_str()
            .map(|value| !value.trim().is_empty()),
        Some(true)
    );
    assert_eq!(
        payload["memory-context"]["active-scope-kind"],
        json!("user")
    );
    assert_eq!(
        payload["memory-context"]["user-memory-summary-exists"],
        json!(true)
    );
    assert_eq!(
        payload["memory-context"]["active-memory-scope-version"]
            .as_str()
            .map(|value| value.starts_with("user:")),
        Some(true)
    );
    assert_eq!(
        payload["memory-scope-version"],
        payload["memory-context"]["active-memory-scope-version"]
    );
    assert_eq!(
        payload["memory-scope-kind"],
        payload["memory-context"]["active-scope-kind"]
    );
    assert_eq!(
        payload["memory-summary-sha256"],
        payload["memory-context"]["active-memory-summary-sha256"]
    );
    assert_eq!(
        payload["memory-binding-key"],
        payload["memory-context"]["active-memory-binding-key"]
    );
    let memory_scope_version = payload["memory-scope-version"]
        .as_str()
        .expect("top-level memory-scope-version");
    let (_scope_kind, short_hash) = memory_scope_version
        .split_once(':')
        .expect("memory-scope-version should contain scope prefix");
    let memory_summary_sha256 = payload["memory-summary-sha256"]
        .as_str()
        .expect("top-level memory-summary-sha256");
    assert!(memory_summary_sha256.starts_with(short_hash));
    let memory_binding_key = payload["memory-binding-key"]
        .as_str()
        .expect("top-level memory-binding-key");
    assert_eq!(
        memory_binding_key,
        format!("{memory_scope_version}:{memory_summary_sha256}")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_mcp_tool_call_complete_payload() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-1";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__echo");
    responses::mount_function_call_agent_response(
        &server,
        call_id,
        "{\"message\":\"ping\"}",
        &tool_name,
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp echo tool",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"));
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if let (Some(mcp_payload), Some(agent_payload)) = (mcp_payload, agent_payload) {
            assert_eq!(mcp_payload["server"], json!(server_name));
            assert_eq!(mcp_payload["tool-name"], json!("echo"));
            assert_eq!(mcp_payload["status"], json!("ok"));
            assert_eq!(
                mcp_payload["provider-name"]
                    .as_str()
                    .map(|value| !value.trim().is_empty()),
                Some(true)
            );
            assert_eq!(
                mcp_payload["model-slug"]
                    .as_str()
                    .map(|value| !value.trim().is_empty()),
                Some(true)
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-scope-kind"],
                json!("user")
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-memory-scope-version"]
                    .as_str()
                    .map(|value| value.starts_with("user:")),
                Some(true)
            );
            assert_eq!(
                mcp_payload["memory-scope-version"],
                mcp_payload["memory-context"]["active-memory-scope-version"]
            );
            assert_eq!(
                mcp_payload["memory-scope-kind"],
                mcp_payload["memory-context"]["active-scope-kind"]
            );
            assert_eq!(
                mcp_payload["memory-summary-sha256"],
                mcp_payload["memory-context"]["active-memory-summary-sha256"]
            );
            assert_eq!(
                mcp_payload["memory-binding-key"],
                mcp_payload["memory-context"]["active-memory-binding-key"]
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-memory-scope-version"],
                agent_payload["memory-context"]["active-memory-scope-version"]
            );
            assert_eq!(
                mcp_payload["memory-scope-version"],
                agent_payload["memory-scope-version"]
            );
            assert_eq!(
                mcp_payload["memory-scope-kind"],
                agent_payload["memory-scope-kind"]
            );
            assert_eq!(
                mcp_payload["memory-summary-sha256"],
                agent_payload["memory-summary-sha256"]
            );
            assert_eq!(
                mcp_payload["memory-binding-key"],
                agent_payload["memory-binding-key"]
            );
            let memory_scope_version = mcp_payload["memory-scope-version"]
                .as_str()
                .expect("mcp payload should include top-level memory scope version");
            let (_scope_kind, short_hash) = memory_scope_version
                .split_once(':')
                .expect("memory scope version should contain scope prefix");
            let memory_summary_sha256 = mcp_payload["memory-summary-sha256"]
                .as_str()
                .expect("mcp payload should include memory summary sha");
            assert!(memory_summary_sha256.starts_with(short_hash));
            let memory_binding_key = mcp_payload["memory-binding-key"]
                .as_str()
                .expect("mcp payload should include memory binding key");
            assert_eq!(
                memory_binding_key,
                format!("{memory_scope_version}:{memory_summary_sha256}")
            );
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for mcp-tool-call-complete payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_mcp_memory_scope_version_matches_developer_instructions() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-memory-scope-match-1";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__echo");
    let response_mocks = responses::mount_function_call_agent_response(
        &server,
        call_id,
        "{\"message\":\"ping\"}",
        &tool_name,
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp echo tool",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    let function_call_request = response_mocks.function_call.single_request();
    let developer_texts = function_call_request.message_input_texts("developer");
    let scope_version = developer_texts
        .iter()
        .find_map(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Active memory scope version: ")
                    .map(|value| value.trim().to_string())
            })
        })
        .expect("memory scope version should appear in developer instructions");

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"));
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if let (Some(mcp_payload), Some(agent_payload)) = (mcp_payload, agent_payload) {
            let mcp_scope_version = mcp_payload["memory-context"]["active-memory-scope-version"]
                .as_str()
                .expect("mcp payload should include memory scope version");
            let agent_scope_version =
                agent_payload["memory-context"]["active-memory-scope-version"]
                    .as_str()
                    .expect("agent payload should include memory scope version");
            let mcp_top_level_scope_version = mcp_payload["memory-scope-version"]
                .as_str()
                .expect("mcp payload should include top-level memory scope version");
            let agent_top_level_scope_version = agent_payload["memory-scope-version"]
                .as_str()
                .expect("agent payload should include top-level memory scope version");
            let mcp_top_level_scope_kind = mcp_payload["memory-scope-kind"]
                .as_str()
                .expect("mcp payload should include top-level memory scope kind");
            let agent_top_level_scope_kind = agent_payload["memory-scope-kind"]
                .as_str()
                .expect("agent payload should include top-level memory scope kind");
            let mcp_top_level_summary_sha = mcp_payload["memory-summary-sha256"]
                .as_str()
                .expect("mcp payload should include top-level memory summary sha");
            let agent_top_level_summary_sha = agent_payload["memory-summary-sha256"]
                .as_str()
                .expect("agent payload should include top-level memory summary sha");
            let mcp_top_level_binding_key = mcp_payload["memory-binding-key"]
                .as_str()
                .expect("mcp payload should include top-level memory binding key");
            let agent_top_level_binding_key = agent_payload["memory-binding-key"]
                .as_str()
                .expect("agent payload should include top-level memory binding key");

            assert_eq!(mcp_scope_version, scope_version.as_str());
            assert_eq!(agent_scope_version, scope_version.as_str());
            assert_eq!(mcp_top_level_scope_version, scope_version.as_str());
            assert_eq!(agent_top_level_scope_version, scope_version.as_str());
            assert_eq!(mcp_top_level_scope_kind, "user");
            assert_eq!(agent_top_level_scope_kind, "user");
            assert_eq!(mcp_top_level_summary_sha, agent_top_level_summary_sha);
            let (_scope_kind, short_hash) = scope_version
                .split_once(':')
                .expect("memory scope version should contain scope prefix");
            assert!(mcp_top_level_summary_sha.starts_with(short_hash));
            assert_eq!(
                mcp_top_level_binding_key,
                format!("{scope_version}:{mcp_top_level_summary_sha}")
            );
            assert_eq!(mcp_top_level_binding_key, agent_top_level_binding_key);
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for memory-scope-match payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_mcp_tool_call_transport_error_payload() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-transport-1";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__not_a_real_tool");
    responses::mount_function_call_agent_response(&server, call_id, "{}", &tool_name).await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call an rmcp tool that does not exist",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(payload) = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"))
        {
            assert_eq!(payload["server"], json!(server_name));
            assert_eq!(payload["tool-name"], json!("not_a_real_tool"));
            assert_eq!(payload["status"], json!("transport-error"));
            assert_eq!(
                payload["error-message"]
                    .as_str()
                    .map(|message| message.contains("tool call error:")),
                Some(true)
            );
            assert_eq!(
                payload["memory-context"]["active-scope-kind"],
                json!("user")
            );
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for transport-error payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_mcp_tool_call_tool_error_payload() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-tool-error-1";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__soft_error");
    responses::mount_function_call_agent_response(&server, call_id, "{}", &tool_name).await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: Some(HashMap::from([(
                            MCP_TEST_ENABLE_SOFT_ERROR_TOOL.to_string(),
                            "1".to_string(),
                        )])),
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp soft_error tool",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"));
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if let (Some(mcp_payload), Some(agent_payload)) = (mcp_payload, agent_payload) {
            assert_eq!(mcp_payload["server"], json!(server_name));
            assert_eq!(mcp_payload["tool-name"], json!("soft_error"));
            assert_eq!(mcp_payload["status"], json!("tool-error"));
            assert_eq!(mcp_payload["error-message"], Value::Null);
            assert_eq!(
                mcp_payload["memory-context"]["active-scope-kind"],
                json!("user")
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-memory-scope-version"],
                agent_payload["memory-context"]["active-memory-scope-version"]
            );
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for tool-error payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_consistent_memory_version_for_multiple_mcp_calls() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let first_call_id = "notify-mcp-multi-1";
    let second_call_id = "notify-mcp-multi-2";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__echo");
    let first_response = sse(vec![
        responses::ev_response_created("resp-multi-1"),
        responses::ev_function_call(first_call_id, &tool_name, "{\"message\":\"first\"}"),
        responses::ev_function_call(second_call_id, &tool_name, "{\"message\":\"second\"}"),
        ev_completed("resp-multi-1"),
    ]);
    let second_response = sse(vec![
        ev_assistant_message("msg-1", "done"),
        ev_completed("resp-multi-2"),
    ]);
    responses::mount_sse_sequence(&server, vec![first_response, second_response]).await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp echo tool twice",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payloads: Vec<&Value> = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("mcp-tool-call-complete"))
            .collect();
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if mcp_payloads.len() >= 2
            && let Some(agent_payload) = agent_payload
        {
            let mut call_ids = mcp_payloads
                .iter()
                .filter_map(|payload| payload["call-id"].as_str())
                .collect::<Vec<_>>();
            call_ids.sort_unstable();
            assert_eq!(call_ids, vec![first_call_id, second_call_id]);

            let expected_memory_version =
                &agent_payload["memory-context"]["active-memory-scope-version"];
            for payload in mcp_payloads {
                assert_eq!(payload["server"], json!(server_name));
                assert_eq!(payload["tool-name"], json!("echo"));
                assert_eq!(payload["status"], json!("ok"));
                assert_eq!(
                    payload["memory-context"]["active-memory-scope-version"],
                    *expected_memory_version
                );
            }
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for multi-call payloads: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_mcp_tool_call_declined_payload() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-declined-1";
    let tool_name = format!("mcp__{CODEX_APPS_SERVER_NAME}__dangerous_write");
    responses::mount_function_call_agent_response(&server, call_id, "{}", &tool_name).await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                CODEX_APPS_SERVER_NAME.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: Some(HashMap::from([(
                            MCP_TEST_ENABLE_APPROVAL_TOOL.to_string(),
                            "1".to_string(),
                        )])),
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    submit_mcp_approval_turn(&fixture, "call codex apps dangerous tool", "Deny").await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"));
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if let (Some(mcp_payload), Some(agent_payload)) = (mcp_payload, agent_payload) {
            assert_eq!(mcp_payload["server"], json!(CODEX_APPS_SERVER_NAME));
            assert_eq!(mcp_payload["tool-name"], json!("dangerous_write"));
            assert_eq!(mcp_payload["status"], json!("declined"));
            assert_eq!(
                mcp_payload["error-message"],
                json!("user rejected MCP tool call")
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-memory-scope-version"],
                agent_payload["memory-context"]["active-memory-scope-version"]
            );
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for declined payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_emits_mcp_tool_call_cancelled_payload() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    let call_id = "notify-mcp-cancelled-1";
    let tool_name = format!("mcp__{CODEX_APPS_SERVER_NAME}__dangerous_write");
    responses::mount_function_call_agent_response(&server, call_id, "{}", &tool_name).await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                CODEX_APPS_SERVER_NAME.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: Some(HashMap::from([(
                            MCP_TEST_ENABLE_APPROVAL_TOOL.to_string(),
                            "1".to_string(),
                        )])),
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    submit_mcp_approval_turn(&fixture, "call codex apps dangerous tool", "Cancel").await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("mcp-tool-call-complete"));
        let agent_payload = payloads
            .iter()
            .find(|payload| payload["type"] == json!("agent-turn-complete"));

        if let (Some(mcp_payload), Some(agent_payload)) = (mcp_payload, agent_payload) {
            assert_eq!(mcp_payload["server"], json!(CODEX_APPS_SERVER_NAME));
            assert_eq!(mcp_payload["tool-name"], json!("dangerous_write"));
            assert_eq!(mcp_payload["status"], json!("cancelled"));
            assert_eq!(
                mcp_payload["error-message"],
                json!("user cancelled MCP tool call")
            );
            assert_eq!(
                mcp_payload["memory-context"]["active-memory-scope-version"],
                agent_payload["memory-context"]["active-memory-scope-version"]
            );
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for cancelled payload: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_remembers_mcp_tool_approval_for_session_and_keeps_memory_bound()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let first_call_id = "notify-mcp-approved-session-1";
    let second_call_id = "notify-mcp-approved-session-2";
    let tool_name = format!("mcp__{CODEX_APPS_SERVER_NAME}__dangerous_write");

    let first_turn_response = sse(vec![
        responses::ev_response_created("resp-approval-1"),
        responses::ev_function_call(first_call_id, &tool_name, "{}"),
        ev_completed("resp-approval-1"),
    ]);
    let first_turn_completion = sse(vec![
        ev_assistant_message("msg-approval-1", "first done"),
        ev_completed("resp-approval-2"),
    ]);
    let second_turn_response = sse(vec![
        responses::ev_response_created("resp-approval-3"),
        responses::ev_function_call(second_call_id, &tool_name, "{}"),
        ev_completed("resp-approval-3"),
    ]);
    let second_turn_completion = sse(vec![
        ev_assistant_message("msg-approval-2", "second done"),
        ev_completed("resp-approval-4"),
    ]);
    responses::mount_sse_sequence(
        &server,
        vec![
            first_turn_response,
            first_turn_completion,
            second_turn_response,
            second_turn_completion,
        ],
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                CODEX_APPS_SERVER_NAME.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: Some(HashMap::from([(
                            MCP_TEST_ENABLE_APPROVAL_TOOL.to_string(),
                            "1".to_string(),
                        )])),
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    submit_mcp_approval_turn(
        &fixture,
        "call codex apps dangerous tool",
        "Approve this Session",
    )
    .await?;

    let session_model = fixture.session_configured.model.clone();
    fixture
        .codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "call codex apps dangerous tool again".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            cwd: fixture.cwd.path().to_path_buf(),
            approval_policy: AskForApproval::OnRequest,
            sandbox_policy: SandboxPolicy::new_read_only_policy(),
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
            collaboration_mode: None,
            personality: None,
        })
        .await?;

    let second_turn_terminal_event = wait_for_event(&fixture.codex, |event| {
        matches!(
            event,
            EventMsg::TurnComplete(_) | EventMsg::RequestUserInput(_)
        )
    })
    .await;
    assert!(
        matches!(second_turn_terminal_event, EventMsg::TurnComplete(_)),
        "expected remembered approval to avoid extra prompt, got {second_turn_terminal_event:?}"
    );

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("mcp-tool-call-complete"))
            .collect::<Vec<_>>();
        let agent_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("agent-turn-complete"))
            .collect::<Vec<_>>();

        if mcp_payloads.len() >= 2 && agent_payloads.len() >= 2 {
            let mut call_ids = mcp_payloads
                .iter()
                .filter_map(|payload| payload["call-id"].as_str())
                .collect::<Vec<_>>();
            call_ids.sort_unstable();
            assert_eq!(call_ids, vec![first_call_id, second_call_id]);

            let turn_memory_versions = agent_payloads
                .iter()
                .filter_map(|payload| {
                    let turn_id = payload["turn-id"].as_str()?;
                    let memory_version =
                        payload["memory-context"]["active-memory-scope-version"].as_str()?;
                    Some((turn_id, memory_version))
                })
                .collect::<HashMap<_, _>>();
            assert_eq!(turn_memory_versions.len(), agent_payloads.len());

            for payload in mcp_payloads {
                assert_eq!(payload["server"], json!(CODEX_APPS_SERVER_NAME));
                assert_eq!(payload["tool-name"], json!("dangerous_write"));
                assert_eq!(payload["status"], json!("ok"));
                assert_eq!(payload["error-message"], Value::Null);

                let turn_id = payload["turn-id"]
                    .as_str()
                    .expect("mcp payload should include turn-id");
                let expected_memory_version = turn_memory_versions
                    .get(turn_id)
                    .expect("agent payload should exist for mcp payload turn");
                assert_eq!(
                    payload["memory-context"]["active-memory-scope-version"]
                        .as_str()
                        .expect("mcp payload should include memory scope version"),
                    *expected_memory_version
                );
            }
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for remembered-approval payloads: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_approve_once_requires_reapproval_next_turn() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let first_call_id = "notify-mcp-approve-once-1";
    let second_call_id = "notify-mcp-approve-once-2";
    let tool_name = format!("mcp__{CODEX_APPS_SERVER_NAME}__dangerous_write");

    let first_turn_response = sse(vec![
        responses::ev_response_created("resp-approve-once-1"),
        responses::ev_function_call(first_call_id, &tool_name, "{}"),
        ev_completed("resp-approve-once-1"),
    ]);
    let first_turn_completion = sse(vec![
        ev_assistant_message("msg-approve-once-1", "first done"),
        ev_completed("resp-approve-once-2"),
    ]);
    let second_turn_response = sse(vec![
        responses::ev_response_created("resp-approve-once-3"),
        responses::ev_function_call(second_call_id, &tool_name, "{}"),
        ev_completed("resp-approve-once-3"),
    ]);
    let second_turn_completion = sse(vec![
        ev_assistant_message("msg-approve-once-2", "second done"),
        ev_completed("resp-approve-once-4"),
    ]);
    responses::mount_sse_sequence(
        &server,
        vec![
            first_turn_response,
            first_turn_completion,
            second_turn_response,
            second_turn_completion,
        ],
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(user_memory_root.join("memory_summary.md"), "user summary")
                .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.enable(Feature::Apps);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                CODEX_APPS_SERVER_NAME.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: Some(HashMap::from([(
                            MCP_TEST_ENABLE_APPROVAL_TOOL.to_string(),
                            "1".to_string(),
                        )])),
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    submit_mcp_approval_turn(&fixture, "call codex apps dangerous tool", "Approve Once").await?;
    submit_mcp_approval_turn(&fixture, "call codex apps dangerous tool again", "Deny").await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("mcp-tool-call-complete"))
            .collect::<Vec<_>>();
        let agent_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("agent-turn-complete"))
            .collect::<Vec<_>>();

        if mcp_payloads.len() >= 2 && agent_payloads.len() >= 2 {
            let status_by_call_id = mcp_payloads
                .iter()
                .filter_map(|payload| {
                    let call_id = payload["call-id"].as_str()?;
                    let status = payload["status"].as_str()?;
                    Some((call_id, status))
                })
                .collect::<HashMap<_, _>>();
            assert_eq!(status_by_call_id.get(first_call_id), Some(&"ok"));
            assert_eq!(status_by_call_id.get(second_call_id), Some(&"declined"));

            let turn_memory_versions = agent_payloads
                .iter()
                .filter_map(|payload| {
                    let turn_id = payload["turn-id"].as_str()?;
                    let memory_version =
                        payload["memory-context"]["active-memory-scope-version"].as_str()?;
                    Some((turn_id, memory_version))
                })
                .collect::<HashMap<_, _>>();
            assert_eq!(turn_memory_versions.len(), agent_payloads.len());

            for payload in mcp_payloads {
                let turn_id = payload["turn-id"]
                    .as_str()
                    .expect("mcp payload should include turn-id");
                let expected_memory_version = turn_memory_versions
                    .get(turn_id)
                    .expect("agent payload should exist for mcp payload turn");
                assert_eq!(
                    payload["memory-context"]["active-memory-scope-version"]
                        .as_str()
                        .expect("mcp payload should include memory scope version"),
                    *expected_memory_version
                );
            }
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for approve-once payloads: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(mcp_test_value)]
async fn notify_mcp_memory_scope_version_updates_across_turns() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let first_call_id = "notify-mcp-memory-version-1";
    let second_call_id = "notify-mcp-memory-version-2";
    let server_name = "rmcp";
    let tool_name = format!("mcp__{server_name}__echo");

    let first_turn_response = sse(vec![
        responses::ev_response_created("resp-memory-version-1"),
        responses::ev_function_call(first_call_id, &tool_name, "{\"message\":\"first\"}"),
        ev_completed("resp-memory-version-1"),
    ]);
    let first_turn_completion = sse(vec![
        ev_assistant_message("msg-memory-version-1", "first done"),
        ev_completed("resp-memory-version-2"),
    ]);
    let second_turn_response = sse(vec![
        responses::ev_response_created("resp-memory-version-3"),
        responses::ev_function_call(second_call_id, &tool_name, "{\"message\":\"second\"}"),
        ev_completed("resp-memory-version-3"),
    ]);
    let second_turn_completion = sse(vec![
        ev_assistant_message("msg-memory-version-2", "second done"),
        ev_completed("resp-memory-version-4"),
    ]);
    responses::mount_sse_sequence(
        &server,
        vec![
            first_turn_response,
            first_turn_completion,
            second_turn_response,
            second_turn_completion,
        ],
    )
    .await;

    let notify_dir = TempDir::new()?;
    let notify_script = notify_dir.path().join("notify.sh");
    std::fs::write(
        &notify_script,
        r#"#!/bin/bash
set -e
echo "${@: -1}" >> $(dirname "${0}")/notify.ndjson"#,
    )?;
    std::fs::set_permissions(&notify_script, std::fs::Permissions::from_mode(0o755))?;

    let notify_file = notify_dir.path().join("notify.ndjson");
    let notify_script_str = notify_script.to_str().unwrap().to_string();
    let rmcp_test_server_bin = stdio_server_bin()?;

    let fixture = test_codex()
        .with_pre_build_hook(|codex_home| {
            let user_memory_root = codex_home.join("memories").join("user").join("memory");
            std::fs::create_dir_all(&user_memory_root).expect("create user memory root");
            std::fs::write(
                user_memory_root.join("memory_summary.md"),
                "first user summary",
            )
            .expect("write user memory summary");
        })
        .with_config(move |config| {
            config.features.enable(Feature::MemoryTool);
            config.features.disable(Feature::SearchTool);
            config.notify = Some(vec![notify_script_str.clone()]);

            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                server_name.to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("test mcp servers should accept any configuration");
        })
        .build(&server)
        .await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp echo tool first",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    let user_memory_summary_path = fixture
        .codex_home_path()
        .join("memories")
        .join("user")
        .join("memory")
        .join("memory_summary.md");
    tokio::fs::write(&user_memory_summary_path, "second user summary").await?;

    fixture
        .submit_turn_with_policies(
            "call the rmcp echo tool second",
            AskForApproval::Never,
            SandboxPolicy::new_read_only_policy(),
        )
        .await?;

    fs_wait::wait_for_path_exists(&notify_file, Duration::from_secs(5)).await?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        let raw = tokio::fs::read_to_string(&notify_file).await?;
        let payloads: Vec<Value> = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()?;

        let mcp_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("mcp-tool-call-complete"))
            .collect::<Vec<_>>();
        let agent_payloads = payloads
            .iter()
            .filter(|payload| payload["type"] == json!("agent-turn-complete"))
            .collect::<Vec<_>>();

        if mcp_payloads.len() >= 2 && agent_payloads.len() >= 2 {
            let turn_memory_versions = agent_payloads
                .iter()
                .filter_map(|payload| {
                    let turn_id = payload["turn-id"].as_str()?;
                    let memory_version =
                        payload["memory-context"]["active-memory-scope-version"].as_str()?;
                    Some((turn_id, memory_version))
                })
                .collect::<HashMap<_, _>>();
            assert_eq!(turn_memory_versions.len(), agent_payloads.len());

            let mut mcp_version_by_call = HashMap::new();
            for payload in mcp_payloads {
                let call_id = payload["call-id"]
                    .as_str()
                    .expect("mcp payload should include call-id");
                let turn_id = payload["turn-id"]
                    .as_str()
                    .expect("mcp payload should include turn-id");
                let memory_version = payload["memory-context"]["active-memory-scope-version"]
                    .as_str()
                    .expect("mcp payload should include memory scope version");
                let expected_memory_version = turn_memory_versions
                    .get(turn_id)
                    .expect("agent payload should exist for mcp payload turn");
                assert_eq!(memory_version, *expected_memory_version);
                assert!(memory_version.starts_with("user:"));
                mcp_version_by_call.insert(call_id.to_string(), memory_version.to_string());
            }

            assert_eq!(mcp_version_by_call.len(), 2);
            let first_memory_version = mcp_version_by_call
                .get(first_call_id)
                .expect("first call should have memory version");
            let second_memory_version = mcp_version_by_call
                .get(second_call_id)
                .expect("second call should have memory version");
            assert_ne!(first_memory_version, second_memory_version);
            return Ok(());
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for memory-version payloads: {payloads:?}");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn submit_mcp_approval_turn(
    fixture: &TestCodex,
    prompt: &str,
    choice: &str,
) -> anyhow::Result<()> {
    let session_model = fixture.session_configured.model.clone();
    fixture
        .codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            cwd: fixture.cwd.path().to_path_buf(),
            approval_policy: AskForApproval::OnRequest,
            sandbox_policy: SandboxPolicy::new_read_only_policy(),
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
            collaboration_mode: None,
            personality: None,
        })
        .await?;

    let mut saw_request = false;
    loop {
        let event = wait_for_event(&fixture.codex, |event| {
            matches!(
                event,
                EventMsg::RequestUserInput(_) | EventMsg::TurnComplete(_)
            )
        })
        .await;
        match event {
            EventMsg::RequestUserInput(request) => {
                if saw_request {
                    anyhow::bail!("expected at most one approval prompt per turn");
                }
                saw_request = true;
                let Some(question) = request.questions.first() else {
                    anyhow::bail!("approval question should exist");
                };
                let question_id = question.id.clone();
                let mut answers = HashMap::new();
                answers.insert(
                    question_id,
                    RequestUserInputAnswer {
                        answers: vec![choice.to_string()],
                    },
                );
                fixture
                    .codex
                    .submit(Op::UserInputAnswer {
                        id: request.turn_id,
                        response: RequestUserInputResponse { answers },
                    })
                    .await?;
            }
            EventMsg::TurnComplete(_) => {
                if !saw_request {
                    anyhow::bail!("expected approval prompt, but turn completed directly");
                }
                break;
            }
            _ => unreachable!("wait_for_event predicate filters event variants"),
        }
    }
    Ok(())
}
