use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use app_test_support::McpProcess;
use app_test_support::to_response;
use app_test_support::write_models_cache;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_app_server_protocol::RequestId;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const INVALID_REQUEST_ERROR_CODE: i64 = -32600;

#[tokio::test]
async fn list_models_returns_all_models_with_large_limit() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_models_cache(codex_home.path())?;
    let mut mcp = McpProcess::new(codex_home.path()).await?;

    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(100),
            cursor: None,
        })
        .await?;

    let response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;

    let ModelListResponse {
        data: items,
        next_cursor,
    } = to_response::<ModelListResponse>(response)?;

    let expected_models = vec![
        Model {
            id: "gpt-5.3-codex".to_string(),
            model: "gpt-5.3-codex".to_string(),
            upgrade: None,
            display_name: "gpt-5.3-codex".to_string(),
            description: "Latest frontier agentic coding model with enhanced reasoning.".to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: true,
        },
        Model {
            id: "gpt-5.2-codex".to_string(),
            model: "gpt-5.2-codex".to_string(),
            upgrade: Some("gpt-5.3-codex".to_string()),
            display_name: "gpt-5.2-codex".to_string(),
            description: "Frontier agentic coding model.".to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gpt-5.2".to_string(),
            model: "gpt-5.2".to_string(),
            upgrade: Some("gpt-5.2-codex".to_string()),
            display_name: "gpt-5.2".to_string(),
            description:
                "Latest frontier model with improvements across knowledge, reasoning and coding"
                    .to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Balances speed with some reasoning; useful for straightforward \
                                   queries and short explanations"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Provides a solid balance of reasoning depth and latency for \
                         general-purpose tasks"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gemini-3-pro-preview".to_string(),
            model: "gemini-3-pro-preview".to_string(),
            upgrade: None,
            display_name: "Gemini 3 Pro".to_string(),
            description: "Google Gemini 3 Pro with deep reasoning and 1M context.".to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::High,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gemini-3-flash-preview".to_string(),
            model: "gemini-3-flash-preview".to_string(),
            upgrade: None,
            display_name: "Gemini 3 Flash".to_string(),
            description: "Google Gemini 3 Flash — fast and efficient with 1M context.".to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks"
                        .to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gemini-3-pro-image-preview".to_string(),
            model: "gemini-3-pro-image-preview".to_string(),
            upgrade: None,
            display_name: "Gemini 3 Pro Image".to_string(),
            description: "Gemini 3 Pro for text, image understanding, and image generation."
                .to_string(),
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description:
                        "Default Gemini reasoning behaviour for image workflows.".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
    ];

    assert_eq!(items, expected_models);
    assert!(next_cursor.is_none());
    Ok(())
}

#[tokio::test]
async fn list_models_pagination_works() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_models_cache(codex_home.path())?;
    let mut mcp = McpProcess::new(codex_home.path()).await?;

    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let first_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: None,
        })
        .await?;

    let first_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(first_request)),
    )
    .await??;

    let ModelListResponse {
        data: first_items,
        next_cursor: first_cursor,
    } = to_response::<ModelListResponse>(first_response)?;

    assert_eq!(first_items.len(), 1);
    assert_eq!(first_items[0].id, "gpt-5.3-codex");
    let next_cursor = first_cursor.ok_or_else(|| anyhow!("cursor for second page"))?;

    let second_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: Some(next_cursor.clone()),
        })
        .await?;

    let second_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(second_request)),
    )
    .await??;

    let ModelListResponse {
        data: second_items,
        next_cursor: second_cursor,
    } = to_response::<ModelListResponse>(second_response)?;

    assert_eq!(second_items.len(), 1);
    assert_eq!(second_items[0].id, "gpt-5.2-codex");
    let third_cursor = second_cursor.ok_or_else(|| anyhow!("cursor for third page"))?;

    let third_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: Some(third_cursor.clone()),
        })
        .await?;

    let third_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(third_request)),
    )
    .await??;

    let ModelListResponse {
        data: third_items,
        next_cursor: third_cursor,
    } = to_response::<ModelListResponse>(third_response)?;

    assert_eq!(third_items.len(), 1);
    assert_eq!(third_items[0].id, "gpt-5.2");
    let fourth_cursor = third_cursor.ok_or_else(|| anyhow!("cursor for fourth page"))?;

    let fourth_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: Some(fourth_cursor.clone()),
        })
        .await?;

    let fourth_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(fourth_request)),
    )
    .await??;

    let ModelListResponse {
        data: fourth_items,
        next_cursor: fourth_cursor,
    } = to_response::<ModelListResponse>(fourth_response)?;

    assert_eq!(fourth_items.len(), 1);
    assert_eq!(fourth_items[0].id, "gemini-3-pro-preview");
    let fifth_cursor = fourth_cursor.ok_or_else(|| anyhow!("cursor for fifth page"))?;

    let fifth_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: Some(fifth_cursor.clone()),
        })
        .await?;

    let fifth_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(fifth_request)),
    )
    .await??;

    let ModelListResponse {
        data: fifth_items,
        next_cursor: fifth_cursor,
    } = to_response::<ModelListResponse>(fifth_response)?;

    assert_eq!(fifth_items.len(), 1);
    assert_eq!(fifth_items[0].id, "gemini-3-flash-preview");
    let sixth_cursor = fifth_cursor.ok_or_else(|| anyhow!("cursor for sixth page"))?;

    let sixth_request = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(1),
            cursor: Some(sixth_cursor.clone()),
        })
        .await?;

    let sixth_response: JSONRPCResponse = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(sixth_request)),
    )
    .await??;

    let ModelListResponse {
        data: sixth_items,
        next_cursor: sixth_cursor,
    } = to_response::<ModelListResponse>(sixth_response)?;

    assert_eq!(sixth_items.len(), 1);
    assert_eq!(sixth_items[0].id, "gemini-3-pro-image-preview");
    assert!(sixth_cursor.is_none());
    Ok(())
}

#[tokio::test]
async fn list_models_rejects_invalid_cursor() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_models_cache(codex_home.path())?;
    let mut mcp = McpProcess::new(codex_home.path()).await?;

    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_list_models_request(ModelListParams {
            limit: None,
            cursor: Some("invalid".to_string()),
        })
        .await?;

    let error: JSONRPCError = timeout(
        DEFAULT_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    assert_eq!(error.id, RequestId::Integer(request_id));
    assert_eq!(error.error.code, INVALID_REQUEST_ERROR_CODE);
    assert_eq!(error.error.message, "invalid cursor: invalid");
    Ok(())
}
