use super::*;
use crate::token_data::IdTokenInfo;
use anyhow::Context;
use base64::Engine;
use codex_secrets::LocalSecretsNamespace;
use codex_secrets::SecretScope;
use codex_secrets::SecretsBackendKind;
use codex_secrets::SecretsManager;
use codex_secrets::compute_keyring_account;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use codex_keyring_store::tests::MockKeyringStore;
use keyring::Error as KeyringError;

#[tokio::test]
async fn file_storage_load_returns_auth_dot_json() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("test-key".to_string()),
        tokens: None,
        last_refresh: Some(Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage
        .save(&auth_dot_json)
        .context("failed to save auth file")?;

    let loaded = storage.load().context("failed to load auth file")?;
    assert_eq!(Some(auth_dot_json), loaded);
    Ok(())
}

#[tokio::test]
async fn file_storage_save_persists_auth_dot_json() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("test-key".to_string()),
        tokens: None,
        last_refresh: Some(Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    let file = get_auth_file(codex_home.path());
    storage
        .save(&auth_dot_json)
        .context("failed to save auth file")?;

    let same_auth_dot_json = storage
        .try_read_auth_json(&file)
        .context("failed to read auth file after save")?;
    assert_eq!(auth_dot_json, same_auth_dot_json);
    Ok(())
}

#[tokio::test]
async fn file_storage_round_trips_agent_identity_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let agent_identity = jwt_with_payload(json!({
        "agent_runtime_id": "agent-runtime-id",
        "agent_private_key": "private-key",
        "account_id": "account-id",
        "chatgpt_user_id": "user-id",
        "email": "user@example.com",
        "plan_type": "pro",
        "chatgpt_account_is_fedramp": false,
    }));
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::AgentIdentity),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: Some(AgentIdentityStorage::Jwt(agent_identity)),
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth_dot_json)?;

    let loaded = storage.load()?;
    assert_eq!(Some(auth_dot_json), loaded);
    Ok(())
}

#[tokio::test]
async fn file_storage_round_trips_registered_agent_identity_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let record = AgentIdentityAuthRecord {
        agent_runtime_id: "agent-runtime-id".to_string(),
        agent_private_key: "private-key".to_string(),
        account_id: "account-id".to_string(),
        chatgpt_user_id: "user-id".to_string(),
        email: Some("user@example.com".to_string()),
        plan_type: AccountPlanType::Pro,
        chatgpt_account_is_fedramp: false,
        task_id: Some("task-id".to_string()),
    };
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: Some(AgentIdentityStorage::Record(record)),
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth_dot_json)?;

    let loaded = storage.load()?;
    assert_eq!(Some(auth_dot_json), loaded);
    Ok(())
}

#[tokio::test]
async fn file_storage_loads_empty_agent_identity_email_as_none() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let auth_file = get_auth_file(codex_home.path());
    std::fs::write(
        &auth_file,
        serde_json::to_string_pretty(&json!({
            "auth_mode": "chatgpt",
            "agent_identity": {
                "agent_runtime_id": "agent-runtime-id",
                "agent_private_key": "private-key",
                "account_id": "account-id",
                "chatgpt_user_id": "user-id",
                "email": "",
                "plan_type": "pro",
                "chatgpt_account_is_fedramp": false,
            },
        }))?,
    )?;

    let loaded = storage.load()?;

    assert_eq!(
        loaded,
        Some(AuthDotJson {
            auth_mode: Some(AuthMode::Chatgpt),
            openai_api_key: None,
            tokens: None,
            last_refresh: None,
            agent_identity: Some(AgentIdentityStorage::Record(AgentIdentityAuthRecord {
                agent_runtime_id: "agent-runtime-id".to_string(),
                agent_private_key: "private-key".to_string(),
                account_id: "account-id".to_string(),
                chatgpt_user_id: "user-id".to_string(),
                email: None,
                plan_type: AccountPlanType::Pro,
                chatgpt_account_is_fedramp: false,
                task_id: None,
            })),
            personal_access_token: None,
            bedrock_api_key: None,
            bedrock_access_keys: None,
        })
    );
    Ok(())
}

