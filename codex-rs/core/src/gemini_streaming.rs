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

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;

use crate::client_common::ResponseEvent;
use crate::client_common::ResponseStream;
use crate::error::Result;
use crate::gemini_types::*;

// ── Helpers ──────────────────────────────────────────────────────────

static GEMINI_CALL_ID_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

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
    if trimmed.len() > 100
        && let Some(first_char) = trimmed.chars().next()
    {
        let same_char_count = trimmed.chars().filter(|&c| c == first_char).count();
        let ratio = same_char_count as f64 / trimmed.len() as f64;
        if ratio > 0.9 {
            return false;
        }
    }
    if trimmed.len() > 50 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

#[derive(Debug, Default)]
struct GeminiTextStreamState {
    raw_text: String,
    emitted_reasoning: String,
    emitted_answer: String,
}

#[derive(Debug, Default)]
struct GeminiTextDeltas {
    reasoning_delta: Option<String>,
    answer_delta: Option<String>,
}

impl GeminiTextStreamState {
    fn ingest(&mut self, chunk: &str) -> GeminiTextDeltas {
        if chunk.starts_with(&self.raw_text) {
            self.raw_text.clear();
            self.raw_text.push_str(chunk);
        } else if self.raw_text.starts_with(chunk) {
            return GeminiTextDeltas::default();
        } else {
            self.raw_text.push_str(chunk);
        }

        let (reasoning, answer) = extract_reasoning_and_answer(&self.raw_text);
        GeminiTextDeltas {
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
    if !contains_gemini_markup(raw) {
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
            // Wait for a future chunk to complete the tag.
            return (reasoning, answer);
        }

        // Unknown tag-like content: treat '<' as literal text.
        push_markup_text("<", mode, &mut reasoning, &mut answer);
        cursor = start + 1;
    }

    if cursor < raw.len() {
        push_markup_text(&raw[cursor..], mode, &mut reasoning, &mut answer);
    }

    (reasoning, answer)
}

fn contains_gemini_markup(raw: &str) -> bool {
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
            // When markup tags are present, whitespace between sections should
            // not become user-visible output.
            if !text.trim().is_empty() {
                answer.push_str(text);
            }
        }
    }
}

