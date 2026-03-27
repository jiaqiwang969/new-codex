use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use codex_api::RawMemory;
use codex_api::RawMemoryMetadata;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use codex_utils_string::take_bytes_at_char_boundary;
use serde_json::Value;
use tracing::warn;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::compact;
use crate::state_db;
use crate::utility_model;
use codex_features::Feature;

const THREAD_MEMORY_MAX_TRACE_ITEMS: usize = 200;
const THREAD_MEMORY_MAX_TRACE_BYTES: usize = 60_000;
const THREAD_MEMORY_MAX_MESSAGE_BYTES: usize = 2_000;
const THREAD_MEMORY_THREAD_POLL_ATTEMPTS: usize = 80;
const THREAD_MEMORY_THREAD_POLL_SLEEP: Duration = Duration::from_millis(25);
const THREAD_MEMORY_MIN_UPDATE_INTERVAL_SECS: i64 = 10 * 60;

pub(crate) fn maybe_spawn_thread_memory_update_after_compaction(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    compaction_summary: String,
    trace_items: Vec<Value>,
) {
    if !thread_memory_updates_enabled(&sess, &turn_context) {
        return;
    }
    if compaction_summary.trim().is_empty() && trace_items.is_empty() {
        return;
    }

    tokio::spawn(async move {
        update_thread_memory_after_compaction(sess, turn_context, compaction_summary, trace_items)
            .await;
    });
}

pub(crate) fn maybe_spawn_thread_memory_update_after_turn(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    last_agent_message: Option<String>,
) {
    if !thread_memory_updates_enabled(&sess, &turn_context) {
        return;
    }

    tokio::spawn(async move {
        update_thread_memory_after_turn(sess, turn_context, last_agent_message).await;
    });
}

fn thread_memory_updates_enabled(sess: &Session, turn_context: &TurnContext) -> bool {
    if sess.state_db().is_none() || !turn_context.features.enabled(Feature::MemoryTool) {
        return false;
    }

    !matches!(turn_context.session_source, SessionSource::SubAgent(_))
}

