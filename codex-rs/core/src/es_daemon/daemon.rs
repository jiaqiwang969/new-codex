use endpoint_sec::Client;
use endpoint_sec::Event;
use endpoint_sec::EventRenameDestinationFile;
use endpoint_sec::Message;
use endpoint_sec::sys::es_auth_result_t;
use endpoint_sec::sys::es_event_type_t;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

const POLICY_PATH_ENV_VAR: &str = "CODEX_ES_POLICY_PATH";
const DEFAULT_PROTECTED_ZONES_ENV_VAR: &str = "CODEX_ES_DEFAULT_PROTECTED_ZONES";

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
struct SecurityPolicy {
    #[serde(default)]
    protected_zones: Vec<String>,
    #[serde(default)]
    temporary_overrides: Vec<String>,
    #[serde(default)]
    temporary_override_expirations: BTreeMap<String, i64>,
}

impl SecurityPolicy {
    fn from_environment_defaults() -> Self {
        let protected_zones = std::env::var(DEFAULT_PROTECTED_ZONES_ENV_VAR)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        Self {
            protected_zones,
            temporary_overrides: Vec::new(),
            temporary_override_expirations: BTreeMap::new(),
        }
    }

    fn in_protected_zone(&self, path: &Path) -> bool {
        let normalized = normalize_path_for_match(path);
        self.protected_zones.iter().any(|zone| {
            let normalized_zone = normalize_path_for_match(Path::new(zone));
            path_is_within(&normalized, &normalized_zone)
        })
    }

    fn is_temporarily_overridden(&self, path: &Path, now: i64) -> bool {
        let normalized = normalize_path_for_match(path);
        self.temporary_overrides.iter().any(|override_path| {
            let active = self
                .temporary_override_expirations
                .get(override_path)
                .map(|expires_at| *expires_at > now)
                .unwrap_or(true);
            if !active {
                return false;
            }
            let normalized_override = normalize_path_for_match(Path::new(override_path));
            path_is_within(&normalized, &normalized_override)
        })
    }

    fn is_protected(&self, path: &Path, now: i64) -> bool {
        self.in_protected_zone(path) && !self.is_temporarily_overridden(path, now)
    }

    fn prune_expired_overrides(&mut self, now: i64) -> bool {
        let before_overrides = self.temporary_overrides.len();
        let expirations = self.temporary_override_expirations.clone();
        self.temporary_overrides.retain(|entry| {
            expirations
                .get(entry)
                .map(|expires_at| *expires_at > now)
                .unwrap_or(true)
        });

        let before_expirations = self.temporary_override_expirations.len();
        self.temporary_override_expirations
            .retain(|entry, expires_at| {
                *expires_at > now
                    && self
                        .temporary_overrides
                        .iter()
                        .any(|override_path| override_path == entry)
            });

        self.temporary_overrides.len() != before_overrides
            || self.temporary_override_expirations.len() != before_expirations
    }
}

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn resolve_policy_path() -> PathBuf {
    if let Ok(path) = std::env::var(POLICY_PATH_ENV_VAR) {
        return PathBuf::from(path);
    }

    if let Ok(home_dir) = std::env::var("HOME") {
        return PathBuf::from(home_dir)
            .join(".codex")
            .join("es_policy.json");
    }

    PathBuf::from("/var/root/.codex/es_policy.json")
}

fn normalize_path_for_match(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    if let Some(parent) = path.parent()
        && let Ok(canonical_parent) = parent.canonicalize()
    {
        if let Some(name) = path.file_name() {
            return canonical_parent.join(name);
        }
        return canonical_parent;
    }

    path.to_path_buf()
}

fn path_is_within(path: &Path, prefix: &Path) -> bool {
    path == prefix || path.starts_with(prefix)
}

fn load_policy(policy_path: &Path) -> Option<SecurityPolicy> {
    let content = fs::read_to_string(policy_path).ok()?;
    serde_json::from_str(&content).ok()
}

fn merge_default_protected_zones(policy: &mut SecurityPolicy) -> bool {
    let mut changed = false;
    for zone in SecurityPolicy::from_environment_defaults().protected_zones {
        let normalized_zone = normalize_path_for_match(Path::new(zone.as_str()));
        let already_present = policy.protected_zones.iter().any(|existing_zone| {
            normalize_path_for_match(Path::new(existing_zone.as_str())) == normalized_zone
        });
        if !already_present {
            policy
                .protected_zones
                .push(normalized_zone.to_string_lossy().to_string());
            changed = true;
        }
    }
    changed
}

fn persist_policy(policy_path: &Path, policy: &SecurityPolicy) -> std::io::Result<()> {
    if let Some(parent) = policy_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(policy)
        .map_err(|err| std::io::Error::other(format!("failed to serialize policy: {err}")))?;
    fs::write(policy_path, content)
}

fn ensure_policy_file(policy_path: &Path) -> SecurityPolicy {
    if let Some(mut policy) = load_policy(policy_path) {
        if merge_default_protected_zones(&mut policy)
            && let Err(err) = persist_policy(policy_path, &policy)
        {
            tracing::warn!("failed to persist merged default protected zones: {err}");
        }
        return policy;
    }

    let policy = SecurityPolicy::from_environment_defaults();
    if let Err(err) = persist_policy(policy_path, &policy) {
        tracing::warn!("failed to initialize {policy_path:?}: {err}");
    }
    policy
}

fn is_exempted_temp(path: &Path) -> bool {
    let as_text = path.to_string_lossy();
    as_text.contains("/.Trash/")
        || as_text.contains("/tmp/")
        || as_text.contains("/private/tmp/")
        || as_text.contains("/var/folders/")
        || as_text.contains("/private/var/folders/")
        || as_text.contains("/.cache/")
        || as_text.contains("/target/")
        || as_text.contains("/node_modules/")
        || as_text.contains("/result/")
        || as_text.contains("/.git/")
}

