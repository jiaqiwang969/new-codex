use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tracing::warn;

use super::BedrockAccessKeysAuth;
use super::BedrockApiKeyAuth;
use crate::token_data::TokenData;
use codex_agent_identity::AgentIdentityJwtClaims;
use codex_agent_identity::decode_agent_identity_jwt;
use codex_config::types::AuthCredentialsStoreMode;
pub use codex_config::types::AuthKeyringBackendKind;
use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;
use codex_protocol::account::PlanType as AccountPlanType;
use codex_protocol::auth::AuthMode;
use codex_secrets::LocalSecretsNamespace;
use codex_secrets::SecretName;
use codex_secrets::SecretScope;
use codex_secrets::SecretsBackendKind;
use codex_secrets::SecretsManager;
use once_cell::sync::Lazy;

const CODEX_AUTH_PROFILE_ENV: &str = "CODEX_AUTH_PROFILE";
const MAX_AUTH_PROFILE_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum AuthProfile {
    Default,
    Named(String),
    Invalid,
}

impl AuthProfile {
    fn from_env() -> Self {
        match std::env::var_os(CODEX_AUTH_PROFILE_ENV) {
            None => Self::Default,
            Some(value) => match value.into_string() {
                Ok(value) => Self::parse(&value),
                Err(_) => Self::Invalid,
            },
        }
    }

    fn parse(value: &str) -> Self {
        let bytes = value.as_bytes();
        let Some(first) = bytes.first() else {
            return Self::Invalid;
        };
        if bytes.len() > MAX_AUTH_PROFILE_LEN || !first.is_ascii_alphanumeric() {
            return Self::Invalid;
        }
        if !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Self::Invalid;
        }
        Self::Named(value.to_string())
    }

    fn validate(&self) -> std::io::Result<()> {
        match self {
            Self::Default | Self::Named(_) => Ok(()),
            Self::Invalid => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid CODEX_AUTH_PROFILE; expected [A-Za-z0-9][A-Za-z0-9._-]{0,63}",
            )),
        }
    }

    fn name(&self) -> std::io::Result<Option<&str>> {
        self.validate()?;
        Ok(match self {
            Self::Default => None,
            Self::Named(profile) => Some(profile),
            Self::Invalid => unreachable!("profile validation returned success"),
        })
    }
}

// Authentication profile selection is process-scoped. Capture it once so every auth manager and
// every refresh/save/delete path in this process uses the same credential slot even if a caller
// later mutates the process environment.
static PROCESS_AUTH_PROFILE: Lazy<AuthProfile> = Lazy::new(AuthProfile::from_env);

fn captured_auth_profile() -> AuthProfile {
    // Unit tests construct explicit identities when they exercise profile isolation. Keep legacy
    // helpers deterministic even when the developer has exported this variable in the shell
    // running the test suite.
    if cfg!(test) {
        AuthProfile::Default
    } else {
        PROCESS_AUTH_PROFILE.clone()
    }
}

#[derive(Clone, Debug)]
struct AuthStorageIdentity {
    codex_home: PathBuf,
    profile: AuthProfile,
}

impl AuthStorageIdentity {
    fn capture(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            profile: captured_auth_profile(),
        }
    }

    fn auth_file(&self) -> std::io::Result<PathBuf> {
        self.profile.validate()?;
        Ok(match &self.profile {
            AuthProfile::Default => get_auth_file(&self.codex_home),
            AuthProfile::Named(profile) => self.codex_home.join(format!("auth-{profile}.json")),
            AuthProfile::Invalid => unreachable!("profile validation returned success"),
        })
    }

    fn profile_storage_home(&self) -> std::io::Result<PathBuf> {
        self.profile.validate()?;
        Ok(self.profile_storage_home_unchecked())
    }

    fn profile_storage_home_unchecked(&self) -> PathBuf {
        match &self.profile {
            AuthProfile::Default => self.codex_home.clone(),
            AuthProfile::Named(profile) => self
                .codex_home
                .canonicalize()
                .unwrap_or_else(|_| self.codex_home.clone())
                .join("auth-profiles")
                .join(profile),
            // No operation may use this path: every backend validates before I/O. It exists only
            // because SecretsManager construction itself is infallible and requires a path.
            AuthProfile::Invalid => self.codex_home.join(".invalid-auth-profile"),
        }
    }

    fn keyring_store_key(&self) -> std::io::Result<String> {
        compute_store_key(&self.profile_storage_home()?)
    }
}