pub fn build_thread_memory_trace_items(history: &[ResponseItem]) -> Vec<Value> {
    let mut candidates: Vec<(usize, Value)> = Vec::new();

    let summary_prefix = format!("{}\n", compact::SUMMARY_PREFIX);

    for item in history {
        match item {
            ResponseItem::Message { role, content, .. } => {
                if role != "user" && role != "assistant" && role != "system" && role != "developer"
                {
                    continue;
                }

                let Some(raw_text) = compact::content_items_to_text(content) else {
                    continue;
                };
                if raw_text.trim().is_empty() {
                    continue;
                }

                let text = if role == "user" && compact::is_summary_message(&raw_text) {
                    let suffix = raw_text
                        .strip_prefix(&summary_prefix)
                        .unwrap_or(raw_text.as_str())
                        .trim();
                    format!("Session summary:\n{suffix}")
                } else {
                    raw_text
                };

                let text = truncate_text_bytes(text.trim(), THREAD_MEMORY_MAX_MESSAGE_BYTES);
                if text.is_empty() {
                    continue;
                }

                let content_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                let value = serde_json::json!({
                    "type": "message",
                    "role": role,
                    "content": [{
                        "type": content_type,
                        "text": text,
                    }],
                });
                let cost = role.len().saturating_add(text.len()).saturating_add(96);
                candidates.push((cost, value));
            }
            ResponseItem::FunctionCall { name, call_id, .. } => {
                let value = serde_json::json!({
                    "type": "function_call",
                    "name": name,
                    "arguments": "{}",
                    "call_id": call_id,
                });
                let cost = name.len().saturating_add(call_id.len()).saturating_add(64);
                candidates.push((cost, value));
            }
            ResponseItem::CustomToolCall { call_id, name, .. } => {
                let value = serde_json::json!({
                    "type": "custom_tool_call",
                    "call_id": call_id,
                    "name": name,
                    "input": "",
                });
                let cost = name.len().saturating_add(call_id.len()).saturating_add(64);
                candidates.push((cost, value));
            }
            ResponseItem::LocalShellCall { .. }
            | ResponseItem::Reasoning { .. }
            | ResponseItem::FunctionCallOutput { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::GhostSnapshot { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::Other => {}
        }
    }

    take_last_values_with_byte_budget(
        candidates,
        THREAD_MEMORY_MAX_TRACE_ITEMS,
        THREAD_MEMORY_MAX_TRACE_BYTES,
    )
}

async fn update_thread_memory_after_compaction(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    compaction_summary: String,
    trace_items: Vec<Value>,
) {
    let Some(db) = sess.state_db() else {
        return;
    };

    let thread_id = sess.conversation_id;
    if !wait_for_thread_metadata(db.as_ref(), thread_id, "thread_memory_compaction").await {
        return;
    }

    let summary = compaction_summary.trim();
    if !summary.is_empty() {
        state_db::upsert_thread_memory(
            Some(db.as_ref()),
            thread_id,
            summary,
            summary,
            "thread_memory_compaction_fallback",
        )
        .await;
    }

    if trace_items.is_empty() {
        return;
    }

    let Some(output) = summarize_trace_items(&sess, &turn_context, thread_id, trace_items).await
    else {
        return;
    };

    if output.raw_memory.trim().is_empty() && output.memory_summary.trim().is_empty() {
        return;
    }

    state_db::upsert_thread_memory(
        Some(db.as_ref()),
        thread_id,
        output.raw_memory.trim(),
        output.memory_summary.trim(),
        "thread_memory_trace_summarize",
    )
    .await;
}

async fn update_thread_memory_after_turn(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    last_agent_message: Option<String>,
) {
    let Some(db) = sess.state_db() else {
        return;
    };

    let thread_id = sess.conversation_id;
    if !wait_for_thread_metadata(db.as_ref(), thread_id, "thread_memory_turn_complete").await {
        return;
    }

    let existing = state_db::get_thread_memory(
        Some(db.as_ref()),
        thread_id,
        "thread_memory_turn_complete_read",
    )
    .await;
    let missing_memory = existing.is_none();
    if let Some(ref memory) = existing {
        let age = Utc::now()
            .signed_duration_since(memory.updated_at)
            .num_seconds();
        if (0..THREAD_MEMORY_MIN_UPDATE_INTERVAL_SECS).contains(&age) {
            return;
        }
    }

    let history_snapshot = sess.clone_history().await;
    let trace_items = build_thread_memory_trace_items(history_snapshot.raw_items());
    if trace_items.is_empty() {
        return;
    }

    if missing_memory
        && let Some(summary) = last_agent_message.as_deref()
        && !summary.trim().is_empty()
    {
        let summary = summary.trim();
        state_db::upsert_thread_memory(
            Some(db.as_ref()),
            thread_id,
            summary,
            summary,
            "thread_memory_turn_complete_fallback",
        )
        .await;
    }

    let Some(output) = summarize_trace_items(&sess, &turn_context, thread_id, trace_items).await
    else {
        return;
    };

    if output.raw_memory.trim().is_empty() && output.memory_summary.trim().is_empty() {
        return;
    }

    state_db::upsert_thread_memory(
        Some(db.as_ref()),
        thread_id,
        output.raw_memory.trim(),
        output.memory_summary.trim(),
        "thread_memory_trace_summarize",
    )
    .await;
}

async fn summarize_trace_items(
    sess: &Session,
    turn_context: &TurnContext,
    thread_id: codex_protocol::ThreadId,
    trace_items: Vec<Value>,
) -> Option<codex_api::MemorySummarizeOutput> {
    if trace_items.is_empty() {
        return None;
    }

    let primary = sess
        .services
        .model_client
        .summarize_memories(
            vec![RawMemory {
                id: format!("trace_{thread_id}"),
                metadata: RawMemoryMetadata {
                    source_path: turn_context.cwd.display().to_string(),
                },
                items: trace_items.clone(),
            }],
            &turn_context.model_info,
            turn_context.reasoning_effort,
            &turn_context.session_telemetry,
        )
        .await;

    match primary {
        Ok(mut outputs) => match outputs.pop() {
            Some(output) if outputs.is_empty() => Some(output),
            _ => {
                warn!("unexpected memory trace summarize output length");
                None
            }
        },
        Err(err) => {
            if !matches!(err, crate::error::CodexErr::UnsupportedOperation(_)) {
                warn!("memory trace summarization failed: {err}");
                return None;
            }

            // Memory trace summarization is a Responses-only endpoint. Prefer the dedicated
            // Responses utility model override when configured. Otherwise, fall back to the
            // general utility model when it is Responses-compatible.
            let fallback_model_slug =
                utility_model::responses_utility_model_slug(turn_context.config.as_ref());

            let Some((utility_client, utility_model_info, provider_id)) = sess
                .utility_client_and_model_for_slug(
                    turn_context.config.as_ref(),
                    fallback_model_slug,
                )
                .await
            else {
                warn!("memory trace summarize fallback unavailable; skipping");
                return None;
            };

            warn!(
                primary_model = turn_context.model_info.slug.as_str(),
                primary_provider = turn_context.provider.name.as_str(),
                fallback_model = fallback_model_slug,
                fallback_provider_id = provider_id.as_str(),
                "memory trace summarize falling back to utility provider"
            );

            match utility_client
                .summarize_memories(
                    vec![RawMemory {
                        id: format!("trace_{thread_id}"),
                        metadata: RawMemoryMetadata {
                            source_path: turn_context.cwd.display().to_string(),
                        },
                        items: trace_items,
                    }],
                    &utility_model_info,
                    turn_context.reasoning_effort,
                    &turn_context.session_telemetry,
                )
                .await
            {
                Ok(mut outputs) => match outputs.pop() {
                    Some(output) if outputs.is_empty() => Some(output),
                    _ => {
                        warn!("unexpected memory trace summarize output length");
                        None
                    }
                },
                Err(err) => {
                    warn!("memory trace summarize fallback failed: {err}");
                    None
                }
            }
        }
    }
}

async fn wait_for_thread_metadata(
    db: &codex_state::StateRuntime,
    thread_id: codex_protocol::ThreadId,
    stage: &str,
) -> bool {
    for _ in 0..THREAD_MEMORY_THREAD_POLL_ATTEMPTS {
        match db.get_thread(thread_id).await {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(err) => {
                warn!("failed to read thread metadata during {stage}: {err}");
                return false;
            }
        }
        tokio::time::sleep(THREAD_MEMORY_THREAD_POLL_SLEEP).await;
    }

    warn!("thread metadata not yet available in sqlite during {stage}");
    false
}

fn take_last_values_with_byte_budget(
    values: Vec<(usize, Value)>,
    max_items: usize,
    max_bytes: usize,
) -> Vec<Value> {
    if max_items == 0 || max_bytes == 0 || values.is_empty() {
        return Vec::new();
    }

    let mut used = 0usize;
    let mut selected_rev = Vec::new();
    for (cost, value) in values.into_iter().rev().take(max_items) {
        if !selected_rev.is_empty() && used.saturating_add(cost) > max_bytes {
            break;
        }
        selected_rev.push(value);
        used = used.saturating_add(cost);
    }
    selected_rev.reverse();
    selected_rev
}

fn truncate_text_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let truncated = take_bytes_at_char_boundary(text, max_bytes);
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::ContentItem;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn build_thread_memory_trace_items_filters_outputs_and_reasoning() {
        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "hello".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "shell".to_string(),
                arguments: "{\"cmd\":\"echo hi\"}".to_string(),
                call_id: "call-1".to_string(),
                thought_signature: None,

                namespace: None,
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: Default::default(),
            },
            ResponseItem::Reasoning {
                id: "r1".to_string(),
                summary: Vec::new(),
                content: None,
                encrypted_content: None,
            },
        ];

        let trace = build_thread_memory_trace_items(&items);
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0]["type"], "message");
        assert_eq!(trace[1]["type"], "function_call");
        assert_eq!(trace[1]["arguments"], "{}");
    }

    #[test]
    fn build_thread_memory_trace_items_rewrites_summary_messages() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!("{}\nsummary body", compact::SUMMARY_PREFIX),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }];

        let trace = build_thread_memory_trace_items(&items);
        assert_eq!(
            trace,
            vec![json!({
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "Session summary:\nsummary body",
                }],
            })]
        );
    }
}