#[tokio::test]
async fn file_storage_writes_missing_agent_identity_email_as_empty_string() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: Some(AgentIdentityStorage::Record(AgentIdentityAuthRecord {
            agent_runtime_id: "agent-runtime-id".to_string(),
            agent_private_key: "private-key".to_string(),
            account_id: "account-id".to_string(),
            chatgpt_user_id: "user-id".to_string(),
            email: None,
            plan_type: AccountPlanType::Pro,
            chatgpt_account_is_fedramp: false,
            task_id: None,
        })),
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth_dot_json)?;

    let auth_file = get_auth_file(codex_home.path());
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(auth_file)?)?;
    assert_eq!(saved["agent_identity"]["email"], "");
    assert_eq!(storage.load()?, Some(auth_dot_json));
    Ok(())
}

#[tokio::test]
async fn file_storage_round_trips_personal_access_token_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::PersonalAccessToken),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: Some("at-example".to_string()),
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth_dot_json)?;

    let loaded = storage.load()?;
    assert_eq!(Some(auth_dot_json), loaded);
    Ok(())
}

#[tokio::test]
async fn file_storage_loads_agent_identity_as_jwt() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
    let agent_identity_jwt = jwt_with_payload(json!({
        "agent_runtime_id": "agent-runtime-id",
        "agent_private_key": "private-key",
        "account_id": "account-id",
        "chatgpt_user_id": "user-id",
        "email": "user@example.com",
        "plan_type": "pro",
        "chatgpt_account_is_fedramp": false,
    }));
    let auth_file = get_auth_file(codex_home.path());
    std::fs::write(
        &auth_file,
        serde_json::to_string_pretty(&json!({
            "auth_mode": "agentIdentity",
            "agent_identity": agent_identity_jwt,
        }))?,
    )?;

    let loaded = storage.load()?;

    assert_eq!(
        loaded.expect("auth should load").agent_identity,
        Some(AgentIdentityStorage::Jwt(agent_identity_jwt))
    );
    Ok(())
}

#[test]
fn file_storage_delete_removes_auth_file() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("sk-test-key".to_string()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };
    let storage = create_auth_storage(
        dir.path().to_path_buf(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    );
    storage.save(&auth_dot_json)?;
    assert!(dir.path().join("auth.json").exists());
    let storage = FileAuthStorage::new(dir.path().to_path_buf());
    let removed = storage.delete()?;
    assert!(removed);
    assert!(!dir.path().join("auth.json").exists());
    Ok(())
}

#[test]
fn ephemeral_storage_save_load_delete_is_in_memory_only() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let storage = create_auth_storage(
        dir.path().to_path_buf(),
        AuthCredentialsStoreMode::Ephemeral,
        AuthKeyringBackendKind::default(),
    );
    let auth_dot_json = AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("sk-ephemeral".to_string()),
        tokens: None,
        last_refresh: Some(Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth_dot_json)?;
    let loaded = storage.load()?;
    assert_eq!(Some(auth_dot_json), loaded);

    let removed = storage.delete()?;
    assert!(removed);
    let loaded = storage.load()?;
    assert_eq!(None, loaded);
    assert!(!get_auth_file(dir.path()).exists());
    Ok(())
}

fn seed_secrets_backend_and_fallback_auth_file_for_delete(
    mock_keyring: &MockKeyringStore,
    codex_home: &Path,
    auth: &AuthDotJson,
) -> anyhow::Result<PathBuf> {
    let manager = SecretsManager::new_with_keyring_store_and_namespace(
        codex_home.to_path_buf(),
        SecretsBackendKind::Local,
        Arc::new(mock_keyring.clone()),
        LocalSecretsNamespace::CodexAuth,
    );
    manager.set(
        &SecretScope::Global,
        &CODEX_AUTH_SECRET_NAME,
        &serde_json::to_string(auth)?,
    )?;
    let auth_file = get_auth_file(codex_home);
    std::fs::write(&auth_file, "stale")?;
    Ok(auth_file)
}

fn seed_secrets_backend_with_auth(
    mock_keyring: &MockKeyringStore,
    codex_home: &Path,
    auth: &AuthDotJson,
) -> anyhow::Result<()> {
    let manager = SecretsManager::new_with_keyring_store_and_namespace(
        codex_home.to_path_buf(),
        SecretsBackendKind::Local,
        Arc::new(mock_keyring.clone()),
        LocalSecretsNamespace::CodexAuth,
    );
    manager.set(
        &SecretScope::Global,
        &CODEX_AUTH_SECRET_NAME,
        &serde_json::to_string(auth)?,
    )?;
    Ok(())
}

