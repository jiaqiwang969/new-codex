//! Anthropic SSE streaming: converts Anthropic Messages API stream events into `ResponseEvent`s.

use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use futures::TryStreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;

use crate::anthropic_types::AnthropicContentBlockDelta;
use crate::anthropic_types::AnthropicContentBlockDeltaEvent;
use crate::anthropic_types::AnthropicContentBlockStart;
use crate::anthropic_types::AnthropicContentBlockStartEvent;
use crate::anthropic_types::AnthropicContentBlockStopEvent;
use crate::anthropic_types::AnthropicMessageDeltaEvent;
use crate::anthropic_types::AnthropicMessageStartEvent;
use crate::anthropic_types::AnthropicUsage;
use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::error::Result;

#[derive(Debug, Default)]
struct AnthropicTextStreamState {
    raw_text: String,
    emitted_reasoning: String,
    emitted_answer: String,
}

#[derive(Debug, Default)]
struct AnthropicTextDeltas {
    reasoning_delta: Option<String>,
    answer_delta: Option<String>,
}

impl AnthropicTextStreamState {
    fn ingest(&mut self, chunk: &str) -> AnthropicTextDeltas {
        if chunk.starts_with(&self.raw_text) {
            self.raw_text.clear();
            self.raw_text.push_str(chunk);
        } else if self.raw_text.starts_with(chunk) {
            return AnthropicTextDeltas::default();
        } else {
            self.raw_text.push_str(chunk);
        }

        let (reasoning, answer) = extract_reasoning_and_answer(&self.raw_text);
        AnthropicTextDeltas {
            reasoning_delta: update_emitted_text(&reasoning, &mut self.emitted_reasoning),
            answer_delta: update_emitted_text(&answer, &mut self.emitted_answer),
        }
    }
}

fn update_emitted_text(next: &str, emitted: &mut String) -> Option<String> {
    if next == emitted {
        return None;
    }

    let delta = if let Some(suffix) = next.strip_prefix(emitted.as_str()) {
        suffix.to_string()
    } else {
        next.to_string()
    };

    emitted.clear();
    emitted.push_str(next);
    (!delta.is_empty()).then_some(delta)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MarkupMode {
    #[default]
    Plain,
    Thinking,
    Answer,
}

fn extract_reasoning_and_answer(raw: &str) -> (String, String) {
    if !contains_reasoning_markup(raw) {
        return (String::new(), raw.to_string());
    }

    let mut reasoning = String::new();
    let mut answer = String::new();
    let mut cursor = 0usize;
    let mut mode = MarkupMode::Plain;

    while let Some(start_rel) = raw[cursor..].find('<') {
        let start = cursor + start_rel;
        let text = &raw[cursor..start];
        push_markup_text(text, mode, &mut reasoning, &mut answer);

        let tail = &raw[start..];
        if tail.starts_with("<thinking>") {
            mode = MarkupMode::Thinking;
            cursor = start + "<thinking>".len();
            continue;
        }
        if tail.starts_with("</thinking>") {
            mode = MarkupMode::Plain;
            cursor = start + "</thinking>".len();
            continue;
        }
        if tail.starts_with("<answer>") {
            mode = MarkupMode::Answer;
            cursor = start + "<answer>".len();
            continue;
        }
        if tail.starts_with("</answer>") {
            mode = MarkupMode::Plain;
            cursor = start + "</answer>".len();
            continue;
        }
        if looks_like_partial_markup_tag(tail) {
            return (reasoning, answer);
        }

        push_markup_text("<", mode, &mut reasoning, &mut answer);
        cursor = start + 1;
    }

    if cursor < raw.len() {
        push_markup_text(&raw[cursor..], mode, &mut reasoning, &mut answer);
    }

    (reasoning, answer)
}

fn contains_reasoning_markup(raw: &str) -> bool {
    raw.contains("<thinking>")
        || raw.contains("<thinking")
        || raw.contains("</thinking>")
        || raw.contains("</thinking")
        || raw.contains("<answer>")
        || raw.contains("<answer")
        || raw.contains("</answer>")
        || raw.contains("</answer")
}

fn looks_like_partial_markup_tag(fragment: &str) -> bool {
    if !fragment.starts_with('<') {
        return false;
    }
    const TAGS: [&str; 4] = ["<thinking>", "</thinking>", "<answer>", "</answer>"];
    TAGS.iter().any(|tag| tag.starts_with(fragment))
}

fn push_markup_text(text: &str, mode: MarkupMode, reasoning: &mut String, answer: &mut String) {
    if text.is_empty() {
        return;
    }

    match mode {
        MarkupMode::Thinking => reasoning.push_str(text),
        MarkupMode::Answer => answer.push_str(text),
        MarkupMode::Plain => {
            if !text.trim().is_empty() {
                if answer.is_empty() {
                    answer.push_str(text.trim_start());
                } else {
                    answer.push_str(text);
                }
            }
        }
    }
}

#[derive(Debug)]
enum BlockState {
    Text {
        id: String,
        accumulated: String,
        text_stream_state: AnthropicTextStreamState,
    },
    Thinking {
        id: String,
        accumulated: String,
    },
    ToolUse {
        call_id: String,
        name: String,
        initial_input: serde_json::Value,
        input_json: String,
    },
}

pub(crate) fn spawn_anthropic_sse_stream<S>(
    byte_stream: S,
    idle_timeout: Duration,
) -> ResponseStream
where
    S: futures::Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);
    tokio::spawn(async move {
        process_anthropic_sse(byte_stream, tx_event, idle_timeout).await;
    });
    ResponseStream { rx_event }
}

