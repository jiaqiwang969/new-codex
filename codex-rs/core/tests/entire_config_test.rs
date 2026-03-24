use codex_core::config::types::MemoriesConfig;
use core_test_support::load_default_config_for_test;
use tempfile::TempDir;

#[test]
fn test_entire_summary_model_fallback_chain() {
    // Test 1: Explicit entire_summary_model takes precedence
    let entire_summary_model = Some("claude-opus-4-6");
    let model_sub = Some("claude-sonnet-4-6");

    let effective = entire_summary_model
        .or(model_sub)
        .unwrap_or(codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL);

    assert_eq!(effective, "claude-opus-4-6");

    // Test 2: Inherit from model_sub
    let entire_summary_model: Option<&str> = None;
    let model_sub = Some("claude-sonnet-4-6");

    let effective = entire_summary_model
        .or(model_sub)
        .unwrap_or(codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL);

    assert_eq!(effective, "claude-sonnet-4-6");

    // Test 3: Use built-in default
    let entire_summary_model: Option<&str> = None;
    let model_sub: Option<&str> = None;

    let effective = entire_summary_model
        .or(model_sub)
        .unwrap_or(codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL);

    assert_eq!(effective, codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL);
}

#[test]
fn test_entire_summary_enabled_default() {
    let config = MemoriesConfig::default();
    assert!(config.entire_summary_enabled);
}

#[test]
fn test_entire_summary_model_none_by_default() {
    let config = MemoriesConfig::default();
    assert!(config.entire_summary_model.is_none());
}

#[tokio::test]
async fn test_test_config_disables_entire_summary_generation() {
    let codex_home = TempDir::new().expect("temp dir");
    let config = load_default_config_for_test(&codex_home).await;
    assert!(!config.memories.entire_summary_enabled);
}