fn assert_keyring_saved_auth_and_removed_fallback(
    mock_keyring: &MockKeyringStore,
    codex_home: &Path,
    expected: &AuthDotJson,
) -> anyhow::Result<()> {
    let manager = SecretsManager::new_with_keyring_store_and_namespace(
        codex_home.to_path_buf(),
        SecretsBackendKind::Local,
        Arc::new(mock_keyring.clone()),
        LocalSecretsNamespace::CodexAuth,
    );
    let saved_value = manager
        .get(&SecretScope::Global, &CODEX_AUTH_SECRET_NAME)?
        .context("encrypted auth entry should exist")?;
    let expected_serialized = serde_json::to_string(expected)?;
    assert_eq!(saved_value, expected_serialized);
    let old_key = compute_store_key(codex_home)?;
    assert!(
        mock_keyring.saved_value(&old_key).is_none(),
        "legacy keyring auth entry should not be used"
    );
    let secrets_key = compute_keyring_account(codex_home);
    assert!(
        mock_keyring.saved_value(&secrets_key).is_some(),
        "secrets backend should persist an encryption passphrase in the keyring"
    );
    assert!(encrypted_auth_file(codex_home).exists());
    let auth_file = get_auth_file(codex_home);
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after keyring save"
    );
    Ok(())
}

fn encrypted_auth_file(codex_home: &Path) -> PathBuf {
    codex_home.join("secrets").join("codex_auth.age")
}

fn id_token_with_prefix(prefix: &str) -> IdTokenInfo {
    #[derive(Serialize)]
    struct Header {
        alg: &'static str,
        typ: &'static str,
    }

    let header = Header {
        alg: "none",
        typ: "JWT",
    };
    let payload = json!({
        "email": format!("{prefix}@example.com"),
        "https://api.openai.com/auth": {
            "chatgpt_account_id": format!("{prefix}-account"),
        },
    });
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header_b64 = encode(&serde_json::to_vec(&header).expect("serialize header"));
    let payload_b64 = encode(&serde_json::to_vec(&payload).expect("serialize payload"));
    let signature_b64 = encode(b"sig");
    let fake_jwt = format!("{header_b64}.{payload_b64}.{signature_b64}");

    crate::token_data::parse_chatgpt_jwt_claims(&fake_jwt).expect("fake JWT should parse")
}

fn auth_with_prefix(prefix: &str) -> AuthDotJson {
    AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some(format!("{prefix}-api-key")),
        tokens: Some(TokenData {
            id_token: id_token_with_prefix(prefix),
            access_token: format!("{prefix}-access"),
            refresh_token: format!("{prefix}-refresh"),
            account_id: Some(format!("{prefix}-account-id")),
        }),
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    }
}

fn jwt_with_payload(payload: serde_json::Value) -> String {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let header_b64 = encode(br#"{"alg":"EdDSA","typ":"JWT"}"#);
    let payload_b64 = encode(&serde_json::to_vec(&payload).expect("payload should serialize"));
    let signature_b64 = encode(b"sig");
    format!("{header_b64}.{payload_b64}.{signature_b64}")
}

#[test]
fn secrets_keyring_auth_storage_load_returns_deserialized_auth() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = SecretsKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let expected = AuthDotJson {
        auth_mode: Some(AuthMode::ApiKey),
        openai_api_key: Some("sk-test".to_string()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };
    seed_secrets_backend_with_auth(&mock_keyring, codex_home.path(), &expected)?;

    let loaded = storage.load()?;
    assert_eq!(Some(expected), loaded);
    Ok(())
}

#[test]
fn keyring_auth_storage_compute_store_key_for_home_directory() -> anyhow::Result<()> {
    let codex_home = PathBuf::from("~/.codex");

    let key = compute_store_key(codex_home.as_path())?;

    assert_eq!(key, "cli|940db7b1d0e4eb40");
    Ok(())
}

#[test]
fn direct_keyring_auth_storage_saves_legacy_keyring_entry() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = DirectKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let auth_file = get_auth_file(codex_home.path());
    std::fs::write(&auth_file, "stale")?;
    let auth = auth_with_prefix("direct");

    storage.save(&auth)?;

    let legacy_key = compute_store_key(codex_home.path())?;
    let saved_value = mock_keyring
        .saved_value(&legacy_key)
        .context("direct keyring auth entry should exist")?;
    assert_eq!(saved_value, serde_json::to_string(&auth)?);
    assert!(!encrypted_auth_file(codex_home.path()).exists());
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after keyring save"
    );
    assert_eq!(storage.load()?, Some(auth));
    Ok(())
}