/// Expected structure for $CODEX_HOME/auth.json.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AuthDotJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<AuthMode>,

    #[serde(rename = "OPENAI_API_KEY")]
    pub openai_api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenData>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_identity: Option<AgentIdentityStorage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal_access_token: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bedrock_api_key: Option<BedrockApiKeyAuth>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bedrock_access_keys: Option<BedrockAccessKeysAuth>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum AgentIdentityStorage {
    Jwt(String),
    Record(AgentIdentityAuthRecord),
}

impl AgentIdentityStorage {
    pub fn has_auth_material(&self) -> bool {
        match self {
            Self::Jwt(jwt) => !jwt.trim().is_empty(),
            Self::Record(record) => {
                !record.agent_runtime_id.trim().is_empty()
                    && !record.agent_private_key.trim().is_empty()
            }
        }
    }

    pub(crate) fn as_record(&self) -> Option<&AgentIdentityAuthRecord> {
        match self {
            Self::Jwt(_) => None,
            Self::Record(record) => Some(record),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentIdentityAuthRecord {
    pub agent_runtime_id: String,
    pub agent_private_key: String,
    pub account_id: String,
    pub chatgpt_user_id: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_empty_string",
        serialize_with = "serialize_optional_string_as_empty"
    )]
    pub email: Option<String>,
    pub plan_type: AccountPlanType,
    pub chatgpt_account_is_fedramp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

fn deserialize_optional_non_empty_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|value| value.filter(|value| !value.is_empty()))
}

fn serialize_optional_string_as_empty<S>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    value.as_deref().unwrap_or_default().serialize(serializer)
}

impl AgentIdentityAuthRecord {
    pub(crate) fn from_agent_identity_jwt(jwt: &str) -> std::io::Result<Self> {
        let claims =
            decode_agent_identity_jwt(jwt, /*jwks*/ None).map_err(std::io::Error::other)?;

        Ok(claims.into())
    }
}

impl From<AgentIdentityJwtClaims> for AgentIdentityAuthRecord {
    fn from(claims: AgentIdentityJwtClaims) -> Self {
        Self {
            agent_runtime_id: claims.agent_runtime_id,
            agent_private_key: claims.agent_private_key,
            account_id: claims.account_id,
            chatgpt_user_id: claims.chatgpt_user_id,
            email: claims.email,
            plan_type: claims.plan_type.into(),
            chatgpt_account_is_fedramp: claims.chatgpt_account_is_fedramp,
            task_id: None,
        }
    }
}

pub(super) fn get_auth_file(codex_home: &Path) -> PathBuf {
    codex_home.join("auth.json")
}

/// Returns the credential file selected for this process by `CODEX_AUTH_PROFILE`.
pub fn active_auth_file(codex_home: &Path) -> std::io::Result<PathBuf> {
    AuthStorageIdentity::capture(codex_home.to_path_buf()).auth_file()
}

/// Returns the named authentication profile selected for this process, if any.
pub fn active_auth_profile_name() -> std::io::Result<Option<String>> {
    captured_auth_profile()
        .name()
        .map(|profile| profile.map(str::to_string))
}

