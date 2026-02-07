//! Gemini SSE streaming: spawns tasks that process Gemini Server-Sent Events
//! and convert them into the internal `ResponseEvent` / `ResponseStream` types.

use std::time::Duration;

use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use futures::TryStreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;

use codex_protocol::models::{ContentItem, ResponseItem};
use codex_protocol::protocol::TokenUsage;

use crate::client_common::{ResponseEvent, ResponseStream};
use crate::error::Result;
use crate::gemini_types::*;

// ── Helpers ──────────────────────────────────────────────────────────

static GEMINI_CALL_ID_COUNTER: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

fn next_gemini_call_id() -> String {
    let id = GEMINI_CALL_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("gemini-function-call-{id}")
}

/// Checks if the given text is meaningful for display as reasoning content.
fn is_meaningful_thought_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() > 100 {
        if let Some(first_char) = trimmed.chars().next() {
            let same_char_count = trimmed.chars().filter(|&c| c == first_char).count();
            let ratio = same_char_count as f64 / trimmed.len() as f64;
            if ratio > 0.9 {
                return false;
            }
        }
    }
    if trimmed.len() > 50 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

// ── Stream constructors ──────────────────────────────────────────────

/// Creates a `ResponseStream` from a single pre-built item (used for error
/// fallback paths).
pub(crate) fn spawn_gemini_response_stream(
    response_item: Option<ResponseItem>,
    response_id: String,
    token_usage: Option<TokenUsage>,
) -> ResponseStream {
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(8);
    tokio::spawn(async move {
        if tx_event.send(Ok(ResponseEvent::Created)).await.is_err() {
            return;
        }
        if let Some(item) = response_item {
            if tx_event
                .send(Ok(ResponseEvent::OutputItemAdded(item.clone())))
                .await
                .is_err()
            {
                return;
            }
            if tx_event
                .send(Ok(ResponseEvent::OutputItemDone(item)))
                .await
                .is_err()
            {
                return;
            }
        }
        let _ = tx_event
            .send(Ok(ResponseEvent::Completed {
                response_id,
                token_usage,
            }))
            .await;
    });
    ResponseStream { rx_event }
}

/// Spawns a background task that processes a Gemini SSE byte stream and
/// converts it into a `ResponseStream`.
pub(crate) fn spawn_gemini_sse_stream<S>(byte_stream: S, idle_timeout: Duration) -> ResponseStream
where
    S: futures::Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent>>(1600);
    tokio::spawn(async move {
        process_gemini_sse(byte_stream, tx_event, idle_timeout).await;
    });
    ResponseStream { rx_event }
}

// ── Core SSE processor ───────────────────────────────────────────────

async fn process_gemini_sse<S>(
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
        .map_ok(|b| b)
        .map_err(|e| std::io::Error::other(e.to_string()))
        .eventsource();

    let mut accumulated_text = String::new();
    let mut assistant_item_sent = false;
    let mut reasoning_item_sent = false;
    // (name, args, thought_signature, call_id)
    let mut function_calls: Vec<(String, String, Option<String>, String)> = Vec::new();
    let mut last_response_id = "gemini-stream".to_string();
    let mut last_token_usage: Option<TokenUsage> = None;
    let mut last_thought_signature: Option<String> = None;
    let mut last_inline_image: Option<(String, String)> = None;

    loop {
        let response = timeout(idle_timeout, stream.next()).await;

        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                debug!("Gemini SSE stream error: {}", e);
                break;
            }
            Ok(None) => break,
            Err(_) => {
                debug!("Gemini SSE idle timeout");
                break;
            }
        };

        if sse.data.trim().is_empty() {
            continue;
        }

        let chunk: GeminiResponse = match serde_json::from_str(&sse.data) {
            Ok(val) => val,
            Err(err) => {
                debug!("Failed to parse Gemini SSE event: {err}, data: {}", &sse.data);
                continue;
            }
        };

        if let Some(id) = chunk.response_id {
            last_response_id = id;
        }
        if let Some(usage) = chunk.usage_metadata {
            last_token_usage = Some(usage.into());
        }

        if let Some(candidates) = chunk.candidates {
            for candidate in candidates {
                if let Some(content) = candidate.content {
                    if let Some(parts) = content.parts {
                        for part in parts {
                            if let Some(sig) = &part.thought_signature {
                                last_thought_signature = Some(sig.clone());
                            }
                            let is_thought = part.thought.is_some();

                            // Handle thought content
                            if is_thought {
                                if let Some(text) = &part.text {
                                    if !text.is_empty() && is_meaningful_thought_text(text) {
                                        if !reasoning_item_sent {
                                            let item = ResponseItem::Reasoning {
                                                id: format!("gemini-thought-{last_response_id}"),
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
                                            reasoning_item_sent = true;
                                        }
                                        if tx_event
                                            .send(Ok(ResponseEvent::ReasoningContentDelta {
                                                delta: text.clone(),
                                                content_index: 0,
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                continue;
                            }

                            // Handle text content
                            if let Some(ref text) = part.text {
                                if !text.is_empty() {
                                    if !assistant_item_sent {
                                        let item = ResponseItem::Message {
                                            id: None,
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
                                        assistant_item_sent = true;
                                    }
                                    if tx_event
                                        .send(Ok(ResponseEvent::OutputTextDelta(text.clone())))
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                    accumulated_text.push_str(text);
                                }
                            }

                            // Handle image content
                            if let Some(inline_data) = part.inline_data {
                                if !inline_data.data.trim().is_empty()
                                    && !inline_data.mime_type.is_empty()
                                {
                                    last_inline_image =
                                        Some((inline_data.mime_type, inline_data.data));
                                }
                            }

                            // Handle function call
                            if let Some(call) = part.function_call {
                                let name = call.name;
                                let args = if call.args.is_null() {
                                    "{}".to_string()
                                } else {
                                    call.args.to_string()
                                };
                                let thought_signature =
                                    part.thought_signature.or(last_thought_signature.clone());
                                // Deduplicate streaming function calls
                                if let Some(last) = function_calls.last_mut() {
                                    if last.0 == name && last.1 == args {
                                        last.2 = thought_signature;
                                        continue;
                                    }
                                }
                                function_calls.push((
                                    name,
                                    args,
                                    thought_signature,
                                    next_gemini_call_id(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Emit final items
    if assistant_item_sent || last_inline_image.is_some() {
        let mut content = Vec::new();
        if !accumulated_text.is_empty() {
            content.push(ContentItem::OutputText {
                text: accumulated_text,
            });
        }
        if let Some((mime_type, data)) = last_inline_image {
            if !mime_type.is_empty() && !data.trim().is_empty() {
                let image_url = format!("data:{mime_type};base64,{data}");
                content.push(ContentItem::InputImage { image_url });
            }
        }
        if !content.is_empty() {
            let item = ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content,
                end_turn: None,
                phase: None,
                thought_signature: last_thought_signature.clone(),
            };
            let _ = tx_event
                .send(Ok(ResponseEvent::OutputItemDone(item)))
                .await;
        }
    }

    for (name, arguments, thought_signature, call_id) in function_calls {
        let item = ResponseItem::FunctionCall {
            id: None,
            name,
            arguments,
            call_id,
            thought_signature,
        };
        let _ = tx_event
            .send(Ok(ResponseEvent::OutputItemDone(item)))
            .await;
    }

    let _ = tx_event
        .send(Ok(ResponseEvent::Completed {
            response_id: last_response_id,
            token_usage: last_token_usage,
        }))
        .await;
}