#[test]
fn direct_keyring_auth_storage_delete_removes_keyring_and_file() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = DirectKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let auth = auth_with_prefix("direct-delete");
    storage.save(&auth)?;
    let auth_file = get_auth_file(codex_home.path());
    std::fs::write(&auth_file, "stale")?;

    let removed = storage.delete()?;

    assert!(removed, "delete should report removal");
    assert_eq!(storage.load()?, None, "keyring auth should be removed");
    assert!(
        mock_keyring
            .saved_value(&compute_store_key(codex_home.path())?)
            .is_none(),
        "legacy keyring auth entry should be removed"
    );
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after keyring delete"
    );
    assert!(!encrypted_auth_file(codex_home.path()).exists());
    Ok(())
}

#[test]
fn factory_uses_secrets_backend_only_when_requested() -> anyhow::Result<()> {
    let direct_home = tempdir()?;
    let direct_keyring = MockKeyringStore::default();
    let direct_storage = create_auth_storage_with_store(
        direct_home.path().to_path_buf(),
        AuthCredentialsStoreMode::Keyring,
        Arc::new(direct_keyring.clone()),
        AuthKeyringBackendKind::Direct,
    );
    let direct_auth = auth_with_prefix("factory-direct");
    direct_storage.save(&direct_auth)?;
    assert!(
        direct_keyring
            .saved_value(&compute_store_key(direct_home.path())?)
            .is_some()
    );
    assert!(!encrypted_auth_file(direct_home.path()).exists());

    let secrets_home = tempdir()?;
    let secrets_keyring = MockKeyringStore::default();
    let secrets_storage = create_auth_storage_with_store(
        secrets_home.path().to_path_buf(),
        AuthCredentialsStoreMode::Keyring,
        Arc::new(secrets_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let secrets_auth = auth_with_prefix("factory-secrets");
    secrets_storage.save(&secrets_auth)?;
    assert!(
        secrets_keyring
            .saved_value(&compute_keyring_account(secrets_home.path()))
            .is_some()
    );
    assert!(encrypted_auth_file(secrets_home.path()).exists());
    Ok(())
}

#[test]
fn secrets_keyring_auth_storage_save_persists_and_removes_fallback_file() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = SecretsKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let auth_file = get_auth_file(codex_home.path());
    std::fs::write(&auth_file, "stale")?;
    let auth = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: Some(TokenData {
            id_token: Default::default(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            account_id: Some("account".to_string()),
        }),
        last_refresh: Some(Utc::now()),
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    storage.save(&auth)?;

    assert_keyring_saved_auth_and_removed_fallback(&mock_keyring, codex_home.path(), &auth)?;
    Ok(())
}

#[test]
fn secrets_keyring_auth_storage_delete_removes_keyring_and_file() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = SecretsKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let auth = auth_with_prefix("to-delete");
    let auth_file = seed_secrets_backend_and_fallback_auth_file_for_delete(
        &mock_keyring,
        codex_home.path(),
        &auth,
    )?;

    let removed = storage.delete()?;

    assert!(removed, "delete should report removal");
    assert_eq!(storage.load()?, None, "encrypted auth should be removed");
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after keyring delete"
    );
    Ok(())
}

#[test]
fn secrets_keyring_auth_storage_delete_removes_legacy_direct_keyring_entry() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let direct_storage = DirectKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    direct_storage.save(&auth_with_prefix("legacy-direct"))?;
    let storage = SecretsKeyringAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
    );
    let auth = auth_with_prefix("to-delete");
    let auth_file = seed_secrets_backend_and_fallback_auth_file_for_delete(
        &mock_keyring,
        codex_home.path(),
        &auth,
    )?;

    let removed = storage.delete()?;

    assert!(removed, "delete should report removal");
    assert_eq!(storage.load()?, None, "encrypted auth should be removed");
    assert_eq!(
        direct_storage.load()?,
        None,
        "legacy direct keyring auth should be removed"
    );
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after keyring delete"
    );
    Ok(())
}

