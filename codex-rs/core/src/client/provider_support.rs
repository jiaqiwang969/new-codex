use super::*;
use std::borrow::Cow;
use std::time::Duration;

use crate::client_common::tools::ToolSpec;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::model_supports_data_url_input_images;
use crate::model_compat::model_supports_input_images;
use crate::model_compat::model_supports_memory_trace_summarize;
use crate::model_compat::model_supports_reasoning_effort;
use crate::model_compat::normalized_grok_model_slug;
use crate::provider_auth::resolve_gemini_api_key;
use crate::provider_auth::resolve_provider_api_key;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use reqwest::StatusCode;

impl ModelClientSession {
    /// Streams a turn via the Google Gemini `:streamGenerateContent` endpoint.
    pub(super) async fn stream_gemini(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
    ) -> Result<ResponseStream> {
        use crate::gemini_content::*;
        use crate::gemini_streaming::*;
        use crate::gemini_types::*;

        let provider = &self.client.state.provider;
        let base_url = provider.base_url.as_ref().ok_or_else(|| {
            CodexErr::UnsupportedOperation("Gemini providers must define a base_url".to_string())
        })?;
        let base_url = normalize_gemini_base_url(base_url);

        let api_model = strip_model_suffix(&model_info.slug);

        let url = format!(
            "{}/models/{api_model}:streamGenerateContent?alt=sse",
            base_url.as_ref().trim_end_matches('/'),
        );

        let instructions = prompt.base_instructions.text.clone();
        let formatted_input = prompt.get_formatted_input();
        let contents = build_gemini_contents(&formatted_input, &prompt.reference_images, api_model);
        if contents.is_empty() {
            return Err(CodexErr::UnsupportedOperation(
                "Gemini requests require at least one message".to_string(),
            ));
        }

        let system_instruction = (!instructions.trim().is_empty()).then(|| GeminiContentRequest {
            role: None,
            parts: vec![GeminiPartRequest {
                text: Some(instructions),
                inline_data: None,
                function_call: None,
                function_response: None,
                thought_signature: None,
                compat_thought_signature: None,
            }],
        });

        let tools = build_gemini_tools(&prompt.tools, api_model);
        let has_function_tools = prompt
            .tools
            .iter()
            .any(|tool| matches!(tool, ToolSpec::Function(_)));
        let tool_config = (tools.is_some() && has_function_tools).then(|| GeminiToolConfig {
            function_calling_config: build_gemini_tool_config(
                &prompt.tools,
                &formatted_input,
                api_model,
            ),
        });

        let contents = ensure_active_loop_has_thought_signatures(&contents);

        let reasoning_effort = effort.or(model_info.default_reasoning_level);
        let thinking_config = build_gemini_thinking_config(api_model, reasoning_effort);
        let max_output_tokens = resolve_gemini_max_output_tokens(&model_info.slug);

        let generation_config = Some(GeminiGenerationConfig {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            max_output_tokens,
            thinking_config,
            media_resolution: None,
            response_modalities: if api_model.contains("image") {
                Some(vec![
                    GeminiResponseModality::Text,
                    GeminiResponseModality::Image,
                ])
            } else {
                None
            },
            image_config: if api_model.contains("image") {
                Some(GeminiImageConfig {
                    image_size: prompt.image_size,
                    aspect_ratio: prompt.aspect_ratio,
                })
            } else {
                None
            },
        });

        let safety_settings = Some(default_safety_settings());

        let request = GeminiRequest {
            system_instruction,
            contents,
            tools,
            tool_config,
            generation_config,
            safety_settings,
        };

        if std::env::var("CODEX_DEBUG_GEMINI_REQUEST").is_ok()
            && let Ok(json) = serde_json::to_string_pretty(&request)
        {
            tracing::debug!("DEBUG GEMINI REQUEST:\n{json}");
        }

        let client = crate::default_client::build_reqwest_client();

        let auth = match self.client.state.auth_manager.as_ref() {
            Some(manager) => manager.auth().await,
            None => None,
        };

        let gemini_api_key = resolve_gemini_api_key(provider, auth.as_ref());

        let make_request_builder = || {
            let mut req_builder = client.post(&url);
            req_builder = provider.apply_http_headers(req_builder);
            if let Some(api_key) = gemini_api_key.as_deref() {
                req_builder = if provider.requires_openai_auth {
                    req_builder.bearer_auth(api_key)
                } else {
                    req_builder.header("x-goog-api-key", api_key)
                };
            }
            req_builder
        };

        const MAX_ATTEMPTS: u64 = 3;
        const INITIAL_DELAY_MS: u64 = 5000;
        const MAX_DELAY_MS: u64 = 30000;

        let mut attempt: u64 = 0;
        let mut current_delay = INITIAL_DELAY_MS;

        let response = loop {
            attempt += 1;
            let result = make_request_builder().json(&request).send().await;

            match result {
                Ok(resp) => break resp,
                Err(err) => {
                    let should_retry = if let Some(status) = err.status() {
                        status == StatusCode::TOO_MANY_REQUESTS
                            || (status.as_u16() >= 500 && status.as_u16() < 600)
                    } else {
                        err.is_connect() || err.is_timeout()
                    };

                    if should_retry && attempt < MAX_ATTEMPTS {
                        let jitter =
                            (current_delay as f64 * 0.3 * (rand::random::<f64>() * 2.0 - 1.0))
                                as u64;
                        let delay_with_jitter = current_delay.saturating_add(jitter);
                        tracing::debug!(
                            "Gemini request attempt {} failed, retrying after {}ms: {}",
                            attempt,
                            delay_with_jitter,
                            err
                        );
                        tokio::time::sleep(Duration::from_millis(delay_with_jitter)).await;
                        current_delay = std::cmp::min(MAX_DELAY_MS, current_delay * 2);
                        continue;
                    }

                    return Err(CodexErr::ResponseStreamFailed(
                        crate::error::ResponseStreamFailed {
                            source: err,
                            request_id: None,
                        },
                    ));
                }
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Graceful degradation for thought_signature validation errors.
            if (status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::BAD_REQUEST)
                && body.contains("missing a `thought_signature`")
            {
                let message = format!(
                    "Gemini backend rejected this request due to a thought_signature \
                     validation error.\n\nUpstream error:\n{}",
                    body.chars().take(2000).collect::<String>()
                );
                let item = ResponseItem::Message {
                    id: None,
                    role: "assistant".to_string(),
                    content: vec![codex_protocol::models::ContentItem::OutputText {
                        text: message,
                    }],
                    end_turn: None,
                    phase: None,
                    thought_signature: None,
                };
                return Ok(spawn_gemini_response_stream(
                    Some(item),
                    "gemini-error-thought-signature".to_string(),
                    /*token_usage*/ None,
                ));
            }

            return Err(CodexErr::UnexpectedStatus(
                crate::error::UnexpectedResponseError {
                    status,
                    body,
                    url: Some(url.clone()),
                    cf_ray: None,
                    request_id: None,
                    identity_authorization_error: None,
                    identity_error_code: None,
                },
            ));
        }

        let idle_timeout = provider.stream_idle_timeout();
        let byte_stream = response.bytes_stream();
        Ok(spawn_gemini_sse_stream(byte_stream, idle_timeout))
    }

    /// Streams a turn via the Anthropic Messages API (`/v1/messages`).
    pub(super) async fn stream_anthropic(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        effort: Option<ReasoningEffortConfig>,
    ) -> Result<ResponseStream> {
        use crate::anthropic_content::build_anthropic_messages_and_extract_memory_requirements;
        use crate::anthropic_content::build_anthropic_tools;
        use crate::anthropic_content::normalize_anthropic_base_url;
        use crate::anthropic_streaming::spawn_anthropic_sse_stream;
        use crate::anthropic_types::AnthropicRequest;
        use crate::anthropic_types::AnthropicThinking;

        let provider = &self.client.state.provider;
        let base_url = provider.base_url.as_ref().ok_or_else(|| {
            CodexErr::UnsupportedOperation("Anthropic providers must define a base_url".to_string())
        })?;
        let base_url = normalize_anthropic_base_url(base_url);
        let url = format!("{}/messages", base_url.as_ref().trim_end_matches('/'));

        let auth = match self.client.state.auth_manager.as_ref() {
            Some(manager) => manager.auth().await,
            None => None,
        };

        let Some(env_key) = provider.env_key.as_deref() else {
            return Err(CodexErr::UnsupportedOperation(
                "Anthropic providers must define env_key".to_string(),
            ));
        };
        let anthropic_api_key =
            resolve_provider_api_key(provider, auth.as_ref()).ok_or_else(|| {
                CodexErr::EnvVar(crate::error::EnvVarError {
                    var: env_key.to_string(),
                    instructions: provider.env_key_instructions.clone(),
                })
            })?;

        let formatted_input = prompt.get_formatted_input();
        let (messages, memory_citation_requirements) =
            build_anthropic_messages_and_extract_memory_requirements(&formatted_input);
        if messages.is_empty() {
            return Err(CodexErr::UnsupportedOperation(
                "Anthropic requests require at least one message".to_string(),
            ));
        }

        let mut instructions = prompt.base_instructions.text.trim().to_string();
        if let Some(memory_requirements) = memory_citation_requirements {
            if !instructions.is_empty() {
                instructions.push_str("\n\n");
            }
            instructions.push_str("## Memory Citation Requirements\n\n");
            instructions.push_str(&memory_requirements);
        }

        let tools = build_anthropic_tools(&prompt.tools);
        let reasoning_effort = effort.or(model_info.default_reasoning_level);
        let enable_thinking = anthropic_thinking_enabled(reasoning_effort);
        let thinking = if enable_thinking {
            Some(AnthropicThinking {
                thinking_type: "enabled".to_string(),
                budget_tokens: match reasoning_effort {
                    Some(ReasoningEffortConfig::XHigh) => 8_192,
                    Some(ReasoningEffortConfig::High) => 4_096,
                    Some(ReasoningEffortConfig::Medium) => 2_048,
                    _ => 1_024,
                },
            })
        } else {
            None
        };

        let request = AnthropicRequest {
            model: crate::model_compat::normalized_anthropic_model_slug(&model_info.slug)
                .unwrap_or(model_info.slug.as_str())
                .to_string(),
            max_tokens: resolve_anthropic_max_output_tokens(&model_info.slug),
            system: (!instructions.is_empty()).then_some(instructions),
            messages,
            tools,
            thinking,
            stream: true,
        };

        if std::env::var("CODEX_DEBUG_ANTHROPIC_REQUEST").is_ok()
            && let Ok(json) = serde_json::to_string_pretty(&request)
        {
            tracing::debug!("DEBUG ANTHROPIC REQUEST:\n{json}");
        }

        let client = crate::default_client::build_reqwest_client();
        let provider_sets_anthropic_version =
            provider.http_headers.as_ref().is_some_and(|headers| {
                headers
                    .keys()
                    .any(|header| header.eq_ignore_ascii_case("anthropic-version"))
            }) || provider.env_http_headers.as_ref().is_some_and(|headers| {
                headers
                    .keys()
                    .any(|header| header.eq_ignore_ascii_case("anthropic-version"))
            });
        let make_request_builder = || {
            let mut req_builder = client.post(&url);
            req_builder = provider.apply_http_headers(req_builder);
            req_builder = req_builder.header("x-api-key", anthropic_api_key.as_str());
            if provider_sets_anthropic_version {
                req_builder
            } else {
                req_builder.header("anthropic-version", "2023-06-01")
            }
        };

        const MAX_ATTEMPTS: u64 = 3;
        const INITIAL_DELAY_MS: u64 = 5000;
        const MAX_DELAY_MS: u64 = 30000;

        let mut attempt: u64 = 0;
        let mut current_delay = INITIAL_DELAY_MS;

        let response = loop {
            attempt += 1;
            let result = make_request_builder().json(&request).send().await;

            match result {
                Ok(resp) => break resp,
                Err(err) => {
                    let should_retry = if let Some(status) = err.status() {
                        status == StatusCode::TOO_MANY_REQUESTS
                            || (status.as_u16() >= 500 && status.as_u16() < 600)
                    } else {
                        err.is_connect() || err.is_timeout()
                    };

                    if should_retry && attempt < MAX_ATTEMPTS {
                        let jitter =
                            (current_delay as f64 * 0.3 * (rand::random::<f64>() * 2.0 - 1.0))
                                as u64;
                        let delay_with_jitter = current_delay.saturating_add(jitter);
                        tracing::debug!(
                            "Anthropic request attempt {} failed, retrying after {}ms: {}",
                            attempt,
                            delay_with_jitter,
                            err
                        );
                        tokio::time::sleep(Duration::from_millis(delay_with_jitter)).await;
                        current_delay = std::cmp::min(MAX_DELAY_MS, current_delay * 2);
                        continue;
                    }

                    return Err(CodexErr::ResponseStreamFailed(
                        crate::error::ResponseStreamFailed {
                            source: err,
                            request_id: None,
                        },
                    ));
                }
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CodexErr::UnexpectedStatus(
                crate::error::UnexpectedResponseError {
                    status,
                    body,
                    url: Some(url.clone()),
                    cf_ray: None,
                    request_id: None,
                    identity_authorization_error: None,
                    identity_error_code: None,
                },
            ));
        }

        let idle_timeout = provider.stream_idle_timeout();
        let byte_stream = response.bytes_stream();
        Ok(spawn_anthropic_sse_stream(byte_stream, idle_timeout))
    }
}

pub(super) fn validate_image_input_compat(prompt: &Prompt, model_slug: &str) -> Result<()> {
    if !model_supports_input_images(model_slug) && prompt_contains_input_images(prompt) {
        return Err(CodexErr::UnsupportedOperation(format!(
            "Model {model_slug} does not support image inputs."
        )));
    }

    if model_supports_data_url_input_images(model_slug) || !prompt_contains_data_url_images(prompt)
    {
        return Ok(());
    }

    Err(CodexErr::UnsupportedOperation(format!(
        "Model {model_slug} does not support `data:` image inputs. Use a public HTTPS image URL instead."
    )))
}

pub(super) fn build_reasoning_payload(
    supports_reasoning_summaries: bool,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
) -> Option<Reasoning> {
    let summary = if supports_reasoning_summaries && summary != ReasoningSummaryConfig::None {
        Some(summary)
    } else {
        None
    };
    if effort.is_none() && summary.is_none() {
        return None;
    }

    Some(Reasoning { effort, summary })
}

pub(super) fn sanitize_reasoning_effort_for_model(
    effort: Option<ReasoningEffortConfig>,
    model_info: &ModelInfo,
) -> Option<ReasoningEffortConfig> {
    fn effort_rank(effort: ReasoningEffortConfig) -> i64 {
        match effort {
            ReasoningEffortConfig::None => 0,
            ReasoningEffortConfig::Minimal => 1,
            ReasoningEffortConfig::Low => 2,
            ReasoningEffortConfig::Medium => 3,
            ReasoningEffortConfig::High => 4,
            ReasoningEffortConfig::XHigh => 5,
        }
    }

    let effort = effort?;

    if !model_supports_reasoning_effort(&model_info.slug) {
        let model_slug = model_info.slug.as_str();
        warn!(
            "model_reasoning_effort is set but ignored as the model does not support reasoning.effort: {model_slug}",
        );
        return None;
    }

    if model_info.supported_reasoning_levels.is_empty() {
        return Some(effort);
    }

    let supported_levels = model_info
        .supported_reasoning_levels
        .iter()
        .map(|preset| preset.effort)
        .collect::<Vec<_>>();
    if supported_levels.contains(&effort) {
        return Some(effort);
    }

    let effort_score = effort_rank(effort);
    let fallback = supported_levels
        .iter()
        .copied()
        .filter(|candidate| effort_rank(*candidate) <= effort_score)
        .max_by_key(|candidate| effort_rank(*candidate))
        .or_else(|| {
            supported_levels
                .iter()
                .copied()
                .min_by_key(|candidate| effort_rank(*candidate))
        })
        .or(model_info.default_reasoning_level);
    if let Some(fallback) = fallback {
        let model_slug = model_info.slug.as_str();
        warn!(
            "reasoning.effort={effort} is not supported for model {model_slug}, using {fallback} instead",
        );
        return Some(fallback);
    }

    let model_slug = model_info.slug.as_str();
    warn!("reasoning.effort={effort} is not supported for model {model_slug}, omitting it",);
    None
}

pub(super) fn supports_memory_trace_summarize(
    provider: &ModelProviderInfo,
    model_slug: &str,
) -> bool {
    provider.wire_api == WireApi::Responses && model_supports_memory_trace_summarize(model_slug)
}

pub(super) fn canonical_model_slug_for_provider<'a>(
    provider: &ModelProviderInfo,
    model_slug: &'a str,
) -> Cow<'a, str> {
    if !provider.is_grok() {
        return Cow::Borrowed(model_slug);
    }

    let canonical = match normalized_grok_model_slug(model_slug) {
        Some("grok-4.1") => "grok-4-latest",
        Some(grok_slug) => grok_slug,
        None => model_slug,
    };
    Cow::Borrowed(canonical)
}

