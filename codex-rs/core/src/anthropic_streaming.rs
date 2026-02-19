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

#[derive(Debug)]
enum BlockState {
    Text {
        id: String,
        accumulated: String,
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
                        if !text.is_empty() {
                            if tx_event
                                .send(Ok(ResponseEvent::OutputTextDelta(text.clone())))
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
                                accumulated: text,
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
                        if !thinking.is_empty() {
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
                        BlockState::Text { accumulated, .. },
                        AnthropicContentBlockDelta::TextDelta { text },
                    ) => {
                        if tx_event
                            .send(Ok(ResponseEvent::OutputTextDelta(text.clone())))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        accumulated.push_str(&text);
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
                    BlockState::Text { id, accumulated } => {
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
                BlockState::Text { id, accumulated } => {
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
}