#[test]
fn auto_auth_storage_load_prefers_keyring_value() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let keyring_auth = auth_with_prefix("keyring");
    seed_secrets_backend_with_auth(&mock_keyring, codex_home.path(), &keyring_auth)?;

    let file_auth = auth_with_prefix("file");
    storage.file_storage.save(&file_auth)?;

    let loaded = storage.load()?;
    assert_eq!(loaded, Some(keyring_auth));
    Ok(())
}

#[test]
fn auto_auth_storage_load_uses_file_when_keyring_empty() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring),
        AuthKeyringBackendKind::Secrets,
    );

    let expected = auth_with_prefix("file-only");
    storage.file_storage.save(&expected)?;

    let loaded = storage.load()?;
    assert_eq!(loaded, Some(expected));
    Ok(())
}

#[test]
fn auto_auth_storage_load_falls_back_when_keyring_errors() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let key = compute_keyring_account(codex_home.path());

    let encrypted = auth_with_prefix("encrypted");
    seed_secrets_backend_with_auth(&mock_keyring, codex_home.path(), &encrypted)?;
    mock_keyring.set_error(&key, KeyringError::Invalid("error".into(), "load".into()));

    let expected = auth_with_prefix("fallback");
    storage.file_storage.save(&expected)?;

    let loaded = storage.load()?;
    assert_eq!(loaded, Some(expected));
    Ok(())
}

#[test]
fn auto_auth_storage_save_prefers_keyring() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let stale = auth_with_prefix("stale");
    storage.file_storage.save(&stale)?;

    let expected = auth_with_prefix("to-save");
    storage.save(&expected)?;

    assert_keyring_saved_auth_and_removed_fallback(&mock_keyring, codex_home.path(), &expected)?;
    Ok(())
}

#[test]
fn auto_auth_storage_save_falls_back_when_keyring_errors() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let key = compute_keyring_account(codex_home.path());
    mock_keyring.set_error(&key, KeyringError::Invalid("error".into(), "save".into()));

    let auth = auth_with_prefix("fallback");
    storage.save(&auth)?;

    let auth_file = get_auth_file(codex_home.path());
    assert!(
        auth_file.exists(),
        "fallback auth.json should be created when keyring save fails"
    );
    let saved = storage
        .file_storage
        .load()?
        .context("fallback auth should exist")?;
    assert_eq!(saved, auth);
    assert!(
        mock_keyring.saved_value(&key).is_none(),
        "keyring should not contain value when save fails"
    );
    Ok(())
}

#[test]
fn auto_auth_storage_delete_removes_keyring_and_file() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage = AutoAuthStorage::new(
        codex_home.path().to_path_buf(),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let auth = auth_with_prefix("to-delete");
    let auth_file = seed_secrets_backend_and_fallback_auth_file_for_delete(
        &mock_keyring,
        codex_home.path(),
        &auth,
    )?;

    let removed = storage.delete()?;

    assert!(removed, "delete should report removal");
    assert_eq!(storage.load()?, None, "encrypted auth should be removed");
    assert!(
        !auth_file.exists(),
        "fallback auth.json should be removed after delete"
    );
    Ok(())
}

fn identity_with_profile(codex_home: &Path, profile: AuthProfile) -> AuthStorageIdentity {
    AuthStorageIdentity {
        codex_home: codex_home.to_path_buf(),
        profile,
    }
}

#[test]
fn auth_profile_validation_matches_documented_grammar() {
    for valid in [
        "a",
        "A0",
        "jiaqiwang969",
        "OmarGuthorn8",
        "profile.with-dash_and_underscore",
    ] {
        assert_eq!(AuthProfile::parse(valid), AuthProfile::Named(valid.into()));
    }

    let max_len = format!("a{}", "0".repeat(MAX_AUTH_PROFILE_LEN - 1));
    assert_eq!(
        AuthProfile::parse(&max_len),
        AuthProfile::Named(max_len.clone())
    );

    for invalid in [
        "",
        "-profile",
        ".profile",
        "_profile",
        "profile/name",
        "profile\\name",
        "profile name",
        "profile中文",
    ] {
        assert_eq!(AuthProfile::parse(invalid), AuthProfile::Invalid);
    }
    assert_eq!(
        AuthProfile::parse(&format!("a{}", "0".repeat(MAX_AUTH_PROFILE_LEN))),
        AuthProfile::Invalid
    );
}

