use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelInstructionsVariables;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationMode;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::default_input_modalities;

use crate::config::Config;
use crate::features::Feature;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::is_grok_model_slug;
use crate::truncate::approx_bytes_for_tokens;
use tracing::warn;

pub const BASE_INSTRUCTIONS: &str = include_str!("../../prompt.md");
const BASE_INSTRUCTIONS_WITH_APPLY_PATCH: &str =
    include_str!("../../prompt_with_apply_patch_instructions.md");

const GPT_5_CODEX_INSTRUCTIONS: &str = include_str!("../../gpt_5_codex_prompt.md");
const GPT_5_1_INSTRUCTIONS: &str = include_str!("../../gpt_5_1_prompt.md");
const GPT_5_2_INSTRUCTIONS: &str = include_str!("../../gpt_5_2_prompt.md");
const GPT_5_2_CODEX_INSTRUCTIONS: &str = include_str!("../../gpt-5.2-codex_prompt.md");

const GEMINI_INSTRUCTIONS: &str = include_str!("../../gemini_prompt.md");
const GROK_INSTRUCTIONS: &str = include_str!("../../grok_prompt.md");

pub(crate) const CONTEXT_WINDOW_1M: i64 = 1_048_576;
pub(crate) const CONTEXT_WINDOW_8K: i64 = 8_192;
const GPT_5_2_CODEX_INSTRUCTIONS_TEMPLATE: &str =
    include_str!("../../templates/model_instructions/gpt-5.2-codex_instructions_template.md");

const GPT_5_2_CODEX_PERSONALITY_FRIENDLY: &str =
    include_str!("../../templates/personalities/gpt-5.2-codex_friendly.md");
const GPT_5_2_CODEX_PERSONALITY_PRAGMATIC: &str =
    include_str!("../../templates/personalities/gpt-5.2-codex_pragmatic.md");

pub(crate) const CONTEXT_WINDOW_272K: i64 = 272_000;

macro_rules! model_info {
    (
        $slug:expr $(, $key:ident : $value:expr )* $(,)?
    ) => {{
        #[allow(unused_mut)]
        let mut model = ModelInfo {
            slug: $slug.to_string(),
            display_name: $slug.to_string(),
            description: None,
            // This is primarily used when remote metadata is available. When running
            // offline, core generally omits the effort field unless explicitly
            // configured by the user.
            default_reasoning_level: None,
            supported_reasoning_levels: supported_reasoning_level_low_medium_high(),
            shell_type: ConfigShellToolType::Default,
            visibility: ModelVisibility::None,
            supported_in_api: true,
            priority: 99,
            upgrade: None,
            base_instructions: BASE_INSTRUCTIONS.to_string(),
            model_messages: None,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            truncation_policy: TruncationPolicyConfig::bytes(10_000),
            supports_parallel_tool_calls: false,
            context_window: Some(CONTEXT_WINDOW_272K),
            auto_compact_token_limit: None,
            effective_context_window_percent: 95,
            experimental_supported_tools: Vec::new(),
            input_modalities: default_input_modalities(),
        };

        $(
            model.$key = $value;
        )*
        model
    }};
}

pub(crate) fn with_config_overrides(mut model: ModelInfo, config: &Config) -> ModelInfo {
    if let Some(supports_reasoning_summaries) = config.model_supports_reasoning_summaries {
        model.supports_reasoning_summaries = supports_reasoning_summaries;
    }
    if let Some(context_window) = config.model_context_window {
        model.context_window = Some(context_window);
    }
    if let Some(auto_compact_token_limit) = config.model_auto_compact_token_limit {
        model.auto_compact_token_limit = Some(auto_compact_token_limit);
    }
    if let Some(token_limit) = config.tool_output_token_limit {
        model.truncation_policy = match model.truncation_policy.mode {
            TruncationMode::Bytes => {
                let byte_limit =
                    i64::try_from(approx_bytes_for_tokens(token_limit)).unwrap_or(i64::MAX);
                TruncationPolicyConfig::bytes(byte_limit)
            }
            TruncationMode::Tokens => {
                let limit = i64::try_from(token_limit).unwrap_or(i64::MAX);
                TruncationPolicyConfig::tokens(limit)
            }
        };
    }

    if let Some(base_instructions) = &config.base_instructions {
        model.base_instructions = base_instructions.clone();
        model.model_messages = None;
    } else if !config.features.enabled(Feature::Personality) {
        model.model_messages = None;
    }

    model
}

/// Build a minimal fallback model descriptor for missing/unknown slugs.
pub(crate) fn model_info_from_slug(slug: &str) -> ModelInfo {
    warn!("Unknown model {slug} is used. This will use fallback model metadata.");
    ModelInfo {
        slug: slug.to_string(),
        display_name: slug.to_string(),
        description: None,
        default_reasoning_level: None,
        supported_reasoning_levels: Vec::new(),
        shell_type: ConfigShellToolType::Default,
        visibility: ModelVisibility::None,
        supported_in_api: true,
        priority: 99,
        upgrade: None,
        base_instructions: BASE_INSTRUCTIONS.to_string(),
        model_messages: local_personality_messages_for_slug(slug),
        supports_reasoning_summaries: false,
        support_verbosity: false,
        default_verbosity: None,
        apply_patch_tool_type: None,
        truncation_policy: TruncationPolicyConfig::bytes(10_000),
        supports_parallel_tool_calls: false,
        context_window: Some(272_000),
        auto_compact_token_limit: None,
        effective_context_window_percent: 95,
        experimental_supported_tools: Vec::new(),
        input_modalities: default_input_modalities(),
        prefer_websockets: false,
    }
}

#[cfg(test)]
mod tests {
    use super::find_model_info_for_slug;
    use codex_protocol::openai_models::ApplyPatchToolType;
    use codex_protocol::openai_models::ConfigShellToolType;
    use codex_protocol::openai_models::ReasoningEffort;
    use pretty_assertions::assert_eq;

    #[test]
    fn grok_models_use_function_apply_patch() {
        let model = find_model_info_for_slug("grok-4-latest");

        assert_eq!(
            model.apply_patch_tool_type,
            Some(ApplyPatchToolType::Function)
        );
        assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
        assert!(model.supports_parallel_tool_calls);
        assert!(model.supports_reasoning_summaries);
        assert!(model.support_verbosity);
        assert!(model.supported_reasoning_levels.is_empty());
        assert!(
            model
                .base_instructions
                .contains("Grok provider addendum for Codex CLI."),
            "grok model should include Grok-specific prompt addendum"
        );
        assert!(
            model.base_instructions.contains("`web_search`"),
            "grok model should include web_search guidance"
        );
    }

    #[test]
    fn gemma_models_use_lean_defaults_with_medium_reasoning() {
        let model = find_model_info_for_slug("gemma-3n");

        assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
        assert!(model.supports_parallel_tool_calls);
        assert!(!model.supports_reasoning_summaries);
        assert!(!model.support_verbosity);
        assert_eq!(model.default_reasoning_level, Some(ReasoningEffort::Medium));
        assert_eq!(model.context_window, Some(super::CONTEXT_WINDOW_8K));
        assert!(
            model
                .base_instructions
                .contains("You are Codex, based on GPT-5."),
            "gemma model should use the lean codex prompt"
        );
        assert_eq!(
            model.experimental_supported_tools,
            vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string()
            ]
        );
    }
}