/// Returns the WellAU API-key proxy profile when `profile` uses the reserved
/// `wellau-<account>` namespace.
///
/// The namespace is deliberately lowercase. Rejecting case-only variants avoids
/// credential-slot collisions on case-insensitive filesystems. The common
/// `wellua-` transposition is also rejected so a proxy key cannot silently fall
/// back to the default OpenAI route.
pub fn wellau_auth_profile_name(profile: Option<&str>) -> std::io::Result<Option<&str>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    let lowercase = profile.to_ascii_lowercase();
    if lowercase.starts_with("wellua-") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid WellAU auth profile prefix `wellua-`; use lowercase `wellau-`",
        ));
    }
    if !lowercase.starts_with("wellau-") {
        return Ok(None);
    }
    if !profile.starts_with("wellau-") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WellAU auth profiles must use the lowercase `wellau-` prefix",
        ));
    }
    if profile.len() == "wellau-".len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WellAU auth profiles require a non-empty account suffix, for example `wellau-example`",
        ));
    }
    Ok(Some(profile))
}

/// Returns the active process-scoped WellAU API-key proxy profile, if any.
pub fn active_wellau_auth_profile_name() -> std::io::Result<Option<String>> {
    let profile = active_auth_profile_name()?;
    wellau_auth_profile_name(profile.as_deref()).map(|profile| profile.map(str::to_string))
}

fn delete_file_if_exists(auth_file: &Path) -> std::io::Result<bool> {
    match std::fs::remove_file(auth_file) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) trait AuthStorageBackend: Debug + Send + Sync {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>>;
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()>;
    fn delete(&self) -> std::io::Result<bool>;
}

#[derive(Clone, Debug)]
struct ProfilePolicyAuthStorage {
    identity: AuthStorageIdentity,
    storage: Arc<dyn AuthStorageBackend>,
}

impl ProfilePolicyAuthStorage {
    fn validate_auth(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.identity.profile.validate()?;
        let profile = self.identity.profile.name()?;
        let Some(wellau_profile) = wellau_auth_profile_name(profile)? else {
            return Ok(());
        };

        let resolved_mode = auth.auth_mode.unwrap_or_else(|| {
            if auth.personal_access_token.is_some() {
                AuthMode::PersonalAccessToken
            } else if auth.bedrock_api_key.is_some() {
                AuthMode::BedrockApiKey
            } else if auth.openai_api_key.is_some() {
                AuthMode::ApiKey
            } else {
                AuthMode::Chatgpt
            }
        });
        if resolved_mode == AuthMode::ApiKey {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "CODEX_AUTH_PROFILE={wellau_profile} only accepts dedicated API-key credentials; found {resolved_mode:?}"
                ),
            ))
        }
    }
}

impl AuthStorageBackend for ProfilePolicyAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let auth = self.storage.load()?;
        if let Some(auth) = auth.as_ref() {
            self.validate_auth(auth)?;
        }
        Ok(auth)
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.validate_auth(auth)?;
        self.storage.save(auth)
    }

    fn delete(&self) -> std::io::Result<bool> {
        self.storage.delete()
    }
}

#[derive(Clone, Debug)]
pub(super) struct FileAuthStorage {
    identity: AuthStorageIdentity,
}

impl FileAuthStorage {
    #[cfg(test)]
    pub(super) fn new(codex_home: PathBuf) -> Self {
        Self::new_with_identity(AuthStorageIdentity::capture(codex_home))
    }

    fn new_with_identity(identity: AuthStorageIdentity) -> Self {
        Self { identity }
    }

    /// Attempt to read and parse the `auth.json` file in the given `CODEX_HOME` directory.
    /// Returns the full AuthDotJson structure.
    pub(super) fn try_read_auth_json(&self, auth_file: &Path) -> std::io::Result<AuthDotJson> {
        let mut file = File::open(auth_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let auth_dot_json: AuthDotJson = serde_json::from_str(&contents)?;

        Ok(auth_dot_json)
    }
}

impl AuthStorageBackend for FileAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let auth_file = self.identity.auth_file()?;
        let auth_dot_json = match self.try_read_auth_json(&auth_file) {
            Ok(auth) => auth,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(Some(auth_dot_json))
    }