#[test]
fn auth_profile_name_distinguishes_default_named_and_invalid_profiles() -> anyhow::Result<()> {
    assert_eq!(AuthProfile::Default.name()?, None);
    assert_eq!(
        AuthProfile::parse("wellau-jiaqiwang969").name()?,
        Some("wellau-jiaqiwang969")
    );

    let err = AuthProfile::Invalid
        .name()
        .expect_err("invalid profiles must fail closed");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    Ok(())
}

#[test]
fn wellau_profile_namespace_is_lowercase_and_typo_safe() -> anyhow::Result<()> {
    assert_eq!(wellau_auth_profile_name(None)?, None);
    assert_eq!(wellau_auth_profile_name(Some("jiaqiwang969"))?, None);
    assert_eq!(
        wellau_auth_profile_name(Some("wellau-jiaqiwang969"))?,
        Some("wellau-jiaqiwang969")
    );

    for invalid in ["wellau-", "WellAU-jiaqiwang969", "wellua-jiaqiwang969"] {
        let error = wellau_auth_profile_name(Some(invalid))
            .expect_err("reserved WellAU names must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
    Ok(())
}

#[test]
fn wellau_storage_policy_accepts_only_api_keys_for_every_backend() -> anyhow::Result<()> {
    let cases = [
        (
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::Direct,
        ),
        (
            AuthCredentialsStoreMode::Keyring,
            AuthKeyringBackendKind::Direct,
        ),
        (
            AuthCredentialsStoreMode::Keyring,
            AuthKeyringBackendKind::Secrets,
        ),
        (
            AuthCredentialsStoreMode::Auto,
            AuthKeyringBackendKind::Secrets,
        ),
        (
            AuthCredentialsStoreMode::Ephemeral,
            AuthKeyringBackendKind::Direct,
        ),
    ];
    let legacy_api_key = AuthDotJson {
        auth_mode: None,
        openai_api_key: Some("wellau-test-key".to_string()),
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };
    let chatgpt_auth = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };

    for (mode, keyring_backend_kind) in cases {
        let codex_home = tempdir()?;
        let storage = create_auth_storage_with_store_and_identity(
            identity_with_profile(codex_home.path(), AuthProfile::parse("wellau-test-account")),
            mode,
            Arc::new(MockKeyringStore::default()),
            keyring_backend_kind,
        );

        storage.save(&legacy_api_key)?;
        assert_eq!(storage.load()?, Some(legacy_api_key.clone()));
        let error = storage
            .save(&chatgpt_auth)
            .expect_err("WellAU storage must reject non-API-key credentials");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(storage.load()?, Some(legacy_api_key.clone()));
        assert!(storage.delete()?);
    }
    Ok(())
}

#[test]
fn wellau_storage_rejects_wrong_existing_file_but_allows_delete() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let identity =
        identity_with_profile(codex_home.path(), AuthProfile::parse("wellau-test-account"));
    let auth_file = identity.auth_file()?;
    std::fs::write(
        &auth_file,
        serde_json::to_vec(&AuthDotJson {
            auth_mode: Some(AuthMode::Chatgpt),
            openai_api_key: None,
            tokens: None,
            last_refresh: None,
            agent_identity: None,
            personal_access_token: None,
            bedrock_api_key: None,
            bedrock_access_keys: None,
        })?,
    )?;
    let storage = create_auth_storage_with_store_and_identity(
        identity,
        AuthCredentialsStoreMode::File,
        Arc::new(MockKeyringStore::default()),
        AuthKeyringBackendKind::Direct,
    );

    let error = storage
        .load()
        .expect_err("an existing non-API WellAU credential must not load");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(storage.delete()?);
    assert!(!auth_file.exists());
    Ok(())
}

