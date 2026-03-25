use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelInstructionsVariables;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationMode;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::openai_models::default_input_modalities;

use crate::config::Config;
use crate::model_compat::is_anthropic_model_slug;
use crate::model_compat::is_gemma_model_slug;
use crate::model_compat::is_grok_model_slug;
use crate::model_compat::normalized_grok_model_slug;
use crate::truncate::approx_bytes_for_tokens;
use codex_features::Feature;
use tracing::warn;

pub const BASE_INSTRUCTIONS: &str = include_str!("../../prompt.md");
const BASE_INSTRUCTIONS_WITH_APPLY_PATCH: &str =
    include_str!("../../prompt_with_apply_patch_instructions.md");

const GPT_5_CODEX_INSTRUCTIONS: &str = include_str!("../../gpt_5_codex_prompt.md");
const GPT_5_1_INSTRUCTIONS: &str = include_str!("../../gpt_5_1_prompt.md");
const GPT_5_2_INSTRUCTIONS: &str = include_str!("../../gpt_5_2_prompt.md");
const GPT_5_2_CODEX_INSTRUCTIONS: &str = include_str!("../../gpt-5.2-codex_prompt.md");
const GPT_5_3_CODEX_SPARK_INSTRUCTIONS: &str = include_str!("../../gpt-5.3-codex-spark_prompt.md");

const GEMINI_INSTRUCTIONS: &str = include_str!("../../gemini_prompt.md");
const GROK_INSTRUCTIONS: &str = include_str!("../../grok_prompt.md");
const CLAUDE_INSTRUCTIONS: &str = include_str!("../../claude_prompt.md");

pub(crate) const CONTEXT_WINDOW_1M: i64 = 1_048_576;
pub(crate) const CONTEXT_WINDOW_8K: i64 = 8_192;
pub(crate) const CONTEXT_WINDOW_272K: i64 = 272_000;
pub(crate) const CONTEXT_WINDOW_128K: i64 = 128_000;
pub(crate) const CONTEXT_WINDOW_256K: i64 = 256_000;
pub(crate) const CONTEXT_WINDOW_2M: i64 = 2_000_000;
pub(crate) const CONTEXT_WINDOW_200K: i64 = 200_000;
const GPT_5_2_CODEX_INSTRUCTIONS_TEMPLATE: &str =
    include_str!("../../templates/model_instructions/gpt-5.2-codex_instructions_template.md");
const GPT_5_3_CODEX_SPARK_INSTRUCTIONS_TEMPLATE: &str =
    include_str!("../../templates/model_instructions/gpt-5.3-codex-spark_instructions_template.md");

const GPT_5_2_CODEX_PERSONALITY_FRIENDLY: &str =
    include_str!("../../templates/personalities/gpt-5.2-codex_friendly.md");
const GPT_5_2_CODEX_PERSONALITY_PRAGMATIC: &str =
    include_str!("../../templates/personalities/gpt-5.2-codex_pragmatic.md");

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
            availability_nux: None,
            upgrade: None,
            base_instructions: BASE_INSTRUCTIONS.to_string(),
            model_messages: None,
            supports_reasoning_summaries: false,
            default_reasoning_summary: ReasoningSummary::Auto,
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            web_search_tool_type: WebSearchToolType::Text,
            truncation_policy: TruncationPolicyConfig::bytes(10_000),
            supports_parallel_tool_calls: false,
            supports_image_detail_original: false,
            context_window: Some(CONTEXT_WINDOW_272K),
            auto_compact_token_limit: None,
            effective_context_window_percent: 95,
            experimental_supported_tools: Vec::new(),
            input_modalities: default_input_modalities(),
            used_fallback_model_metadata: false,
            supports_search_tool: false,
        };

        $(
            model.$key = $value;
        )*
        model
    }};
}

pub(crate) fn with_config_overrides(mut model: ModelInfo, config: &Config) -> ModelInfo {
    if let Some(supports_reasoning_summaries) = config.model_supports_reasoning_summaries
        && supports_reasoning_summaries
    {
        model.supports_reasoning_summaries = true;
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

/// Returns reasoning effort presets for Low, Medium, High.
fn supported_reasoning_level_low_medium_high() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Fast responses with lighter reasoning".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Balances speed and reasoning depth for everyday tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Greater reasoning depth for complex problems".to_string(),
        },
    ]
}

