use super::*;
use crate::config::test_config;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
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
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_256K));
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
fn grok_fast_reasoning_models_use_2m_context_window() {
    let model = find_model_info_for_slug("grok-4-1-fast-reasoning");

    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_2M));
}

#[test]
fn gemma_models_use_lean_defaults_with_medium_reasoning() {
    let model = find_model_info_for_slug("gemma-3n");

    assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
    assert!(model.supports_parallel_tool_calls);
    assert!(!model.supports_reasoning_summaries);
    assert!(!model.support_verbosity);
    assert_eq!(model.default_reasoning_level, Some(ReasoningEffort::Medium));
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_8K));
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

#[test]
fn claude_models_use_1m_context_with_function_apply_patch() {
    let model = find_model_info_for_slug("claude-opus-4-6");

    assert_eq!(
        model.apply_patch_tool_type,
        Some(ApplyPatchToolType::Function)
    );
    assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
    assert!(model.supports_parallel_tool_calls);
    assert!(
        model
            .base_instructions
            .contains("Claude provider addendum for Codex CLI."),
        "claude model should include Claude-specific prompt addendum"
    );
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_1M));
    assert_eq!(model.default_reasoning_level, Some(ReasoningEffort::High));
    assert_eq!(
        model.input_modalities,
        vec![InputModality::Text, InputModality::Image]
    );
}

#[test]
fn antigravity_gemini_models_reuse_gemini_metadata_without_fallback() {
    let model = find_model_info_for_slug("antigravity/gemini-3.1-pro-preview");

    assert_eq!(model.slug, "antigravity/gemini-3.1-pro-preview".to_string());
    assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
    assert!(!model.used_fallback_model_metadata);
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_1M));
    assert_eq!(model.default_reasoning_level, Some(ReasoningEffort::High));
    assert!(model.supports_parallel_tool_calls);
    assert_eq!(
        model.experimental_supported_tools,
        vec![
            "grep_files".to_string(),
            "list_dir".to_string(),
            "read_file".to_string()
        ]
    );
}

#[test]
fn antigravity_gpt_oss_models_reuse_gpt_oss_metadata_without_fallback() {
    let model = find_model_info_for_slug("antigravity/gpt-oss-120b-medium");

    assert_eq!(
        model.apply_patch_tool_type,
        Some(ApplyPatchToolType::Function)
    );
    assert!(!model.used_fallback_model_metadata);
    assert_eq!(model.context_window, Some(96_000));
}

#[test]
fn antigravity_anthropic_models_reuse_claude_metadata_without_fallback() {
    let model = find_model_info_for_slug("antigravity-anthropic/claude-sonnet-4-6");

    assert_eq!(
        model.apply_patch_tool_type,
        Some(ApplyPatchToolType::Function)
    );
    assert!(!model.used_fallback_model_metadata);
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_1M));
}

#[test]
fn spark_model_uses_low_latency_text_only_defaults() {
    let model = find_model_info_for_slug("gpt-5.3-codex-spark|[pro]");

    assert_eq!(model.shell_type, ConfigShellToolType::ShellCommand);
    assert!(model.supports_parallel_tool_calls);
    assert!(!model.supports_reasoning_summaries);
    assert!(model.supported_in_api);
    assert_eq!(model.context_window, Some(CONTEXT_WINDOW_128K));
    assert_eq!(model.input_modalities, vec![InputModality::Text]);
    assert!(
        model
            .base_instructions
            .contains("fast, iterative coding assistance"),
        "spark model should use the spark-specific prompt"
    );
}

#[test]
fn gpt_5_family_uses_272k_context_window_defaults() {
    let gpt_53_codex = find_model_info_for_slug("gpt-5.3-codex");
    let gpt_52_codex = find_model_info_for_slug("gpt-5.2-codex");
    let gpt_51_codex = find_model_info_for_slug("gpt-5.1-codex");
    let gpt_52 = find_model_info_for_slug("gpt-5.2");
    let gpt_51 = find_model_info_for_slug("gpt-5.1");
    let gpt_5 = find_model_info_for_slug("gpt-5");

    assert_eq!(gpt_53_codex.context_window, Some(CONTEXT_WINDOW_272K));
    assert_eq!(gpt_52_codex.context_window, Some(CONTEXT_WINDOW_272K));
    assert_eq!(gpt_51_codex.context_window, Some(CONTEXT_WINDOW_272K));
    assert_eq!(gpt_52.context_window, Some(CONTEXT_WINDOW_272K));
    assert_eq!(gpt_51.context_window, Some(CONTEXT_WINDOW_272K));
    assert_eq!(gpt_5.context_window, Some(CONTEXT_WINDOW_272K));
}

#[test]
fn reasoning_summaries_override_true_enables_support() {
    let model = model_info_from_slug("unknown-model");
    let mut config = test_config();
    config.model_supports_reasoning_summaries = Some(true);

    let updated = with_config_overrides(model.clone(), &config);
    let mut expected = model;
    expected.supports_reasoning_summaries = true;

    assert_eq!(updated, expected);
}

#[test]
fn reasoning_summaries_override_false_does_not_disable_support() {
    let mut model = model_info_from_slug("unknown-model");
    model.supports_reasoning_summaries = true;
    let mut config = test_config();
    config.model_supports_reasoning_summaries = Some(false);

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated, model);
}

#[test]
fn reasoning_summaries_override_false_is_noop_when_model_is_false() {
    let model = model_info_from_slug("unknown-model");
    let mut config = test_config();
    config.model_supports_reasoning_summaries = Some(false);

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated, model);
}