    fn save(&self, auth_dot_json: &AuthDotJson) -> std::io::Result<()> {
        let auth_file = self.identity.auth_file()?;

        if let Some(parent) = auth_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(auth_dot_json)?;
        let mut options = OpenOptions::new();
        options.truncate(true).write(true).create(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(auth_file)?;
        file.write_all(json_data.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        delete_file_if_exists(&self.identity.auth_file()?)
    }
}

static CODEX_AUTH_SECRET_NAME: Lazy<SecretName> =
    Lazy::new(|| match SecretName::new("CODEX_AUTH") {
        Ok(name) => name,
        Err(err) => unreachable!("CODEX_AUTH should be a valid secret name: {err}"),
    });
const KEYRING_SERVICE: &str = "Codex Auth";

// turns codex_home path into a stable, short key string
fn compute_store_key(codex_home: &Path) -> std::io::Result<String> {
    let canonical = codex_home
        .canonicalize()
        .unwrap_or_else(|_| codex_home.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    let truncated = hex.get(..16).unwrap_or(&hex);
    Ok(format!("cli|{truncated}"))
}

#[derive(Clone, Debug)]
struct DirectKeyringAuthStorage {
    identity: AuthStorageIdentity,
    keyring_store: Arc<dyn KeyringStore>,
}

impl DirectKeyringAuthStorage {
    #[cfg(test)]
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self::new_with_identity(AuthStorageIdentity::capture(codex_home), keyring_store)
    }

    fn new_with_identity(
        identity: AuthStorageIdentity,
        keyring_store: Arc<dyn KeyringStore>,
    ) -> Self {
        Self {
            identity,
            keyring_store,
        }
    }

    fn load_from_keyring(&self, key: &str) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_store.load(KEYRING_SERVICE, key) {
            Ok(Some(serialized)) => serde_json::from_str(&serialized).map(Some).map_err(|err| {
                std::io::Error::other(format!(
                    "failed to deserialize CLI auth from keyring: {err}"
                ))
            }),
            Ok(None) => Ok(None),
            Err(error) => Err(std::io::Error::other(format!(
                "failed to load CLI auth from keyring: {}",
                error.message()
            ))),
        }
    }

    fn save_to_keyring(&self, key: &str, value: &str) -> std::io::Result<()> {
        match self.keyring_store.save(KEYRING_SERVICE, key, value) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = format!(
                    "failed to write OAuth tokens to keyring: {}",
                    error.message()
                );
                warn!("{message}");
                Err(std::io::Error::other(message))
            }
        }
    }
}

impl AuthStorageBackend for DirectKeyringAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let key = self.identity.keyring_store_key()?;
        self.load_from_keyring(&key)
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        let key = self.identity.keyring_store_key()?;
        // Simpler error mapping per style: prefer method reference over closure
        let serialized = serde_json::to_string(auth).map_err(std::io::Error::other)?;
        self.save_to_keyring(&key, &serialized)?;
        if let Err(err) = delete_file_if_exists(&self.identity.auth_file()?) {
            warn!("failed to remove CLI auth fallback file: {err}");
        }
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        let key = self.identity.keyring_store_key()?;
        let keyring_removed = self
            .keyring_store
            .delete(KEYRING_SERVICE, &key)
            .map_err(|err| {
                std::io::Error::other(format!("failed to delete auth from keyring: {err}"))
            })?;
        let file_removed = delete_file_if_exists(&self.identity.auth_file()?)?;
        Ok(keyring_removed || file_removed)
    }
}

#[derive(Clone)]
struct SecretsKeyringAuthStorage {
    identity: AuthStorageIdentity,
    direct_storage: DirectKeyringAuthStorage,
    secrets_manager: SecretsManager,
}

