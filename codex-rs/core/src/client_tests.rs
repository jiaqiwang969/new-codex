use super::AuthRequestTelemetryContext;
use super::ModelClient;
use super::PendingUnauthorizedRetry;
use super::UnauthorizedRecoveryExecution;
use super::anthropic_thinking_enabled_with_env;
use super::build_reasoning_payload;
use super::sanitize_reasoning_effort_for_model;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::HashMap;

fn test_model_client(session_source: SessionSource) -> ModelClient {
    let provider = crate::model_provider_info::create_oss_provider_with_base_url(
        "https://example.com/v1",
        crate::model_provider_info::WireApi::Responses,
    );
    ModelClient::new(
        None,
        ThreadId::new(),
        provider,
        session_source,
        None,
        false,
        false,
        None,
    )
}

fn test_model_info() -> ModelInfo {
    serde_json::from_value(json!({
        "slug": "gpt-test",
        "display_name": "gpt-test",
        "description": "desc",
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [
            {"effort": "medium", "description": "medium"}
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "upgrade": null,
        "base_instructions": "base instructions",
        "model_messages": null,
        "supports_reasoning_summaries": false,
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "supports_parallel_tool_calls": false,
        "supports_image_detail_original": false,
        "context_window": 272000,
        "auto_compact_token_limit": null,
        "experimental_supported_tools": []
    }))
    .expect("deserialize test model info")
}

fn test_session_telemetry() -> SessionTelemetry {
    SessionTelemetry::new(
        ThreadId::new(),
        "gpt-test",
        "gpt-test",
        None,
        None,
        None,
        "test-originator".to_string(),
        false,
        "test-terminal".to_string(),
        SessionSource::Cli,
    )
}

#[test]
fn build_subagent_headers_sets_other_subagent_label() {
    let client = test_model_client(SessionSource::SubAgent(SubAgentSource::Other(
        "memory_consolidation".to_string(),
    )));
    let headers = client.build_subagent_headers();
    let value = headers
        .get("x-openai-subagent")
        .and_then(|value| value.to_str().ok());
    assert_eq!(value, Some("memory_consolidation"));
}

#[tokio::test]
async fn summarize_memories_returns_empty_for_empty_input() {
    let client = test_model_client(SessionSource::Cli);
    let model_info = test_model_info();
    let session_telemetry = test_session_telemetry();

    let output = client
        .summarize_memories(Vec::new(), &model_info, None, &session_telemetry)
        .await
        .expect("empty summarize request should succeed");
    assert_eq!(output.len(), 0);
}

#[test]
fn auth_request_telemetry_context_tracks_attached_auth_and_retry_phase() {
    let auth_context = AuthRequestTelemetryContext::new(
        Some(crate::auth::AuthMode::Chatgpt),
        &crate::api_bridge::CoreAuthProvider::for_test(Some("access-token"), Some("workspace-123")),
        PendingUnauthorizedRetry::from_recovery(UnauthorizedRecoveryExecution {
            mode: "managed",
            phase: "refresh_token",
        }),
    );

    assert_eq!(auth_context.auth_mode, Some("Chatgpt"));
    assert!(auth_context.auth_header_attached);
    assert_eq!(auth_context.auth_header_name, Some("authorization"));
    assert!(auth_context.retry_after_unauthorized);
    assert_eq!(auth_context.recovery_mode, Some("managed"));
    assert_eq!(auth_context.recovery_phase, Some("refresh_token"));
}

#[test]
fn resolve_provider_api_key_uses_env_specific_mapping_first() {
    let mut provider = crate::model_provider_info::ModelProviderInfo::create_anthropic_provider();
    provider.env_key = Some("__CODEX_TEST_ANTIGRAVITY_KEY__".to_string());

    let auth = crate::auth::CodexAuth::from_api_key_and_env_keys_for_testing(
        "fallback-key",
        HashMap::from([(
            "__CODEX_TEST_ANTIGRAVITY_KEY__".to_string(),
            "mapped-key".to_string(),
        )]),
    );

    assert_eq!(
        crate::provider_auth::resolve_provider_api_key(&provider, Some(&auth)),
        Some("mapped-key".to_string())
    );
}

#[test]
fn resolve_provider_api_key_falls_back_to_primary_auth_key() {
    let mut provider = crate::model_provider_info::ModelProviderInfo::create_anthropic_provider();
    provider.env_key = Some("__CODEX_TEST_MISSING_KEY__".to_string());

    let auth = crate::auth::CodexAuth::from_api_key_and_env_keys_for_testing(
        "fallback-key",
        HashMap::new(),
    );

    assert_eq!(
        crate::provider_auth::resolve_provider_api_key(&provider, Some(&auth)),
        Some("fallback-key".to_string())
    );
}

#[test]
fn resolve_provider_api_key_uses_selected_account_env_key_after_provider_switch() {
    let mut provider = crate::model_provider_info::ModelProviderInfo::create_anthropic_provider();
    provider.account_pool = vec![
        crate::model_provider_info::ModelProviderAccount {
            base_url: Some("https://pool-primary.example".to_string()),
            env_key: Some("__CODEX_TEST_POOL_PRIMARY_KEY__".to_string()),
        },
        crate::model_provider_info::ModelProviderAccount {
            base_url: Some("https://pool-secondary.example".to_string()),
            env_key: Some("__CODEX_TEST_POOL_SECONDARY_KEY__".to_string()),
        },
    ];

    let selected_provider = provider.with_account(&provider.account_pool[1]);
    let auth = crate::auth::CodexAuth::from_api_key_and_env_keys_for_testing(
        "fallback-key",
        HashMap::from([
            (
                "__CODEX_TEST_POOL_PRIMARY_KEY__".to_string(),
                "primary-key".to_string(),
            ),
            (
                "__CODEX_TEST_POOL_SECONDARY_KEY__".to_string(),
                "secondary-key".to_string(),
            ),
        ]),
    );

    assert_eq!(
        crate::provider_auth::resolve_provider_api_key(&selected_provider, Some(&auth)),
        Some("secondary-key".to_string())
    );
}

#[test]
fn build_reasoning_payload_omits_summary_when_unsupported() {
    let reasoning = build_reasoning_payload(
        false,
        Some(ReasoningEffortConfig::High),
        ReasoningSummaryConfig::Detailed,
    );

    assert_eq!(
        reasoning,
        Some(codex_api::common::Reasoning {
            effort: Some(ReasoningEffortConfig::High),
            summary: None,
        })
    );
}

#[test]
fn sanitize_reasoning_effort_clamps_to_supported_level() {
    let model_info = test_model_info();

    let sanitized =
        sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::XHigh), &model_info);

    assert_eq!(sanitized, Some(ReasoningEffortConfig::Medium));
}

