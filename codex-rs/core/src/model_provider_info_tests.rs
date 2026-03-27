use super::*;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn test_deserialize_ollama_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Ollama"
base_url = "http://localhost:11434/v1"
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Ollama".into(),
        base_url: Some("http://localhost:11434/v1".into()),
        env_key: None,
        env_key_instructions: None,
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
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_azure_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Azure"
base_url = "https://xxxxx.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
query_params = { api-version = "2025-04-01-preview" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Azure".into(),
        base_url: Some("https://xxxxx.openai.azure.com/openai".into()),
        env_key: Some("AZURE_OPENAI_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: Some(maplit::hashmap! {
            "api-version".to_string() => "2025-04-01-preview".to_string(),
        }),
        http_headers: None,
        env_http_headers: None,
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_example_model_provider_toml() {
    let azure_provider_toml = r#"
name = "Example"
base_url = "https://example.com"
env_key = "API_KEY"
http_headers = { "X-Example-Header" = "example-value" }
env_http_headers = { "X-Example-Env-Header" = "EXAMPLE_ENV_VAR" }
        "#;
    let expected_provider = ModelProviderInfo {
        name: "Example".into(),
        base_url: Some("https://example.com".into()),
        env_key: Some("API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: Some(maplit::hashmap! {
            "X-Example-Header".to_string() => "example-value".to_string(),
        }),
        env_http_headers: Some(maplit::hashmap! {
            "X-Example-Env-Header".to_string() => "EXAMPLE_ENV_VAR".to_string(),
        }),
        request_max_retries: None,
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    };

    let provider: ModelProviderInfo = toml::from_str(azure_provider_toml).unwrap();
    assert_eq!(expected_provider, provider);
}

#[test]
fn test_deserialize_chat_wire_api_shows_helpful_error() {
    let provider_toml = r#"
name = "OpenAI using Chat Completions"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "chat"
        "#;

    let err = toml::from_str::<ModelProviderInfo>(provider_toml).unwrap_err();
    assert!(err.to_string().contains(CHAT_WIRE_API_REMOVED_ERROR));
}

#[test]
fn test_deserialize_websocket_connect_timeout() {
    let provider_toml = r#"
name = "OpenAI"
base_url = "https://api.openai.com/v1"
websocket_connect_timeout_ms = 15000
supports_websockets = true
        "#;

    let provider: ModelProviderInfo = toml::from_str(provider_toml).unwrap();
    assert_eq!(provider.websocket_connect_timeout_ms, Some(15_000));
}

#[test]
fn built_in_model_providers_include_grok() {
    let providers = built_in_model_providers(/* openai_base_url */ None);
    let grok = providers
        .get(GROK_PROVIDER_ID)
        .expect("built-in providers should include grok");
    let expected_base_url = std::env::var("XAI_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://api.x.ai/v1".to_string());

    assert_eq!(grok.name, GROK_PROVIDER_NAME);
    assert_eq!(grok.env_key.as_deref(), Some("XAI_API_KEY"));
    assert_eq!(grok.base_url.as_deref(), Some(expected_base_url.as_str()));
    assert_eq!(grok.wire_api, WireApi::Responses);
    assert!(!grok.requires_openai_auth);
    assert!(!grok.supports_websockets);
}

#[test]
fn built_in_model_providers_include_gemma() {
    let providers = built_in_model_providers(/* openai_base_url */ None);
    let gemma = providers
        .get(GEMMA_PROVIDER_ID)
        .expect("built-in providers should include gemma");
    let expected_base_url = std::env::var("GEMMA_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:5001/v1beta".to_string());

    assert_eq!(gemma.name, GEMMA_PROVIDER_NAME);
    assert_eq!(gemma.base_url.as_deref(), Some(expected_base_url.as_str()));
    assert_eq!(gemma.wire_api, WireApi::Gemini);
    assert_eq!(gemma.env_key, None);
    assert!(!gemma.requires_openai_auth);
    assert!(!gemma.supports_websockets);
}