async fn process_anthropic_sse<S>(
    stream: S,
    tx_event: mpsc::Sender<Result<ResponseEvent>>,
    idle_timeout: Duration,
) where
    S: futures::Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin,
{
    if tx_event.send(Ok(ResponseEvent::Created)).await.is_err() {
        return;
    }

    let mut stream = stream
        .map_err(|e| std::io::Error::other(e.to_string()))
        .eventsource();

    let mut response_id = "anthropic-stream".to_string();
    let mut blocks: HashMap<usize, BlockState> = HashMap::new();
    let mut last_usage: Option<AnthropicUsage> = None;
    let mut saw_any_output = false;

    loop {
        let response = timeout(idle_timeout, stream.next()).await;

        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                debug!("Anthropic SSE stream error: {}", e);
                break;
            }
            Ok(None) => break,
            Err(_) => {
                debug!("Anthropic SSE idle timeout");
                break;
            }
        };

        if sse.data.trim().is_empty() {
            continue;
        }

        let event_type = match serde_json::from_str::<serde_json::Value>(&sse.data) {
            Ok(value) => value
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string(),
            Err(err) => {
                debug!(
                    "Failed to parse Anthropic SSE envelope: {err}, data: {}",
                    &sse.data
                );
                continue;
            }
        };

        match event_type.as_str() {
            "message_start" => {
                let parsed: AnthropicMessageStartEvent = match serde_json::from_str(&sse.data) {
                    Ok(val) => val,
                    Err(err) => {
                        debug!(
                            "Failed to parse Anthropic message_start: {err}, data: {}",
                            &sse.data
                        );
                        continue;
                    }
                };
                response_id = parsed.message.id.clone();
                if tx_event
                    .send(Ok(ResponseEvent::ServerModel(parsed.message.model.clone())))
                    .await
                    .is_err()
                {
                    return;
                }
                last_usage = parsed.message.usage;
            }
            "content_block_start" => {
                let parsed: AnthropicContentBlockStartEvent = match serde_json::from_str(&sse.data)
                {
                    Ok(val) => val,
                    Err(err) => {
                        debug!(
                            "Failed to parse Anthropic content_block_start: {err}, data: {}",
                            &sse.data
                        );
                        continue;
                    }
                };
                let index = parsed.index;
                match parsed.content_block {
                    AnthropicContentBlockStart::Text { text } => {
                        let id = format!("anthropic-text-{response_id}-{index}");
                        let item = ResponseItem::Message {
                            id: Some(id.clone()),
                            role: "assistant".to_string(),
                            content: vec![],
                            end_turn: None,
                            phase: None,
                            thought_signature: None,
                        };
                        if tx_event
                            .send(Ok(ResponseEvent::OutputItemAdded(item)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        saw_any_output = true;
                        let mut accumulated = String::new();
                        let mut text_stream_state = AnthropicTextStreamState::default();
                        let deltas = text_stream_state.ingest(&text);
                        if let Some(reasoning_delta) = deltas.reasoning_delta
                            && tx_event
                                .send(Ok(ResponseEvent::ReasoningContentDelta {
                                    delta: reasoning_delta,
                                    content_index: 0,
                                }))
                                .await
                                .is_err()
                        {
                            return;
                        }
                        if let Some(answer_delta) = deltas.answer_delta {
                            accumulated.push_str(&answer_delta);
                            if tx_event
                                .send(Ok(ResponseEvent::OutputTextDelta(answer_delta)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        blocks.insert(
                            index,
                            BlockState::Text {
                                id,
                                accumulated,
                                text_stream_state,
                            },
                        );
                    }
                    AnthropicContentBlockStart::Thinking { thinking } => {
                        let id = format!("anthropic-thinking-{response_id}-{index}");
                        let item = ResponseItem::Reasoning {
                            id: id.clone(),
                            summary: vec![],
                            content: None,
                            encrypted_content: None,
                        };
                        if tx_event
                            .send(Ok(ResponseEvent::OutputItemAdded(item)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        saw_any_output = true;
                        if !thinking.is_empty()
                            && tx_event
                                .send(Ok(ResponseEvent::ReasoningContentDelta {
                                    delta: thinking.clone(),
                                    content_index: 0,
                                }))
                                .await
                                .is_err()
                        {
                            return;
                        }
                        blocks.insert(
                            index,
                            BlockState::Thinking {
                                id,
                                accumulated: thinking,
                            },
                        );
                    }
                    AnthropicContentBlockStart::ToolUse { id, name, input } => {
                        saw_any_output = true;
                        blocks.insert(
                            index,
                            BlockState::ToolUse {
                                call_id: id,
                                name,
                                initial_input: input,
                                input_json: String::new(),
                            },
                        );
                    }
                    AnthropicContentBlockStart::Unknown => {}
                }
            }
            "content_block_delta" => {
                let parsed: AnthropicContentBlockDeltaEvent = match serde_json::from_str(&sse.data)
                {
                    Ok(val) => val,
                    Err(err) => {
                        debug!(
                            "Failed to parse Anthropic content_block_delta: {err}, data: {}",
                            &sse.data
                        );
                        continue;
                    }
                };
                let Some(block) = blocks.get_mut(&parsed.index) else {
                    debug!("Anthropic delta for unknown block index {}", parsed.index);
                    continue;
                };
                match (&mut *block, parsed.delta) {
                    (
                        BlockState::Text {
                            accumulated,
                            text_stream_state,
                            ..
                        },
                        AnthropicContentBlockDelta::TextDelta { text },
                    ) => {
                        let deltas = text_stream_state.ingest(&text);
                        if let Some(reasoning_delta) = deltas.reasoning_delta
                            && tx_event
                                .send(Ok(ResponseEvent::ReasoningContentDelta {
                                    delta: reasoning_delta,
                                    content_index: 0,
                                }))
                                .await
                                .is_err()
                        {
                            return;
                        }
                        if let Some(answer_delta) = deltas.answer_delta {
                            if tx_event
                                .send(Ok(ResponseEvent::OutputTextDelta(answer_delta.clone())))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            accumulated.push_str(&answer_delta);
                        }
                    }
                    (
                        BlockState::Thinking { accumulated, .. },
                        AnthropicContentBlockDelta::ThinkingDelta { thinking },
                    ) => {
                        if tx_event
                            .send(Ok(ResponseEvent::ReasoningContentDelta {
                                delta: thinking.clone(),
                                content_index: 0,
                            }))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        accumulated.push_str(&thinking);
                    }
                    (
                        BlockState::ToolUse { input_json, .. },
                        AnthropicContentBlockDelta::InputJsonDelta { partial_json },
                    ) => {
                        input_json.push_str(&partial_json);
                    }
                    (_, AnthropicContentBlockDelta::Unknown) => {}
                    _ => {}
                }
            }
            "content_block_stop" => {
                let parsed: AnthropicContentBlockStopEvent = match serde_json::from_str(&sse.data) {
                    Ok(val) => val,
                    Err(err) => {
                        debug!(
                            "Failed to parse Anthropic content_block_stop: {err}, data: {}",
                            &sse.data
                        );
                        continue;
                    }
                };
                let Some(block) = blocks.remove(&parsed.index) else {
                    continue;
                };
                match block {
                    BlockState::Text {
                        id, accumulated, ..
                    } => {
                        if accumulated.is_empty() {
                            continue;
                        }
                        let item = ResponseItem::Message {
                            id: Some(id),
                            role: "assistant".to_string(),
                            content: vec![ContentItem::OutputText { text: accumulated }],
                            end_turn: None,
                            phase: None,
                            thought_signature: None,
                        };
                        if tx_event
                            .send(Ok(ResponseEvent::OutputItemDone(item)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    BlockState::Thinking { id, accumulated: _ } => {
                        let item = ResponseItem::Reasoning {
                            id,
                            summary: vec![],
                            content: None,
                            encrypted_content: None,
                        };
                        if tx_event
                            .send(Ok(ResponseEvent::OutputItemDone(item)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    BlockState::ToolUse {
                        call_id,
                        name,
                        initial_input,
                        input_json,
                    } => {
                        let arguments = if input_json.trim().is_empty() {
                            if initial_input.is_null() {
                                "{}".to_string()
                            } else {
                                initial_input.to_string()
                            }
                        } else {
                            input_json
                        };
                        let arguments = match serde_json::from_str::<serde_json::Value>(&arguments)
                        {
                            Ok(_) => arguments,
                            Err(_) => serde_json::json!({ "_raw": arguments }).to_string(),
                        };
                        let item = ResponseItem::FunctionCall {
                            id: None,
                            name,
                            arguments,
                            call_id,
                            thought_signature: None,
                        };
                        if tx_event
                            .send(Ok(ResponseEvent::OutputItemDone(item)))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            "message_delta" => {
                let parsed: AnthropicMessageDeltaEvent = match serde_json::from_str(&sse.data) {
                    Ok(val) => val,
                    Err(err) => {
                        debug!(
                            "Failed to parse Anthropic message_delta: {err}, data: {}",
                            &sse.data
                        );
                        continue;
                    }
                };
                if let Some(usage) = parsed.usage {
                    last_usage = Some(merge_usage(last_usage.as_ref(), &usage));
                }
            }
            "message_stop" => break,
            "ping" => {}
            "error" => {
                debug!("Anthropic error event: {}", sse.data);
                break;
            }
            other => {
                debug!("Ignoring Anthropic SSE event type {other}");
            }
        }
    }

    // Flush any remaining blocks (best-effort).
    if !blocks.is_empty() {
        let mut indices: Vec<usize> = blocks.keys().copied().collect();
        indices.sort_unstable();
        for idx in indices {
            let Some(block) = blocks.remove(&idx) else {
                continue;
            };
            match block {
                BlockState::Text {
                    id, accumulated, ..
                } => {
                    if accumulated.is_empty() {
                        continue;
                    }
                    let item = ResponseItem::Message {
                        id: Some(id),
                        role: "assistant".to_string(),
                        content: vec![ContentItem::OutputText { text: accumulated }],
                        end_turn: None,
                        phase: None,
                        thought_signature: None,
                    };
                    let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                }
                BlockState::Thinking { id, accumulated: _ } => {
                    let item = ResponseItem::Reasoning {
                        id,
                        summary: vec![],
                        content: None,
                        encrypted_content: None,
                    };
                    let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                }
                BlockState::ToolUse {
                    call_id,
                    name,
                    initial_input,
                    input_json,
                } => {
                    let arguments = if input_json.trim().is_empty() {
                        if initial_input.is_null() {
                            "{}".to_string()
                        } else {
                            initial_input.to_string()
                        }
                    } else {
                        input_json
                    };
                    let arguments = match serde_json::from_str::<serde_json::Value>(&arguments) {
                        Ok(_) => arguments,
                        Err(_) => serde_json::json!({ "_raw": arguments }).to_string(),
                    };
                    let item = ResponseItem::FunctionCall {
                        id: None,
                        name,
                        arguments,
                        call_id,
                        thought_signature: None,
                    };
                    let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
                }
            }
        }
    }

    if !saw_any_output {
        let item = ResponseItem::Message {
            id: Some(format!("anthropic-empty-{response_id}")),
            role: "assistant".to_string(),
            content: vec![],
            end_turn: None,
            phase: None,
            thought_signature: None,
        };
        let notice =
            "[Anthropic stream returned no content. Check provider model name and endpoint compatibility.]"
                .to_string();
        if tx_event
            .send(Ok(ResponseEvent::OutputItemAdded(item)))
            .await
            .is_ok()
        {
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputTextDelta(notice.clone())))
                .await;
            let item = ResponseItem::Message {
                id: Some(format!("anthropic-empty-{response_id}")),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText { text: notice }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            };
            let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
        }
    }

    let token_usage: Option<TokenUsage> = last_usage.map(Into::into);
    let _ = tx_event
        .send(Ok(ResponseEvent::Completed {
            response_id,
            token_usage,
            can_append: false,
        }))
        .await;
}

fn merge_usage(previous: Option<&AnthropicUsage>, next: &AnthropicUsage) -> AnthropicUsage {
    let mut merged = previous.cloned().unwrap_or_default();
    if next.input_tokens > 0 {
        merged.input_tokens = next.input_tokens;
    }
    if next.output_tokens > 0 {
        merged.output_tokens = next.output_tokens;
    }
    if next.cache_creation_input_tokens > 0 {
        merged.cache_creation_input_tokens = next.cache_creation_input_tokens;
    }
    if next.cache_read_input_tokens > 0 {
        merged.cache_read_input_tokens = next.cache_read_input_tokens;
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn text_block_emits_deltas_and_final_message() {
        let payload = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let bytes_stream =
            futures::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut response_stream = spawn_anthropic_sse_stream(bytes_stream, Duration::from_secs(1));

        let mut deltas = String::new();
        let mut final_output = String::new();
        let mut completed_usage = None;

        while let Some(event) = response_stream.next().await {
            match event.expect("stream event should be ok") {
                ResponseEvent::OutputTextDelta(delta) => deltas.push_str(&delta),
                ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                    for item in content {
                        if let ContentItem::OutputText { text } = item {
                            final_output.push_str(&text);
                        }
                    }
                }
                ResponseEvent::Completed { token_usage, .. } => {
                    completed_usage = token_usage;
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(deltas, "Hello".to_string());
        assert_eq!(final_output, "Hello".to_string());
        assert_eq!(
            completed_usage,
            Some(TokenUsage {
                input_tokens: 1,
                cached_input_tokens: 0,
                output_tokens: 2,
                reasoning_output_tokens: 0,
                total_tokens: 3,
            })
        );
    }

    #[tokio::test]
    async fn tool_use_block_emits_function_call_with_json_args() {
        let payload = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-test\",\"usage\":{}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"shell_command\",\"input\":{}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\\\"echo hi\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let bytes_stream =
            futures::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut response_stream = spawn_anthropic_sse_stream(bytes_stream, Duration::from_secs(1));

        let mut seen_call = None;
        while let Some(event) = response_stream.next().await {
            match event.expect("stream event should be ok") {
                ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                    name,
                    arguments,
                    call_id,
                    ..
                }) => {
                    seen_call = Some((name, arguments, call_id));
                }
                ResponseEvent::Completed { .. } => break,
                _ => {}
            }
        }

        let (name, args, call_id) = seen_call.expect("expected function call");
        assert_eq!(name, "shell_command".to_string());
        assert_eq!(call_id, "toolu_1".to_string());
        assert_eq!(args, "{\"cmd\":\"echo hi\"}".to_string());
    }

    #[tokio::test]
    async fn text_block_with_thinking_markup_routes_only_answer_to_message_output() {
        let payload = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"<thinking>The user just said hi.\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" Simple greeting.</thinking>\\n\\nHey\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"! What are you working on?\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let bytes_stream =
            futures::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut response_stream = spawn_anthropic_sse_stream(bytes_stream, Duration::from_secs(1));

        let mut reasoning = String::new();
        let mut answer_deltas = String::new();
        let mut final_output = String::new();

        while let Some(event) = response_stream.next().await {
            match event.expect("stream event should be ok") {
                ResponseEvent::ReasoningContentDelta { delta, .. } => reasoning.push_str(&delta),
                ResponseEvent::OutputTextDelta(delta) => answer_deltas.push_str(&delta),
                ResponseEvent::OutputItemDone(ResponseItem::Message { content, .. }) => {
                    for item in content {
                        if let ContentItem::OutputText { text } = item {
                            final_output.push_str(&text);
                        }
                    }
                }
                ResponseEvent::Completed { .. } => break,
                _ => {}
            }
        }

        assert_eq!(
            reasoning,
            "The user just said hi. Simple greeting.".to_string()
        );
        assert_eq!(answer_deltas, "Hey! What are you working on?".to_string());
        assert_eq!(final_output, "Hey! What are you working on?".to_string());
    }
}
