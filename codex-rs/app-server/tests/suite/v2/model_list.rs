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
            include_hidden: None,
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
            description: "Latest frontier agentic coding model with enhanced reasoning."
                .to_string(),
            hidden: false,
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
            id: "gpt-5.3-codex-spark|[pro]".to_string(),
            model: "gpt-5.3-codex-spark|[pro]".to_string(),
            upgrade: None,
            display_name: "gpt-5.3-codex-spark|[pro]".to_string(),
            description:
                "Realtime coding model optimized for low-latency edits (text only, Pro, 128K context)."
                    .to_string(),
            hidden: false,
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
            input_modalities: vec![InputModality::Text],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gpt-5.1-codex-max".to_string(),
            model: "gpt-5.1-codex-max".to_string(),
            upgrade: Some("gpt-5.3-codex".to_string()),
            display_name: "gpt-5.1-codex-max".to_string(),
            description: "Codex-optimized flagship for deep and fast reasoning.".to_string(),
            hidden: false,
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
            id: "gpt-5.2-codex".to_string(),
            model: "gpt-5.2-codex".to_string(),
            upgrade: Some("gpt-5.3-codex".to_string()),
            display_name: "gpt-5.2-codex".to_string(),
            description: "Frontier agentic coding model.".to_string(),
            hidden: false,
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
            hidden: false,
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
            hidden: false,
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
            hidden: false,
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
            hidden: false,
            supported_reasoning_efforts: vec![ReasoningEffortOption {
                reasoning_effort: ReasoningEffort::Medium,
                description: "Default Gemini reasoning behaviour for image workflows.".to_string(),
            }],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gemma-3n".to_string(),
            model: "gemma-3n".to_string(),
            upgrade: None,
            display_name: "Gemma 3n (Local)".to_string(),
            description: "Local Gemma 3n served via Gemini-compatible API.".to_string(),
            hidden: false,
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
            id: "grok-4-latest".to_string(),
            model: "grok-4-latest".to_string(),
            upgrade: None,
            display_name: "Grok 4 Latest".to_string(),
            description: "xAI Grok 4 via OpenAI-compatible Responses API (256K context)."
                .to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![ReasoningEffortOption {
                reasoning_effort: ReasoningEffort::None,
                description: "Reasoning effort is not configurable on this model.".to_string(),
            }],
            default_reasoning_effort: ReasoningEffort::None,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "grok-4-1-fast-reasoning".to_string(),
            model: "grok-4-1-fast-reasoning".to_string(),
            upgrade: None,
            display_name: "Grok 4.1 Fast Reasoning".to_string(),
            description: "xAI Grok 4.1 fast reasoning via OpenAI-compatible Responses API (2M context)."
                .to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![ReasoningEffortOption {
                reasoning_effort: ReasoningEffort::None,
                description: "Reasoning effort is not configurable on this model.".to_string(),
            }],
            default_reasoning_effort: ReasoningEffort::None,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "claude-opus-4-6".to_string(),
            model: "claude-opus-4-6".to_string(),
            upgrade: None,
            display_name: "Claude Opus 4.6".to_string(),
            description: "Anthropic Claude Opus 4.6 — deep reasoning with 1M context."
                .to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balanced reasoning for general tasks".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Deep reasoning for complex problems".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::XHigh,
                    description: "Maximum reasoning depth with extended thinking".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::High,
            input_modalities: vec![InputModality::Text],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "gpt-5.1-codex-mini".to_string(),
            model: "gpt-5.1-codex-mini".to_string(),
            upgrade: Some("gpt-5.3-codex".to_string()),
            display_name: "gpt-5.1-codex-mini".to_string(),
            description: "Optimized for codex. Cheaper, faster, but less capable.".to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Dynamically adjusts reasoning based on the task".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems"
                        .to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text, InputModality::Image],
            supports_personality: false,
            is_default: false,
        },
        Model {
            id: "claude-sonnet-4-6".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            upgrade: None,
            display_name: "Claude Sonnet 4.6".to_string(),
            description: "Anthropic Claude Sonnet 4.6 — fast execution with 1M context."
                .to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
                ReasoningEffortOption {
                    reasoning_effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: vec![InputModality::Text],
            supports_personality: false,
            is_default: false,
        },
    ];

    assert_eq!(items, expected_models);
    assert!(next_cursor.is_none());
    Ok(())
}

#[tokio::test]
async fn list_models_includes_hidden_models() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_models_cache(codex_home.path())?;
    let mut mcp = McpProcess::new(codex_home.path()).await?;

    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_list_models_request(ModelListParams {
            limit: Some(100),
            cursor: None,
            include_hidden: Some(true),
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

    assert!(items.iter().any(|item| item.hidden));
    assert!(next_cursor.is_none());
    Ok(())
}

#[tokio::test]
async fn list_models_pagination_works() -> Result<()> {
    let codex_home = TempDir::new()?;
    write_models_cache(codex_home.path())?;
    let mut mcp = McpProcess::new(codex_home.path()).await?;

    timeout(DEFAULT_TIMEOUT, mcp.initialize()).await??;

    let expected_ids = vec![
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark|[pro]",
        "gpt-5.1-codex-max",
        "gpt-5.2-codex",
        "gpt-5.2",
        "gemini-3-pro-preview",
        "gemini-3-flash-preview",
        "gemini-3-pro-image-preview",
        "gemma-3n",
        "grok-4-latest",
        "grok-4-1-fast-reasoning",
        "claude-opus-4-6",
        "gpt-5.1-codex-mini",
        "claude-sonnet-4-6",
    ];

    let mut cursor: Option<String> = None;
    for (index, expected_id) in expected_ids.iter().enumerate() {
        let request_id = mcp
            .send_list_models_request(ModelListParams {
                limit: Some(1),
                cursor: cursor.clone(),
                include_hidden: None,
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

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, *expected_id);
        if index + 1 == expected_ids.len() {
            assert!(next_cursor.is_none());
        } else {
            let next_page = index + 2;
            cursor = Some(next_cursor.ok_or_else(|| anyhow!("cursor for page {next_page}"))?);
        }
    }

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
            include_hidden: None,
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
