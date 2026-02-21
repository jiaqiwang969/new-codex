//! Entire summary generation using utility models.
//!
//! This module handles the generation of WHY-focused summaries for Entire checkpoints
//! by calling the configured entire_summary_model via the utility_model system.

use crate::Prompt;
use crate::client::ModelClient;
use crate::config::Config;
use crate::models_manager::manager::ModelsManager;
use crate::utility_model;
use anyhow::Context;
use anyhow::Result;
use codex_api::ResponseEvent;
use codex_hooks::EntireSummary;
use codex_hooks::EntireSummaryInput;
use codex_protocol::ThreadId;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;
use futures::StreamExt;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// Generates an Entire summary using the configured model.
///
/// This function:
/// 1. Resolves the model slug from config (entire_summary_model -> model_sub -> default)
/// 2. Gets the appropriate model client and provider
/// 3. Calls the model with a structured JSON schema
/// 4. Parses and returns the summary
pub async fn generate_entire_summary(
    input: &EntireSummaryInput,
    base_client: &ModelClient,
    models_manager: &Arc<ModelsManager>,
    config: &Config,
) -> Result<EntireSummary> {
    // Resolve model slug with fallback chain
    let model_slug = config
        .memories
        .entire_summary_model
        .as_deref()
        .or(config.model_sub.as_deref())
        .unwrap_or(crate::DEFAULT_ENTIRE_SUMMARY_MODEL);

    // Get model client and info
    let (model_client, model_info, _provider_id) =
        utility_model::client_and_model_for_slug(base_client, models_manager, config, model_slug)
            .await
            .context("Failed to get model client for entire_summary_model")?;

    // Build prompt and schema
    let prompt = codex_hooks::build_why_prompt(input);
    let schema = output_schema();

    // Build prompt using the same structure as memory phase1
    let prompt_struct = Prompt {
        input: vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: prompt }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }],
        tools: Vec::new(),
        parallel_tool_calls: false,
        base_instructions: BaseInstructions {
            text: "You are a helpful assistant that generates concise WHY-focused summaries of AI coding sessions.".to_string(),
        },
        personality: None,
        output_schema: Some(schema),
        reference_images: Vec::new(),
        image_size: None,
        aspect_ratio: None,
    };

    let mut client_session = model_client.new_session();
    let mut stream = client_session
        .stream(
            &prompt_struct,
            &model_info,
            &codex_otel::OtelManager::new(
                ThreadId::new(),
                model_slug,
                model_slug,
                None,
                None,
                None,
                "entire-summary".to_string(),
                false,
                "cli".to_string(),
                SessionSource::Cli,
            ),
            None,
            codex_protocol::config_types::ReasoningSummary::default(),
            None,
        )
        .await
        .context("Failed to start model stream")?;

    let mut accumulated_text = String::new();

    while let Some(message) = stream.next().await.transpose()? {
        match message {
            ResponseEvent::OutputTextDelta(delta) => {
                accumulated_text.push_str(&delta);
            }
            ResponseEvent::OutputItemDone(item) => {
                if accumulated_text.is_empty()
                    && let ResponseItem::Message { content, .. } = item
                    && let Some(text) = crate::compact::content_items_to_text(&content)
                {
                    accumulated_text.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => {
                break;
            }
            _ => {}
        }
    }

    if accumulated_text.is_empty() {
        return Err(anyhow::anyhow!("Model returned empty response"));
    }

    let summary: EntireSummary = serde_json::from_str(&accumulated_text)
        .context("Failed to parse model response as EntireSummary")?;

    Ok(summary)
}

/// Generates and saves an Entire summary asynchronously.
///
/// This is a fire-and-forget operation that spawns a background task.
/// Errors are logged but don't block the main flow.
pub fn generate_and_save_summary_async(
    input: EntireSummaryInput,
    checkpoint_id: String,
    repo_root: std::path::PathBuf,
    base_client: ModelClient,
    models_manager: Arc<ModelsManager>,
    config: Config,
) {
    tokio::spawn(async move {
        match generate_entire_summary(&input, &base_client, &models_manager, &config).await {
            Ok(summary) => {
                if let Err(e) =
                    codex_hooks::save_summary(&repo_root, &checkpoint_id, &summary).await
                {
                    tracing::warn!("Failed to save Entire summary for {}: {}", checkpoint_id, e);
                } else {
                    tracing::info!("Generated Entire summary for checkpoint {}", checkpoint_id);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to generate Entire summary for {}: {}",
                    checkpoint_id,
                    e
                );
            }
        }
    });
}

/// JSON schema for the summary output.
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
        "required": ["is_meaningful", "motivation", "approach", "challenges", "tradeoffs", "outcome"],
        "additionalProperties": false
    })
}

/// Attempts to load an existing summary for a checkpoint.
pub async fn load_summary_if_exists(
    repo_root: &Path,
    checkpoint_id: &str,
) -> Option<EntireSummary> {
    codex_hooks::load_summary(repo_root, checkpoint_id)
        .await
        .ok()
        .flatten()
}