impl Debug for SecretsKeyringAuthStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretsKeyringAuthStorage")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl SecretsKeyringAuthStorage {
    #[cfg(test)]
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self::new_with_identity(AuthStorageIdentity::capture(codex_home), keyring_store)
    }

    fn new_with_identity(
        identity: AuthStorageIdentity,
        keyring_store: Arc<dyn KeyringStore>,
    ) -> Self {
        let direct_storage = DirectKeyringAuthStorage::new_with_identity(
            identity.clone(),
            Arc::clone(&keyring_store),
        );
        let secrets_manager = SecretsManager::new_with_keyring_store_and_namespace(
            identity.profile_storage_home_unchecked(),
            SecretsBackendKind::Local,
            keyring_store,
            LocalSecretsNamespace::CodexAuth,
        );
        Self {
            identity,
            direct_storage,
            secrets_manager,
        }
    }
}

impl AuthStorageBackend for SecretsKeyringAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.identity.profile.validate()?;
        match self
            .secrets_manager
            .get(&SecretScope::Global, &CODEX_AUTH_SECRET_NAME)
            .map_err(|err| {
                std::io::Error::other(format!(
                    "failed to load CLI auth from encrypted auth storage: {err}"
                ))
            })? {
            Some(serialized) => serde_json::from_str(&serialized).map(Some).map_err(|err| {
                std::io::Error::other(format!(
                    "failed to deserialize CLI auth from encrypted auth storage: {err}"
                ))
            }),
            None => Ok(None),
        }
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.identity.profile.validate()?;
        let serialized = serde_json::to_string(auth).map_err(std::io::Error::other)?;
        self.secrets_manager
            .set(&SecretScope::Global, &CODEX_AUTH_SECRET_NAME, &serialized)
            .map_err(|err| {
                let message =
                    format!("failed to write OAuth tokens to encrypted auth storage: {err}");
                warn!("{message}");
                std::io::Error::other(message)
            })?;
        if let Err(err) = delete_file_if_exists(&self.identity.auth_file()?) {
            warn!("failed to remove CLI auth fallback file: {err}");
        }
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        self.identity.profile.validate()?;
        let keyring_removed = self
            .secrets_manager
            .delete(&SecretScope::Global, &CODEX_AUTH_SECRET_NAME)
            .map_err(|err| {
                std::io::Error::other(format!(
                    "failed to delete auth from encrypted auth storage: {err}"
                ))
            })?;
        let file_removed = delete_file_if_exists(&self.identity.auth_file()?)?;
        let direct_removed = self.direct_storage.delete()?;
        Ok(keyring_removed || file_removed || direct_removed)
    }
}

#[derive(Clone, Debug)]
struct AutoAuthStorage {
    keyring_storage: Arc<dyn AuthStorageBackend>,
    file_storage: Arc<FileAuthStorage>,
}

impl AutoAuthStorage {
    #[cfg(test)]
    fn new(
        codex_home: PathBuf,
        keyring_store: Arc<dyn KeyringStore>,
        keyring_backend_kind: AuthKeyringBackendKind,
    ) -> Self {
        Self::new_with_identity(
            AuthStorageIdentity::capture(codex_home),
            keyring_store,
            keyring_backend_kind,
        )
    }

    fn new_with_identity(
        identity: AuthStorageIdentity,
        keyring_store: Arc<dyn KeyringStore>,
        keyring_backend_kind: AuthKeyringBackendKind,
    ) -> Self {
        Self {
            keyring_storage: create_keyring_auth_storage_with_identity(
                identity.clone(),
                keyring_store,
                keyring_backend_kind,
            ),
            file_storage: Arc::new(FileAuthStorage::new_with_identity(identity)),
        }
    }
}

