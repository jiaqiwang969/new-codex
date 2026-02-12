use anyhow::Result;
use codex_core::CodexAuth;
use codex_core::ThreadManager;
use codex_core::built_in_model_providers;
use codex_core::models_manager::manager::RefreshStrategy;
use core_test_support::load_default_config_for_test;
use pretty_assertions::assert_eq;
use std::collections::HashSet;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_returns_api_key_models() -> Result<()> {
    let codex_home = tempdir()?;
    let config = load_default_config_for_test(&codex_home).await;
    let manager = ThreadManager::with_models_provider(
        CodexAuth::from_api_key("sk-test"),
        built_in_model_providers()["openai"].clone(),
    );
    let models = manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;

    assert_models_shape(&models);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_returns_chatgpt_models() -> Result<()> {
    let codex_home = tempdir()?;
    let config = load_default_config_for_test(&codex_home).await;
    let manager = ThreadManager::with_models_provider(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        built_in_model_providers()["openai"].clone(),
    );
    let models = manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;

    assert_models_shape(&models);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_are_identical_for_api_key_and_chatgpt_auth() -> Result<()> {
    let codex_home = tempdir()?;
    let config = load_default_config_for_test(&codex_home).await;
    let provider = built_in_model_providers()["openai"].clone();

    let api_manager =
        ThreadManager::with_models_provider(CodexAuth::from_api_key("sk-test"), provider.clone());
    let chatgpt_manager = ThreadManager::with_models_provider(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        provider,
    );

    let api_models = api_manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;
    let chatgpt_models = chatgpt_manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;

    assert_eq!(api_models, chatgpt_models);

    Ok(())
}

fn assert_models_shape(models: &[codex_protocol::openai_models::ModelPreset]) {
    assert!(
        !models.is_empty(),
        "expected list_models to return at least one model"
    );

    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    let ids_set: HashSet<&str> = ids.iter().copied().collect();
    if ids.len() != ids_set.len() {
        let mut counts = std::collections::BTreeMap::new();
        for id in &ids {
            *counts.entry(*id).or_insert(0usize) += 1;
        }

        let duplicates: Vec<(&str, usize)> =
            counts.into_iter().filter(|(_, count)| *count > 1).collect();

        panic!("model list should not contain duplicates: {duplicates:?}; full list: {ids:?}");
    }

    for expected in [
        "gpt-5.3-codex",
        "gpt-5.2-codex",
        "gpt-5.2",
        "gemini-3-flash-preview",
        "grok-4-latest",
    ] {
        assert!(
            ids_set.contains(expected),
            "expected list_models output to include {expected}"
        );
    }

    let defaults: Vec<&str> = models
        .iter()
        .filter(|model| model.is_default)
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(defaults, vec!["gpt-5.2-codex"]);
}
