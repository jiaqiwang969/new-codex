use codex_core::config::types::MemoriesConfig;
use core_test_support::load_default_config_for_test;
use tempfile::TempDir;

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