fn gemini_stream_error_message(error: &GeminiErrorResponse) -> Option<String> {
    let message = error
        .message
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "request failed".to_string());
    let mut details = Vec::new();
    if let Some(status) = error.status.as_deref()
        && !status.trim().is_empty()
    {
        details.push(format!("status={status}"));
    }
    if let Some(code) = error.code.as_ref()
        && !code.is_null()
    {
        if let Some(code_str) = code.as_str() {
            details.push(format!("code={code_str}"));
        } else {
            details.push(format!("code={code}"));
        }
    }

    if details.is_empty() {
        Some(message)
    } else {
        Some(format!("{message} ({})", details.join(", ")))
    }
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
    let mut text_stream_state = GeminiTextStreamState::default();
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
                debug!(
                    "Failed to parse Gemini SSE event: {err}, data: {}",
                    &sse.data
                );
                continue;
            }
        };

        if let Some(error_message) = chunk.error.as_ref().and_then(gemini_stream_error_message) {
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
            let separator = (!accumulated_text.is_empty())
                .then_some("\n\n")
                .unwrap_or("");
            let notice = format!("{separator}[Gemini stream interrupted: {error_message}]");
            if tx_event
                .send(Ok(ResponseEvent::OutputTextDelta(notice.clone())))
                .await
                .is_err()
            {
                return;
            }
            accumulated_text.push_str(&notice);
            break;
        }

        if let Some(id) = chunk.response_id {
            last_response_id = id;
        }
        if let Some(usage) = chunk.usage_metadata {
            last_token_usage = Some(usage.into());
        }

        if let Some(candidates) = chunk.candidates {
            for candidate in candidates {
                if let Some(content) = candidate.content
                    && let Some(parts) = content.parts
                {
                    for part in parts {
                        if let Some(sig) = &part.thought_signature {
                            last_thought_signature = Some(sig.clone());
                        }
                        let is_thought = part.thought.is_some();

                        // Handle thought content
                        if is_thought {
                            if let Some(text) = &part.text
                                && !text.is_empty()
                                && is_meaningful_thought_text(text)
                            {
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
                            continue;
                        }

                        // Handle text content
                        if let Some(ref text) = part.text
                            && !text.is_empty()
                        {
                            let deltas = text_stream_state.ingest(text);

                            if let Some(reasoning_delta) = deltas.reasoning_delta
                                && is_meaningful_thought_text(&reasoning_delta)
                            {
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
                                        delta: reasoning_delta,
                                        content_index: 0,
                                    }))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }

                            if let Some(answer_delta) = deltas.answer_delta {
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
                                    .send(Ok(ResponseEvent::OutputTextDelta(answer_delta.clone())))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                accumulated_text.push_str(&answer_delta);
                            }
                        }

                        // Handle image content
                        if let Some(inline_data) = part.inline_data
                            && !inline_data.data.trim().is_empty()
                            && !inline_data.mime_type.is_empty()
                        {
                            last_inline_image = Some((inline_data.mime_type, inline_data.data));
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
                            if let Some(last) = function_calls.last_mut()
                                && last.0 == name
                                && last.1 == args
                            {
                                last.2 = thought_signature;
                                continue;
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

    // Emit final items
    if assistant_item_sent || last_inline_image.is_some() {
        let mut content = Vec::new();
        if !accumulated_text.is_empty() {
            content.push(ContentItem::OutputText {
                text: accumulated_text,
            });
        }
        if let Some((mime_type, data)) = last_inline_image
            && !mime_type.is_empty()
            && !data.trim().is_empty()
        {
            let image_url = format!("data:{mime_type};base64,{data}");
            content.push(ContentItem::InputImage { image_url });
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
            let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
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
        let _ = tx_event.send(Ok(ResponseEvent::OutputItemDone(item))).await;
    }

    let _ = tx_event
        .send(Ok(ResponseEvent::Completed {
            response_id: last_response_id,
            token_usage: last_token_usage,
        }))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use pretty_assertions::assert_eq;

    #[test]
    fn extract_reasoning_and_answer_for_plain_text() {
        let (reasoning, answer) = extract_reasoning_and_answer("hello");

        assert_eq!(reasoning, "");
        assert_eq!(answer, "hello");
    }

    #[test]
    fn extract_reasoning_and_answer_for_marked_text() {
        let (reasoning, answer) =
            extract_reasoning_and_answer("<thinking>plan</thinking>\n<answer>done</answer>");

        assert_eq!(reasoning, "plan");
        assert_eq!(answer, "done");
    }

    #[test]
    fn cumulative_chunks_emit_incremental_reasoning_and_answer() {
        let mut state = GeminiTextStreamState::default();
        let chunks = vec![
            "<thinking",
            "<thinking>plan",
            "<thinking>plan</thinking>\n<answer>do",
            "<thinking>plan</thinking>\n<answer>done</answer>",
        ];

        let mut reasoning = String::new();
        let mut answer = String::new();
        for chunk in chunks {
            let deltas = state.ingest(chunk);
            if let Some(delta) = deltas.reasoning_delta {
                reasoning.push_str(&delta);
            }
            if let Some(delta) = deltas.answer_delta {
                answer.push_str(&delta);
            }
        }

        assert_eq!(reasoning, "plan");
        assert_eq!(answer, "done");
    }

    #[test]
    fn delta_chunks_emit_incremental_answer() {
        let mut state = GeminiTextStreamState::default();
        let chunks = vec!["Hel", "lo", " world"];

        let mut answer = String::new();
        for chunk in chunks {
            let deltas = state.ingest(chunk);
            if let Some(delta) = deltas.answer_delta {
                answer.push_str(&delta);
            }
        }

        assert_eq!(answer, "Hello world");
    }

    #[test]
    fn gemini_stream_error_message_includes_status_and_code() {
        let error = GeminiErrorResponse {
            message: Some("backend timeout".to_string()),
            code: Some(serde_json::json!(504)),
            status: Some("DEADLINE_EXCEEDED".to_string()),
        };

        assert_eq!(
            gemini_stream_error_message(&error),
            Some("backend timeout (status=DEADLINE_EXCEEDED, code=504)".to_string())
        );
    }

    #[tokio::test]
    async fn stream_error_event_appends_visible_notice() {
        let payload = "data: {\"error\":{\"message\":\"backend timeout\"}}\n\n";
        let bytes_stream =
            futures::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(payload))]);
        let mut response_stream = spawn_gemini_sse_stream(bytes_stream, Duration::from_secs(1));

        let mut deltas = String::new();
        let mut final_output = String::new();

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
                ResponseEvent::Completed { .. } => break,
                _ => {}
            }
        }

        assert_eq!(
            deltas,
            "[Gemini stream interrupted: backend timeout]".to_string()
        );
        assert_eq!(
            final_output,
            "[Gemini stream interrupted: backend timeout]".to_string()
        );
    }
}