#[test]
fn sanitize_reasoning_effort_uses_lowest_supported_when_requested_too_low() {
    let mut model_info = test_model_info();
    model_info.supported_reasoning_levels = vec![
        ReasoningEffortPreset {
            effort: ReasoningEffortConfig::Medium,
            description: "medium".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortConfig::High,
            description: "high".to_string(),
        },
    ];

    let sanitized =
        sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::Low), &model_info);

    assert_eq!(sanitized, Some(ReasoningEffortConfig::Medium));
}

#[test]
fn sanitize_reasoning_effort_omits_unsupported_models() {
    let mut model_info = test_model_info();
    model_info.slug = "grok-4-latest".to_string();

    let sanitized =
        sanitize_reasoning_effort_for_model(Some(ReasoningEffortConfig::High), &model_info);

    assert_eq!(sanitized, None);
}

#[test]
fn supports_memory_trace_summarize_requires_responses_wire_api() {
    let responses_provider = crate::model_provider_info::create_oss_provider_with_base_url(
        "https://example.com/v1",
        crate::model_provider_info::WireApi::Responses,
    );
    let gemini_provider = crate::model_provider_info::create_oss_provider_with_base_url(
        "https://example.com/v1",
        crate::model_provider_info::WireApi::Gemini,
    );
    let anthropic_provider =
        crate::model_provider_info::ModelProviderInfo::create_anthropic_provider();

    assert!(super::supports_memory_trace_summarize(
        &responses_provider,
        "gpt-5-codex"
    ));
    assert!(!super::supports_memory_trace_summarize(
        &gemini_provider,
        "gpt-5-codex"
    ));
    assert!(!super::supports_memory_trace_summarize(
        &anthropic_provider,
        "claude-opus-4-6"
    ));
}

#[test]
fn anthropic_thinking_follows_reasoning_effort_when_env_is_unset() {
    assert!(!anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::None),
        false
    ));
    assert!(!anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::Minimal),
        false
    ));
    assert!(!anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::Low),
        false
    ));
    assert!(anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::Medium),
        false
    ));
    assert!(anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::High),
        false
    ));
    assert!(anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::XHigh),
        false
    ));
}

#[test]
fn anthropic_thinking_env_override_enables_thinking_without_effort() {
    assert!(anthropic_thinking_enabled_with_env(None, true));
    assert!(anthropic_thinking_enabled_with_env(
        Some(ReasoningEffortConfig::Low),
        true
    ));
}