#[test]
fn ordinary_named_profile_keeps_existing_auth_mode_behavior() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage = create_auth_storage_with_store_and_identity(
        identity_with_profile(codex_home.path(), AuthProfile::parse("jiaqiwang969")),
        AuthCredentialsStoreMode::File,
        Arc::new(MockKeyringStore::default()),
        AuthKeyringBackendKind::Direct,
    );
    let auth = AuthDotJson {
        auth_mode: Some(AuthMode::Chatgpt),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        bedrock_access_keys: None,
    };
    storage.save(&auth)?;
    assert_eq!(storage.load()?, Some(auth));
    Ok(())
}

#[test]
fn file_auth_profiles_are_isolated_and_default_path_is_compatible() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let default = FileAuthStorage::new_with_identity(identity_with_profile(
        codex_home.path(),
        AuthProfile::Default,
    ));
    let profile_a = FileAuthStorage::new_with_identity(identity_with_profile(
        codex_home.path(),
        AuthProfile::parse("jiaqiwang969"),
    ));
    let profile_b = FileAuthStorage::new_with_identity(identity_with_profile(
        codex_home.path(),
        AuthProfile::parse("OmarGuthorn8"),
    ));
    let default_auth = auth_with_prefix("default-profile");
    let auth_a = auth_with_prefix("profile-a");
    let auth_b = auth_with_prefix("profile-b");

    default.save(&default_auth)?;
    profile_a.save(&auth_a)?;
    profile_b.save(&auth_b)?;

    assert_eq!(default.load()?, Some(default_auth));
    assert_eq!(profile_a.load()?, Some(auth_a));
    assert_eq!(profile_b.load()?, Some(auth_b.clone()));
    assert!(codex_home.path().join("auth.json").exists());
    assert!(codex_home.path().join("auth-jiaqiwang969.json").exists());
    assert!(codex_home.path().join("auth-OmarGuthorn8.json").exists());

    assert!(profile_a.delete()?);
    assert_eq!(profile_a.load()?, None);
    assert_eq!(profile_b.load()?, Some(auth_b));
    assert!(codex_home.path().join("auth.json").exists());
    Ok(())
}

#[test]
fn direct_keyring_auth_profiles_are_isolated() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let identity_a = identity_with_profile(codex_home.path(), AuthProfile::parse("profile-a"));
    let identity_b = identity_with_profile(codex_home.path(), AuthProfile::parse("profile-b"));
    let storage_a = DirectKeyringAuthStorage::new_with_identity(
        identity_a.clone(),
        Arc::new(mock_keyring.clone()),
    );
    let storage_b = DirectKeyringAuthStorage::new_with_identity(
        identity_b.clone(),
        Arc::new(mock_keyring.clone()),
    );
    let auth_a = auth_with_prefix("direct-a");
    let auth_b = auth_with_prefix("direct-b");

    storage_a.save(&auth_a)?;
    storage_b.save(&auth_b)?;

    let key_a = identity_a.keyring_store_key()?;
    let key_b = identity_b.keyring_store_key()?;
    assert_ne!(key_a, key_b);
    assert!(mock_keyring.saved_value(&key_a).is_some());
    assert!(mock_keyring.saved_value(&key_b).is_some());
    assert_eq!(storage_a.load()?, Some(auth_a));
    assert_eq!(storage_b.load()?, Some(auth_b.clone()));

    assert!(storage_a.delete()?);
    assert_eq!(storage_a.load()?, None);
    assert_eq!(storage_b.load()?, Some(auth_b));
    Ok(())
}

#[test]
fn secrets_keyring_auth_profiles_are_isolated() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let identity_a = identity_with_profile(codex_home.path(), AuthProfile::parse("profile-a"));
    let identity_b = identity_with_profile(codex_home.path(), AuthProfile::parse("profile-b"));
    let storage_a = SecretsKeyringAuthStorage::new_with_identity(
        identity_a.clone(),
        Arc::new(mock_keyring.clone()),
    );
    let storage_b = SecretsKeyringAuthStorage::new_with_identity(
        identity_b.clone(),
        Arc::new(mock_keyring.clone()),
    );
    let auth_a = auth_with_prefix("secrets-a");
    let auth_b = auth_with_prefix("secrets-b");

    storage_a.save(&auth_a)?;
    storage_b.save(&auth_b)?;

    let home_a = identity_a.profile_storage_home()?;
    let home_b = identity_b.profile_storage_home()?;
    assert_ne!(home_a, home_b);
    assert!(encrypted_auth_file(&home_a).exists());
    assert!(encrypted_auth_file(&home_b).exists());
    assert!(
        mock_keyring
            .saved_value(&compute_keyring_account(&home_a))
            .is_some()
    );
    assert!(
        mock_keyring
            .saved_value(&compute_keyring_account(&home_b))
            .is_some()
    );
    assert_eq!(storage_a.load()?, Some(auth_a));
    assert_eq!(storage_b.load()?, Some(auth_b.clone()));

    assert!(storage_a.delete()?);
    assert_eq!(storage_a.load()?, None);
    assert_eq!(storage_b.load()?, Some(auth_b));
    Ok(())
}