/// Returns reasoning effort presets for Low, Medium, High, XHigh.
fn supported_reasoning_level_low_medium_high_xhigh() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Fast responses with lighter reasoning".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Balances speed and reasoning depth for everyday tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Greater reasoning depth for complex problems".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::XHigh,
            description: "Extra high reasoning depth for complex problems".to_string(),
        },
    ]
}

/// Returns reasoning effort presets for Low, Medium, High (non-codex models).
fn supported_reasoning_level_low_medium_high_non_codex() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Fast responses with lighter reasoning".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Balances speed and reasoning depth for everyday tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Greater reasoning depth for complex problems".to_string(),
        },
    ]
}

/// Returns reasoning effort presets for Low, Medium, High, XHigh (non-codex models).
fn supported_reasoning_level_low_medium_high_xhigh_non_codex() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Fast responses with lighter reasoning".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Balances speed and reasoning depth for everyday tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Greater reasoning depth for complex problems".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::XHigh,
            description: "Extra high reasoning depth for complex problems".to_string(),
        },
    ]
}

fn context_window_for_grok_slug(slug: &str) -> i64 {
    let Some(grok_slug) = normalized_grok_model_slug(slug) else {
        return CONTEXT_WINDOW_256K;
    };

    if grok_slug.starts_with("grok-4-1-fast") || grok_slug.starts_with("grok-4-fast") {
        CONTEXT_WINDOW_2M
    } else if grok_slug.starts_with("grok-4") || grok_slug.starts_with("grok-code-fast") {
        CONTEXT_WINDOW_256K
    } else if grok_slug.starts_with("grok-2-vision") {
        32_768
    } else if grok_slug.starts_with("grok-2") || grok_slug.starts_with("grok-3") {
        131_072
    } else {
        CONTEXT_WINDOW_256K
    }
}

fn context_window_for_claude_slug(normalized_slug: &str, original_slug: &str) -> i64 {
    if normalized_slug.contains("haiku") || original_slug.starts_with("antigravity/") {
        CONTEXT_WINDOW_200K
    } else {
        CONTEXT_WINDOW_1M
    }
}

fn normalize_slug_for_fallback_model_metadata(slug: &str) -> &str {
    [
        "openai/",
        "google/",
        "anthropic/",
        "xai/",
        "antigravity/",
        "antigravity-gemini/",
        "antigravity-anthropic/",
    ]
    .iter()
    .find_map(|prefix| slug.strip_prefix(prefix))
    .unwrap_or(slug)
}

