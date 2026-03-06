use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use codex_api::ResponseEvent;
use codex_hooks::EntireSummary;
use codex_hooks::EntireSummaryInput;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use futures::StreamExt;
use serde_json::json;

use crate::Prompt;
use crate::codex::Session;
use crate::codex::TurnContext;

pub(crate) async fn generate_entire_summary(
    session: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    input: &EntireSummaryInput,
) -> Result<EntireSummary> {
    let prompt = Prompt {
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: codex_hooks::build_why_prompt(input),
            }],
            end_turn: None,
            phase: None,
        }],
        tools: Vec::new(),
        parallel_tool_calls: false,
        base_instructions: BaseInstructions {
            text: "You are a helpful assistant that generates concise WHY-focused summaries of AI coding sessions.".to_string(),
        },
        personality: None,
        output_schema: Some(output_schema()),
    };

    let mut client_session = session
        .services
        .model_client
        .new_session_for_provider(&turn_context.provider);
    let turn_metadata_header = turn_context.turn_metadata_state.current_header_value();
    let mut stream = client_session
        .stream(
            &prompt,
            &turn_context.model_info,
            &turn_context.otel_manager,
            turn_context.reasoning_effort,
            turn_context.reasoning_summary,
            turn_context.config.service_tier,
            turn_metadata_header.as_deref(),
        )
        .await
        .context("failed to start Entire summary stream")?;

    let mut response = String::new();
    while let Some(message) = stream.next().await.transpose()? {
        match message {
            ResponseEvent::OutputTextDelta(delta) => response.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if response.is_empty()
                    && let ResponseItem::Message { content, .. } = item
                    && let Some(text) = crate::compact::content_items_to_text(&content)
                {
                    response.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }

    if response.is_empty() {
        anyhow::bail!("Entire summary model returned empty response");
    }

    serde_json::from_str(&response).context("failed to parse Entire summary response")
}

pub(crate) fn build_legacy_notify_summary_text(summary: &EntireSummary) -> Option<String> {
    if !summary.is_meaningful {
        return None;
    }

    let motivation = summary
        .motivation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let outcome = summary
        .outcome
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let approach = summary
        .approach
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let challenges = summary
        .challenges
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("None");
    let tradeoffs = summary
        .tradeoffs
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("None");

    let summary_text = format!(
        "{motivation} → {outcome}\n\nMotivation: {motivation}\nApproach: {approach}\nChallenges: {challenges}\nTradeoffs: {tradeoffs}"
    );
    Some(codex_utils_string::take_bytes_at_char_boundary(&summary_text, 4000).to_string())
}

fn output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "is_meaningful": { "type": "boolean" },
            "motivation": { "type": ["string", "null"] },
            "approach": { "type": ["string", "null"] },
            "challenges": { "type": ["string", "null"] },
            "tradeoffs": { "type": ["string", "null"] },
            "outcome": { "type": ["string", "null"] }
        },
        "required": [
            "is_meaningful",
            "motivation",
            "approach",
            "challenges",
            "tradeoffs",
            "outcome"
        ],
        "additionalProperties": false
    })
}
