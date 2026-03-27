use crate::model_provider_info::ANTHROPIC_PROVIDER_ID;
use crate::model_provider_info::ANTIGRAVITY_ANTHROPIC_PROVIDER_ID;
use crate::model_provider_info::ANTIGRAVITY_GEMINI_PROVIDER_ID;
use crate::model_provider_info::DEFAULT_LMSTUDIO_PORT;
use crate::model_provider_info::DEFAULT_OLLAMA_PORT;
use crate::model_provider_info::GEMINI_PROVIDER_ID;
use crate::model_provider_info::GEMMA_PROVIDER_ID;
use crate::model_provider_info::GROK_PROVIDER_ID;
use crate::model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::OLLAMA_OSS_PROVIDER_ID;
use crate::model_provider_info::OPENAI_PROVIDER_ID;
use crate::model_provider_info::WireApi;
use crate::model_provider_info::create_oss_provider;
use std::collections::HashMap;

pub(crate) fn create_gemini_provider() -> ModelProviderInfo {
    let base_url = std::env::var("GEMINI_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string());

    ModelProviderInfo {
        name: "Gemini".into(),
        base_url: Some(base_url),
        env_key: Some("GEMINI_API_KEY".into()),
        env_key_instructions: Some(
            "Get a Gemini API key at https://aistudio.google.com/apikey and set GEMINI_API_KEY."
                .into(),
        ),
        experimental_bearer_token: None,
        wire_api: WireApi::Gemini,
        query_params: None,
        http_headers: None,
        env_http_headers: Some(
            [
                ("X-Goog-Api-Key".to_string(), "GEMINI_API_KEY".to_string()),
                ("Cookie".to_string(), "GEMINI_COOKIE".to_string()),
            ]
            .into_iter()
            .collect(),
        ),
        request_max_retries: Some(3),
        stream_max_retries: Some(3),
        stream_idle_timeout_ms: Some(300_000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn create_grok_provider() -> ModelProviderInfo {
    let base_url = std::env::var("XAI_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://api.x.ai/v1".to_string());

    ModelProviderInfo {
        name: "Grok".into(),
        base_url: Some(base_url),
        env_key: Some("XAI_API_KEY".into()),
        env_key_instructions: Some(
            "Get an xAI API key at https://console.x.ai and set XAI_API_KEY.".into(),
        ),
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn create_antigravity_gemini_provider() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Antigravity Gemini".into(),
        base_url: None,
        env_key: Some("ANTIGRAVITY_API_KEY".into()),
        env_key_instructions: Some("Set ANTIGRAVITY_API_KEY to your CLIProxyAPI key.".into()),
        experimental_bearer_token: None,
        wire_api: WireApi::Gemini,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: true,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn create_antigravity_anthropic_provider() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Antigravity Anthropic".into(),
        base_url: None,
        env_key: Some("ANTIGRAVITY_API_KEY".into()),
        env_key_instructions: Some("Set ANTIGRAVITY_API_KEY to your CLIProxyAPI key.".into()),
        experimental_bearer_token: None,
        wire_api: WireApi::Anthropic,
        query_params: None,
        http_headers: Some(
            [("anthropic-version".to_string(), "2023-06-01".to_string())]
                .into_iter()
                .collect(),
        ),
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: true,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn create_anthropic_provider() -> ModelProviderInfo {
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());

    ModelProviderInfo {
        name: "Anthropic".into(),
        base_url: Some(base_url),
        env_key: Some("ANTHROPIC_API_KEY".into()),
        env_key_instructions: Some(
            "Get an Anthropic API key at https://console.anthropic.com and set ANTHROPIC_API_KEY."
                .into(),
        ),
        experimental_bearer_token: None,
        wire_api: WireApi::Anthropic,
        query_params: None,
        http_headers: Some(
            [("anthropic-version".to_string(), "2023-06-01".to_string())]
                .into_iter()
                .collect(),
        ),
        env_http_headers: None,
        request_max_retries: Some(3),
        stream_max_retries: Some(3),
        stream_idle_timeout_ms: Some(300_000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn create_gemma_provider() -> ModelProviderInfo {
    let base_url = std::env::var("GEMMA_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:5001/v1beta".to_string());

    ModelProviderInfo {
        name: "Gemma".into(),
        base_url: Some(base_url),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Gemini,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(3),
        stream_max_retries: Some(3),
        // Local Gemma stacks often have long first-token latency for large prompts.
        stream_idle_timeout_ms: Some(900_000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    }
}

pub(crate) fn built_in_model_providers(
    openai_base_url: Option<String>,
) -> HashMap<String, ModelProviderInfo> {
    use ModelProviderInfo as P;

    let openai_provider = P::create_openai_provider(openai_base_url);

    [
        (OPENAI_PROVIDER_ID, openai_provider),
        (GEMINI_PROVIDER_ID, create_gemini_provider()),
        (GEMMA_PROVIDER_ID, create_gemma_provider()),
        (GROK_PROVIDER_ID, create_grok_provider()),
        (ANTHROPIC_PROVIDER_ID, create_anthropic_provider()),
        (
            ANTIGRAVITY_GEMINI_PROVIDER_ID,
            create_antigravity_gemini_provider(),
        ),
        (
            ANTIGRAVITY_ANTHROPIC_PROVIDER_ID,
            create_antigravity_anthropic_provider(),
        ),
        (
            OLLAMA_OSS_PROVIDER_ID,
            create_oss_provider(DEFAULT_OLLAMA_PORT, WireApi::Responses),
        ),
        (
            LMSTUDIO_OSS_PROVIDER_ID,
            create_oss_provider(DEFAULT_LMSTUDIO_PORT, WireApi::Responses),
        ),
    ]
    .into_iter()
    .map(|(key, provider)| (key.to_string(), provider))
    .collect()
}