fn resolve_gemini_max_output_tokens(model_slug: &str) -> Option<i32> {
    if let Ok(raw) = std::env::var("CODEX_GEMINI_MAX_OUTPUT_TOKENS") {
        let trimmed = raw.trim();
        if let Ok(parsed) = trimmed.parse::<i32>()
            && parsed > 0
        {
            return Some(parsed);
        }
        warn!("Ignoring invalid CODEX_GEMINI_MAX_OUTPUT_TOKENS value: {trimmed}");
    }

    if is_gemma_model_slug(model_slug) {
        Some(8192)
    } else {
        None
    }
}

fn resolve_anthropic_max_output_tokens(_model_slug: &str) -> i64 {
    if let Ok(raw) = std::env::var("CODEX_ANTHROPIC_MAX_OUTPUT_TOKENS") {
        let trimmed = raw.trim();
        if let Ok(parsed) = trimmed.parse::<i64>()
            && parsed > 0
        {
            return parsed;
        }
    }

    8192
}

fn anthropic_thinking_enabled(reasoning_effort: Option<ReasoningEffortConfig>) -> bool {
    anthropic_thinking_enabled_with_env(reasoning_effort, anthropic_env_thinking_enabled())
}

pub(super) fn anthropic_thinking_enabled_with_env(
    reasoning_effort: Option<ReasoningEffortConfig>,
    env_override: bool,
) -> bool {
    reasoning_effort.is_some_and(anthropic_reasoning_enables_thinking) || env_override
}