#[test]
fn auto_auth_profiles_are_isolated() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let mock_keyring = MockKeyringStore::default();
    let storage_a = AutoAuthStorage::new_with_identity(
        identity_with_profile(codex_home.path(), AuthProfile::parse("profile-a")),
        Arc::new(mock_keyring.clone()),
        AuthKeyringBackendKind::Secrets,
    );
    let storage_b = AutoAuthStorage::new_with_identity(
        identity_with_profile(codex_home.path(), AuthProfile::parse("profile-b")),
        Arc::new(mock_keyring),
        AuthKeyringBackendKind::Secrets,
    );
    let auth_a = auth_with_prefix("auto-a");
    let auth_b = auth_with_prefix("auto-b");

    storage_a.save(&auth_a)?;
    storage_b.save(&auth_b)?;

    assert_eq!(storage_a.load()?, Some(auth_a));
    assert_eq!(storage_b.load()?, Some(auth_b.clone()));
    assert!(storage_a.delete()?);
    assert_eq!(storage_b.load()?, Some(auth_b));
    Ok(())
}

#[test]
fn ephemeral_auth_profiles_are_isolated() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let storage_a = EphemeralAuthStorage::new_with_identity(identity_with_profile(
        codex_home.path(),
        AuthProfile::parse("profile-a"),
    ));
    let storage_b = EphemeralAuthStorage::new_with_identity(identity_with_profile(
        codex_home.path(),
        AuthProfile::parse("profile-b"),
    ));
    let auth_a = auth_with_prefix("ephemeral-a");
    let auth_b = auth_with_prefix("ephemeral-b");

    storage_a.save(&auth_a)?;
    storage_b.save(&auth_b)?;

    assert_eq!(storage_a.load()?, Some(auth_a));
    assert_eq!(storage_b.load()?, Some(auth_b.clone()));
    assert!(storage_a.delete()?);
    assert_eq!(storage_a.load()?, None);
    assert_eq!(storage_b.load()?, Some(auth_b));
    Ok(())
}

#[test]
fn invalid_auth_profile_fails_closed_for_every_storage_backend() -> anyhow::Result<()> {
    let codex_home = tempdir()?;
    let auth = auth_with_prefix("must-not-persist");
    let cases = [
        (
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::Direct,
        ),
        (
            AuthCredentialsStoreMode::Keyring,
            AuthKeyringBackendKind::Direct,
        ),
        (
            AuthCredentialsStoreMode::Keyring,
            AuthKeyringBackendKind::Secrets,
        ),
        (
            AuthCredentialsStoreMode::Auto,
            AuthKeyringBackendKind::Secrets,
        ),
        (
            AuthCredentialsStoreMode::Ephemeral,
            AuthKeyringBackendKind::Direct,
        ),
    ];

    for (mode, keyring_backend_kind) in cases {
        let storage = create_auth_storage_with_store_and_identity(
            identity_with_profile(codex_home.path(), AuthProfile::Invalid),
            mode,
            Arc::new(MockKeyringStore::default()),
            keyring_backend_kind,
        );
        assert_eq!(
            storage
                .load()
                .expect_err("invalid profile must not load")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            storage
                .save(&auth)
                .expect_err("invalid profile must not save")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            storage
                .delete()
                .expect_err("invalid profile must not delete")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    assert!(!get_auth_file(codex_home.path()).exists());
    assert!(!codex_home.path().join(".invalid-auth-profile").exists());
    Ok(())
}