// todo(aibrahim): remove most of the entries here when enabling models.json
pub(crate) fn find_model_info_for_slug(slug: &str) -> ModelInfo {
    let normalized_slug = normalize_slug_for_fallback_model_metadata(slug);

    if normalized_slug.starts_with("o3") || normalized_slug.starts_with("o4-mini") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            supports_reasoning_summaries: true,
            context_window: Some(200_000),
        )
    } else if normalized_slug.starts_with("codex-mini-latest") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            shell_type: ConfigShellToolType::Local,
            supports_reasoning_summaries: true,
            context_window: Some(200_000),
        )
    } else if normalized_slug.starts_with("gpt-4.1") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            supports_reasoning_summaries: false,
            context_window: Some(1_047_576),
        )
    } else if normalized_slug.starts_with("gpt-oss") {
        model_info!(
            slug,
            apply_patch_tool_type: Some(ApplyPatchToolType::Function),
            context_window: Some(96_000),
        )
    } else if normalized_slug.starts_with("gpt-4o") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            supports_reasoning_summaries: false,
            context_window: Some(128_000),
        )
    } else if normalized_slug.starts_with("gpt-3.5") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            supports_reasoning_summaries: false,
            context_window: Some(16_385),
        )
    } else if normalized_slug.starts_with("test-gpt-5") {
        model_info!(
            slug,
            base_instructions: GPT_5_CODEX_INSTRUCTIONS.to_string(),
            experimental_supported_tools: vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string(),
                "test_sync_tool".to_string(),
            ],
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: true,
            shell_type: ConfigShellToolType::ShellCommand,
            support_verbosity: true,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
        )
    } else if normalized_slug.starts_with("exp-codex") || normalized_slug.starts_with("codex-1p") {
        model_info!(
            slug,
            base_instructions: GPT_5_2_CODEX_INSTRUCTIONS.to_string(),
            model_messages: Some(ModelMessages {
                instructions_template: Some(GPT_5_2_CODEX_INSTRUCTIONS_TEMPLATE.to_string()),
                instructions_variables: Some(ModelInstructionsVariables {
                    personality_default: Some(String::new()),
                    personality_friendly: Some(GPT_5_2_CODEX_PERSONALITY_FRIENDLY.to_string()),
                    personality_pragmatic: Some(GPT_5_2_CODEX_PERSONALITY_PRAGMATIC.to_string()),
                }),
            }),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: true,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_272K),
        )
    } else if normalized_slug.starts_with("exp-") {
        model_info!(
            slug,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            supports_reasoning_summaries: true,
            support_verbosity: true,
            default_verbosity: Some(Verbosity::Low),
            base_instructions: BASE_INSTRUCTIONS.to_string(),
            default_reasoning_level: Some(ReasoningEffort::Medium),
            truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
            shell_type: ConfigShellToolType::UnifiedExec,
            supports_parallel_tool_calls: true,
            context_window: Some(CONTEXT_WINDOW_272K),
        )
    } else if normalized_slug.starts_with("gpt-5.3-codex-spark") {
        model_info!(
            slug,
            base_instructions: GPT_5_3_CODEX_SPARK_INSTRUCTIONS.to_string(),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            shell_type: ConfigShellToolType::ShellCommand,
            supported_in_api: true,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_128K),
            supported_reasoning_levels: supported_reasoning_level_low_medium_high_xhigh(),
            input_modalities: vec![InputModality::Text],
            model_messages: Some(ModelMessages {
                instructions_template: Some(GPT_5_3_CODEX_SPARK_INSTRUCTIONS_TEMPLATE.to_string()),
                instructions_variables: Some(ModelInstructionsVariables {
                    personality_default: Some(String::new()),
                    personality_friendly: Some(GPT_5_2_CODEX_PERSONALITY_FRIENDLY.to_string()),
                    personality_pragmatic: Some(GPT_5_2_CODEX_PERSONALITY_PRAGMATIC.to_string()),
                }),
            }),
        )
    } else if normalized_slug.starts_with("gpt-5.3-codex")
        || normalized_slug.starts_with("gpt-5.2-codex")
        || normalized_slug.starts_with("bengalfox")
    {
        model_info!(
            slug,
            base_instructions: GPT_5_2_CODEX_INSTRUCTIONS.to_string(),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: true,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_272K),
            supported_reasoning_levels: supported_reasoning_level_low_medium_high_xhigh(),
            model_messages: Some(ModelMessages {
                instructions_template: Some(GPT_5_2_CODEX_INSTRUCTIONS_TEMPLATE.to_string()),
                instructions_variables: Some(ModelInstructionsVariables {
                    personality_default: Some(String::new()),
                    personality_friendly: Some(GPT_5_2_CODEX_PERSONALITY_FRIENDLY.to_string()),
                    personality_pragmatic: Some(GPT_5_2_CODEX_PERSONALITY_PRAGMATIC.to_string()),
                }),
            }),
        )
    } else if normalized_slug.starts_with("gpt-5.1-codex")
        || normalized_slug.starts_with("gpt-5-codex")
        || normalized_slug.starts_with("codex-")
    {
        model_info!(
            slug,
            base_instructions: GPT_5_CODEX_INSTRUCTIONS.to_string(),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: false,
            supports_reasoning_summaries: true,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_272K),
            supported_reasoning_levels: supported_reasoning_level_low_medium_high(),
        )
    } else if normalized_slug.starts_with("gpt-5.2") || normalized_slug.starts_with("boomslang") {
        model_info!(
            slug,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            supports_reasoning_summaries: true,
            support_verbosity: true,
            default_verbosity: Some(Verbosity::Low),
            base_instructions: GPT_5_2_INSTRUCTIONS.to_string(),
            default_reasoning_level: Some(ReasoningEffort::Medium),
            truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            context_window: Some(CONTEXT_WINDOW_272K),
            supported_reasoning_levels: supported_reasoning_level_low_medium_high_xhigh_non_codex(),
        )
    } else if normalized_slug.starts_with("gpt-5.1") {
        model_info!(
            slug,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            supports_reasoning_summaries: true,
            support_verbosity: true,
            default_verbosity: Some(Verbosity::Low),
            base_instructions: GPT_5_1_INSTRUCTIONS.to_string(),
            default_reasoning_level: Some(ReasoningEffort::Medium),
            truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            context_window: Some(CONTEXT_WINDOW_272K),
            supported_reasoning_levels: supported_reasoning_level_low_medium_high_non_codex(),
        )
    } else if normalized_slug.starts_with("gpt-5") {
        model_info!(
            slug,
            base_instructions: BASE_INSTRUCTIONS_WITH_APPLY_PATCH.to_string(),
            shell_type: ConfigShellToolType::Default,
            supports_reasoning_summaries: true,
            support_verbosity: true,
            supported_reasoning_levels: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Minimal,
                    description: "Fastest responses with little reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            truncation_policy: TruncationPolicyConfig::bytes(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_272K),
        )
    } else if normalized_slug.starts_with("gemini-") {
        model_info!(
            slug,
            base_instructions: GEMINI_INSTRUCTIONS.to_string(),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_1M),
            default_reasoning_level: Some(ReasoningEffort::High),
            experimental_supported_tools: vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string(),
            ],
        )
    } else if is_gemma_model_slug(normalized_slug) {
        // Local Gemma deployments often run with a smaller llama.cpp context
        // window than Gemini cloud models. Use a leaner system prompt and a
        // realistic default window so requests fit by default.
        model_info!(
            slug,
            base_instructions: GPT_5_CODEX_INSTRUCTIONS.to_string(),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(CONTEXT_WINDOW_8K),
            default_reasoning_level: Some(ReasoningEffort::Medium),
            experimental_supported_tools: vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string(),
            ],
        )
    } else if is_anthropic_model_slug(normalized_slug) {
        model_info!(
            slug,
            base_instructions: format!(
                "{BASE_INSTRUCTIONS_WITH_APPLY_PATCH}\n\n{CLAUDE_INSTRUCTIONS}"
            ),
            apply_patch_tool_type: Some(ApplyPatchToolType::Function),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(context_window_for_claude_slug(normalized_slug, slug)),
            default_reasoning_level: Some(ReasoningEffort::High),
            input_modalities: vec![InputModality::Text, InputModality::Image],
            experimental_supported_tools: vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string(),
            ],
        )
    } else if is_grok_model_slug(normalized_slug) {
        // Grok speaks an OpenAI-compatible Responses API, but tool support differs from OpenAI:
        // - `custom` (freeform) tools are rejected by xAI (so use JSON apply_patch if enabled)
        // - `web_search` does not accept `external_web_access` toggles (cached/live)
        // - `reasoning.effort` support is model-dependent (see model_compat.rs)
        model_info!(
            slug,
            base_instructions: format!("{BASE_INSTRUCTIONS}\n\n{GROK_INSTRUCTIONS}"),
            apply_patch_tool_type: Some(ApplyPatchToolType::Function),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: true,
            support_verbosity: true,
            default_verbosity: Some(Verbosity::Low),
            truncation_policy: TruncationPolicyConfig::tokens(/*limit*/ 10_000),
            context_window: Some(context_window_for_grok_slug(normalized_slug)),
            supported_reasoning_levels: Vec::new(),
            experimental_supported_tools: vec![
                "grep_files".to_string(),
                "list_dir".to_string(),
                "read_file".to_string(),
            ],
        )
    } else {
        warn!("Unknown model {slug} is used. This will degrade the performance of Codex.");
        model_info!(
            slug,
            context_window: None,
            supported_reasoning_levels: Vec::new(),
            default_reasoning_level: None,
            used_fallback_model_metadata: true,
        )
    }
}

pub(crate) fn model_info_from_slug(slug: &str) -> ModelInfo {
    find_model_info_for_slug(slug)
}

#[cfg(test)]
#[path = "model_info_tests.rs"]
mod tests;