fn is_editor_temporary(path: &Path) -> bool {
    let as_text = path.to_string_lossy();
    as_text.contains(".swp") || as_text.ends_with('~') || as_text.contains(".tmp")
}

fn rename_destination_path(destination: Option<EventRenameDestinationFile<'_>>) -> Option<PathBuf> {
    match destination {
        Some(EventRenameDestinationFile::ExistingFile(file)) => {
            Some(normalize_path_for_match(Path::new(file.path())))
        }
        Some(EventRenameDestinationFile::NewPath {
            directory,
            filename,
        }) => {
            let path = PathBuf::from(directory.path()).join(filename.to_string_lossy().as_ref());
            Some(normalize_path_for_match(&path))
        }
        None => None,
    }
}

pub fn run_daemon() -> anyhow::Result<()> {
    let policy_path = resolve_policy_path();
    let mut initial_policy = ensure_policy_file(&policy_path);
    if initial_policy.prune_expired_overrides(current_unix_timestamp()) {
        let _ = persist_policy(&policy_path, &initial_policy);
    }
    let shared_policy = Arc::new(Mutex::new(initial_policy));

    let policy_clone = Arc::clone(&shared_policy);
    let path_clone = policy_path;
    thread::spawn(move || {
        let mut last_mtime = SystemTime::UNIX_EPOCH;
        loop {
            if let Ok(metadata) = fs::metadata(&path_clone)
                && let Ok(mtime) = metadata.modified()
                && mtime != last_mtime
            {
                if let Some(mut new_policy) = load_policy(&path_clone) {
                    let now = current_unix_timestamp();
                    if merge_default_protected_zones(&mut new_policy)
                        || new_policy.prune_expired_overrides(now)
                    {
                        let _ = persist_policy(&path_clone, &new_policy);
                    }
                    if let Ok(mut lock) = policy_clone.lock() {
                        *lock = new_policy;
                        tracing::info!("Endpoint Security policy updated.");
                    }
                }
                last_mtime = mtime;
            }

            if let Ok(mut lock) = policy_clone.lock()
                && lock.prune_expired_overrides(current_unix_timestamp())
            {
                let _ = persist_policy(&path_clone, &lock);
            }

            thread::sleep(Duration::from_secs(1));
        }
    });

    let policy_for_handler = Arc::clone(&shared_policy);
    let handler = move |client: &mut Client<'_>, message: Message| {
        let now = current_unix_timestamp();
        let current_policy = match policy_for_handler.lock() {
            Ok(lock) => lock.clone(),
            Err(_) => SecurityPolicy::default(),
        };

        match message.event() {
            Some(Event::AuthUnlink(unlink)) => {
                let target_path = normalize_path_for_match(Path::new(unlink.target().path()));

                if current_policy.is_protected(&target_path, now) && !is_exempted_temp(&target_path)
                {
                    tracing::info!(
                        "[Codex ES Daemon] Blocked physical deletion of protected path: {}",
                        target_path.display()
                    );
                    let _ = client.respond_auth_result(
                        &message,
                        es_auth_result_t::ES_AUTH_RESULT_DENY,
                        false,
                    );
                } else {
                    let _ = client.respond_auth_result(
                        &message,
                        es_auth_result_t::ES_AUTH_RESULT_ALLOW,
                        false,
                    );
                }
            }
            Some(Event::AuthRename(rename)) => {
                let source_path = normalize_path_for_match(Path::new(rename.source().path()));
                let destination_path = rename_destination_path(rename.destination());

                if !current_policy.is_protected(&source_path, now)
                    || is_exempted_temp(&source_path)
                    || is_editor_temporary(&source_path)
                {
                    let _ = client.respond_auth_result(
                        &message,
                        es_auth_result_t::ES_AUTH_RESULT_ALLOW,
                        false,
                    );
                } else if let Some(destination_path) = destination_path {
                    if !current_policy.in_protected_zone(&destination_path) {
                        tracing::info!(
                            "[Codex ES Daemon] Blocked move out of protected zone: {} -> {}",
                            source_path.display(),
                            destination_path.display()
                        );
                        let _ = client.respond_auth_result(
                            &message,
                            es_auth_result_t::ES_AUTH_RESULT_DENY,
                            false,
                        );
                    } else {
                        let _ = client.respond_auth_result(
                            &message,
                            es_auth_result_t::ES_AUTH_RESULT_ALLOW,
                            false,
                        );
                    }
                } else {
                    let _ = client.respond_auth_result(
                        &message,
                        es_auth_result_t::ES_AUTH_RESULT_ALLOW,
                        false,
                    );
                }
            }
            _ => {
                let _ = client.respond_auth_result(
                    &message,
                    es_auth_result_t::ES_AUTH_RESULT_ALLOW,
                    false,
                );
            }
        }
    };

    tracing::info!("Attempting to register Endpoint Security client.");
    let mut client = Client::new(handler).map_err(|err| {
        anyhow::anyhow!(
            "failed to create ES client (requires root + endpoint-security entitlements): {err:?}"
        )
    })?;

    client
        .subscribe(&[
            es_event_type_t::ES_EVENT_TYPE_AUTH_UNLINK,
            es_event_type_t::ES_EVENT_TYPE_AUTH_RENAME,
        ])
        .map_err(|err| anyhow::anyhow!("failed to subscribe to ES events: {err:?}"))?;

    tracing::info!("Codex ES daemon started successfully.");

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