fn anthropic_reasoning_enables_thinking(effort: ReasoningEffortConfig) -> bool {
    matches!(
        effort,
        ReasoningEffortConfig::Medium | ReasoningEffortConfig::High | ReasoningEffortConfig::XHigh
    )
}

fn anthropic_env_thinking_enabled() -> bool {
    std::env::var("CODEX_ANTHROPIC_ENABLE_THINKING")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn prompt_contains_input_images(prompt: &Prompt) -> bool {
    prompt.input.iter().any(response_item_contains_input_image)
}

fn response_item_contains_input_image(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { content, .. } => content.iter().any(content_item_is_input_image),
        ResponseItem::FunctionCallOutput { output, .. } => output
            .content_items()
            .is_some_and(|items| items.iter().any(function_output_item_is_input_image)),
        _ => false,
    }
}

fn prompt_contains_data_url_images(prompt: &Prompt) -> bool {
    prompt
        .input
        .iter()
        .any(response_item_contains_data_url_image)
}

fn response_item_contains_data_url_image(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::Message { content, .. } => content.iter().any(content_item_is_data_url_image),
        ResponseItem::FunctionCallOutput { output, .. } => output
            .content_items()
            .is_some_and(|items| items.iter().any(function_output_item_is_data_url_image)),
        _ => false,
    }
}

fn content_item_is_input_image(item: &ContentItem) -> bool {
    matches!(item, ContentItem::InputImage { .. })
}

fn content_item_is_data_url_image(item: &ContentItem) -> bool {
    matches!(
        item,
        ContentItem::InputImage { image_url } if is_data_url_image(image_url)
    )
}

fn function_output_item_is_input_image(item: &FunctionCallOutputContentItem) -> bool {
    matches!(item, FunctionCallOutputContentItem::InputImage { .. })
}

fn function_output_item_is_data_url_image(item: &FunctionCallOutputContentItem) -> bool {
    matches!(
        item,
        FunctionCallOutputContentItem::InputImage { image_url, .. } if is_data_url_image(image_url)
    )
}

fn is_data_url_image(url: &str) -> bool {
    url.trim_start()
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
}