#[test]
fn built_in_model_providers_include_anthropic() {
    let providers = built_in_model_providers(/* openai_base_url */ None);
    let anthropic = providers
        .get(ANTHROPIC_PROVIDER_ID)
        .expect("built-in providers should include anthropic");
    let expected_base_url = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());

    assert_eq!(anthropic.name, ANTHROPIC_PROVIDER_NAME);
    assert_eq!(anthropic.env_key.as_deref(), Some("ANTHROPIC_API_KEY"));
    assert_eq!(
        anthropic.base_url.as_deref(),
        Some(expected_base_url.as_str())
    );
    assert_eq!(anthropic.wire_api, WireApi::Anthropic);
    assert!(!anthropic.requires_openai_auth);
    assert!(!anthropic.supports_websockets);
}

#[test]
fn builtin_family_override_retargets_endpoint_without_losing_defaults() {
    let built_in = ModelProviderInfo::create_anthropic_provider();
    let override_provider = ModelProviderInfo {
        name: "Anthropic Proxy".into(),
        base_url: Some("https://code.ppchat.vip".into()),
        env_key: Some("ANTHROPIC_PROXY_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: Some(HashMap::from([("routing".to_string(), "pool".to_string())])),
        http_headers: Some(HashMap::from([(
            "x-custom-header".to_string(),
            "enabled".to_string(),
        )])),
        env_http_headers: None,
        request_max_retries: Some(9),
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: true,
        supports_websockets: true,
        account_pool: Vec::new(),
    };

    let merged = built_in.with_builtin_family_override(&override_provider);

    assert_eq!(merged.name, built_in.name);
    assert_eq!(merged.base_url, override_provider.base_url);
    assert_eq!(merged.env_key, override_provider.env_key);
    assert_eq!(merged.env_key_instructions, None);
    assert_eq!(merged.wire_api, built_in.wire_api);
    assert_eq!(merged.query_params, override_provider.query_params);
    assert_eq!(
        merged.http_headers,
        Some(HashMap::from([
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ("x-custom-header".to_string(), "enabled".to_string()),
        ]))
    );
    assert_eq!(merged.request_max_retries, Some(9));
    assert_eq!(merged.stream_max_retries, built_in.stream_max_retries);
    assert_eq!(merged.requires_openai_auth, built_in.requires_openai_auth);
    assert_eq!(merged.supports_websockets, built_in.supports_websockets);
}

#[test]
fn builtin_family_override_keeps_logical_identity_when_pool_present() {
    let built_in = ModelProviderInfo::create_anthropic_provider();
    let override_provider = ModelProviderInfo {
        name: "Anthropic Proxy".into(),
        base_url: Some("https://code.ppchat.vip".into()),
        env_key: Some("ANTHROPIC_PROXY_API_KEY".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: Some(HashMap::from([(
            "x-custom-header".to_string(),
            "enabled".to_string(),
        )])),
        env_http_headers: None,
        request_max_retries: Some(9),
        stream_max_retries: None,
        stream_idle_timeout_ms: None,
        websocket_connect_timeout_ms: None,
        requires_openai_auth: true,
        supports_websockets: true,
        account_pool: vec![ModelProviderAccount {
            base_url: Some("https://code.ppchat.vip".into()),
            env_key: Some("ANTHROPIC_POOL_1".into()),
        }],
    };

    let merged = built_in.with_builtin_family_override(&override_provider);

    assert_eq!(merged.name, built_in.name);
    assert_eq!(merged.base_url, built_in.base_url);
    assert_eq!(merged.env_key, built_in.env_key);
    assert_eq!(merged.env_key_instructions, built_in.env_key_instructions);
    assert_eq!(merged.wire_api, built_in.wire_api);
    assert_eq!(merged.account_pool, override_provider.account_pool);
    assert_eq!(
        merged.http_headers,
        Some(HashMap::from([
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ("x-custom-header".to_string(), "enabled".to_string()),
        ]))
    );
    assert_eq!(merged.request_max_retries, Some(9));
    assert_eq!(merged.requires_openai_auth, built_in.requires_openai_auth);
    assert_eq!(merged.supports_websockets, built_in.supports_websockets);
}