impl AuthStorageBackend for AutoAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_storage.load() {
            Ok(Some(auth)) => Ok(Some(auth)),
            Ok(None) => self.file_storage.load(),
            Err(err) => {
                warn!("failed to load CLI auth from keyring, falling back to file storage: {err}");
                self.file_storage.load()
            }
        }
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        match self.keyring_storage.save(auth) {
            Ok(()) => Ok(()),
            Err(err) => {
                warn!("failed to save auth to keyring, falling back to file storage: {err}");
                self.file_storage.save(auth)
            }
        }
    }

    fn delete(&self) -> std::io::Result<bool> {
        // Keyring storage will delete from disk as well
        self.keyring_storage.delete()
    }
}

// A global in-memory store for mapping codex_home -> AuthDotJson.
static EPHEMERAL_AUTH_STORE: Lazy<Mutex<HashMap<String, AuthDotJson>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct EphemeralAuthStorage {
    identity: AuthStorageIdentity,
}

impl EphemeralAuthStorage {
    fn new_with_identity(identity: AuthStorageIdentity) -> Self {
        Self { identity }
    }

    fn with_store<F, T>(&self, action: F) -> std::io::Result<T>
    where
        F: FnOnce(&mut HashMap<String, AuthDotJson>, String) -> std::io::Result<T>,
    {
        let key = self.identity.keyring_store_key()?;
        let mut store = EPHEMERAL_AUTH_STORE
            .lock()
            .map_err(|_| std::io::Error::other("failed to lock ephemeral auth storage"))?;
        action(&mut store, key)
    }
}

impl AuthStorageBackend for EphemeralAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        self.with_store(|store, key| Ok(store.get(&key).cloned()))
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        self.with_store(|store, key| {
            store.insert(key, auth.clone());
            Ok(())
        })
    }

    fn delete(&self) -> std::io::Result<bool> {
        self.with_store(|store, key| Ok(store.remove(&key).is_some()))
    }
}

pub(super) fn create_auth_storage(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> Arc<dyn AuthStorageBackend> {
    let keyring_store: Arc<dyn KeyringStore> = Arc::new(DefaultKeyringStore);
    create_auth_storage_with_store(codex_home, mode, keyring_store, keyring_backend_kind)
}

fn create_auth_storage_with_store(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
    keyring_store: Arc<dyn KeyringStore>,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> Arc<dyn AuthStorageBackend> {
    create_auth_storage_with_store_and_identity(
        AuthStorageIdentity::capture(codex_home),
        mode,
        keyring_store,
        keyring_backend_kind,
    )
}

fn create_auth_storage_with_store_and_identity(
    identity: AuthStorageIdentity,
    mode: AuthCredentialsStoreMode,
    keyring_store: Arc<dyn KeyringStore>,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> Arc<dyn AuthStorageBackend> {
    let storage: Arc<dyn AuthStorageBackend> = match mode {
        AuthCredentialsStoreMode::File => {
            Arc::new(FileAuthStorage::new_with_identity(identity.clone()))
        }
        AuthCredentialsStoreMode::Keyring => create_keyring_auth_storage_with_identity(
            identity.clone(),
            keyring_store,
            keyring_backend_kind,
        ),
        AuthCredentialsStoreMode::Auto => Arc::new(AutoAuthStorage::new_with_identity(
            identity.clone(),
            keyring_store,
            keyring_backend_kind,
        )),
        AuthCredentialsStoreMode::Ephemeral => {
            Arc::new(EphemeralAuthStorage::new_with_identity(identity.clone()))
        }
    };
    Arc::new(ProfilePolicyAuthStorage { identity, storage })
}

fn create_keyring_auth_storage_with_identity(
    identity: AuthStorageIdentity,
    keyring_store: Arc<dyn KeyringStore>,
    keyring_backend_kind: AuthKeyringBackendKind,
) -> Arc<dyn AuthStorageBackend> {
    match keyring_backend_kind {
        AuthKeyringBackendKind::Direct => Arc::new(DirectKeyringAuthStorage::new_with_identity(
            identity,
            keyring_store,
        )),
        AuthKeyringBackendKind::Secrets => Arc::new(SecretsKeyringAuthStorage::new_with_identity(
            identity,
            keyring_store,
        )),
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
