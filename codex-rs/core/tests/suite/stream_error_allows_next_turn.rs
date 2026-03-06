use codex_core::ModelProviderAccount;
use codex_core::ModelProviderInfo;
use codex_core::WireApi;
use codex_core::config::CONFIG_POOL_TOML_FILE;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_string_contains;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotates_to_next_pool_account_after_429() {
    skip_if_no_network!();

    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    let fail = ResponseTemplate::new(429)
        .insert_header("content-type", "application/json")
        .set_body_string(
            serde_json::json!({
                "error": {"type": "rate_limit", "message": "synthetic rate limit"}
            })
            .to_string(),
        );

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("rotate me"))
        .respond_with(fail)
        .expect(1)
        .mount(&primary)
        .await;

    let ok = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(
            sse(vec![
                ev_response_created("resp_rotated"),
                ev_completed("resp_rotated"),
            ]),
            "text/event-stream",
        );

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("rotate me"))
        .respond_with(ok)
        .expect(1)
        .mount(&secondary)
        .await;

    let provider = ModelProviderInfo {
        name: "mock-openai".into(),
        base_url: Some(format!("{}/v1", primary.uri())),
        env_key: Some("PATH".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        stream_idle_timeout_ms: Some(2_000),
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: vec![
            ModelProviderAccount {
                base_url: Some(format!("{}/v1", primary.uri())),
                env_key: Some("PATH".into()),
            },
            ModelProviderAccount {
                base_url: Some(format!("{}/v1", secondary.uri())),
                env_key: Some("PATH".into()),
            },
        ],
    };

    let test = test_codex()
        .with_config(move |config| {
            config.base_instructions = Some("You are a helpful assistant".to_string());
            config.model_provider = provider;
        })
        .build(&primary)
        .await
        .unwrap();
    let codex = test.codex.clone();

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "rotate me".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();

    let EventMsg::BackgroundEvent(event) =
        wait_for_event(&codex, |ev| matches!(ev, EventMsg::BackgroundEvent(_))).await
    else {
        panic!("expected background event after account rotation");
    };
    assert!(
        event.message.contains("Switched provider account"),
        "background event should mention account rotation: {}",
        event.message
    );

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let persisted = std::fs::read_to_string(test.codex_home_path().join(CONFIG_POOL_TOML_FILE))
        .expect("read config-pool.toml");
    let persisted: toml::Value = toml::from_str(&persisted).expect("parse config-pool.toml");
    let provider = persisted
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .and_then(|providers| providers.get("openai"))
        .and_then(toml::Value::as_table)
        .expect("persisted provider entry");
    assert_eq!(
        provider.get("base_url").and_then(toml::Value::as_str),
        Some(format!("{}/v1", secondary.uri()).as_str())
    );
    assert_eq!(
        provider.get("env_key").and_then(toml::Value::as_str),
        Some("PATH")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn continue_after_stream_error() {
    skip_if_no_network!();

    let server = MockServer::start().await;

    let fail = ResponseTemplate::new(500)
        .insert_header("content-type", "application/json")
        .set_body_string(
            serde_json::json!({
                "error": {"type": "bad_request", "message": "synthetic client error"}
            })
            .to_string(),
        );

    // The provider below disables request retries (request_max_retries = 0),
    // so the failing request should only occur once.
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("first message"))
        .respond_with(fail)
        .up_to_n_times(2)
        .mount(&server)
        .await;

    let ok = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(
            sse(vec![
                ev_response_created("resp_ok2"),
                ev_completed("resp_ok2"),
            ]),
            "text/event-stream",
        );

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("follow up"))
        .respond_with(ok)
        .expect(1)
        .mount(&server)
        .await;

    // Configure a provider that uses the Responses API and points at our mock
    // server. Use an existing env var (PATH) to satisfy the auth plumbing
    // without requiring a real secret.
    let provider = ModelProviderInfo {
        name: "mock-openai".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        env_key: Some("PATH".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(1),
        stream_max_retries: Some(1),
        stream_idle_timeout_ms: Some(2_000),
        requires_openai_auth: false,
        supports_websockets: false,
        account_pool: Vec::new(),
    };

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |config| {
            config.base_instructions = Some("You are a helpful assistant".to_string());
            config.model_provider = provider;
        })
        .build(&server)
        .await
        .unwrap();

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "first message".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();

    // Expect an Error followed by TurnComplete so the session is released.
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::Error(_))).await;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // 2) Second turn: now send another prompt that should succeed using the
    // mock server SSE stream. If the agent failed to clear the running task on
    // error above, this submission would be rejected/queued indefinitely.
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "follow up".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}
