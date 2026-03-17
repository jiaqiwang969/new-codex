use crate::security_host::RuntimeDeniedEffect;
use crate::security_host::SecurityHost;
use crate::security_types::PredictedEffect;
use crate::security_types::PredictedEffectKind;
use crate::security_types::SecurityCapabilitySnapshot;
use crate::security_types::SecurityMismatch;
use crate::security_types::SecurityPermit;
use crate::security_types::SecurityPermitScope;
use codex_utils_absolute_path::AbsolutePathBuf;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum LegacyRuntimeDeniedEffect {
    ProtectedDelete {
        target_path: PathBuf,
        process_name: Option<String>,
        ancestor_name: Option<String>,
    },
    ProtectedMoveOut {
        source_path: PathBuf,
        destination_path: PathBuf,
        process_name: Option<String>,
        ancestor_name: Option<String>,
    },
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

fn absolute_path_buf(path: &Path) -> Option<AbsolutePathBuf> {
    AbsolutePathBuf::try_from(normalize_path_for_match(path)).ok()
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

fn legacy_runtime_capability_snapshot(policy: &SecurityPolicy) -> SecurityCapabilitySnapshot {
    SecurityCapabilitySnapshot {
        protected_zones: policy
            .protected_zones
            .iter()
            .filter_map(|zone| absolute_path_buf(Path::new(zone.as_str())))
            .collect(),
        transfer_gate_enabled: true,
        ..Default::default()
    }
}

fn legacy_runtime_mismatch(
    policy: &SecurityPolicy,
    permit: Option<&SecurityPermit>,
    predicted_effects: Vec<PredictedEffect>,
    denied_effect: LegacyRuntimeDeniedEffect,
) -> SecurityMismatch {
    let host = SecurityHost::new(legacy_runtime_capability_snapshot(policy));
    match denied_effect {
        LegacyRuntimeDeniedEffect::ProtectedDelete {
            target_path,
            process_name,
            ancestor_name,
        } => host.runtime_mismatch_for_denial(
            permit,
            predicted_effects,
            RuntimeDeniedEffect {
                actual_kind: PredictedEffectKind::ProtectedDelete,
                actual_scope: SecurityPermitScope {
                    target_path: absolute_path_buf(&target_path),
                    source_path: None,
                    destination_path: None,
                    tool_name: None,
                    process_name: process_name.clone(),
                    trusted_identity: None,
                    recursive: false,
                },
                process_name,
                ancestor_name,
                summary: format!(
                    "Endpoint Security blocked protected delete: {}",
                    target_path.display()
                ),
            },
        ),
        LegacyRuntimeDeniedEffect::ProtectedMoveOut {
            source_path,
            destination_path,
            process_name,
            ancestor_name,
        } => host.runtime_mismatch_for_denial(
            permit,
            predicted_effects,
            RuntimeDeniedEffect {
                actual_kind: PredictedEffectKind::ProtectedMoveOut,
                actual_scope: SecurityPermitScope {
                    target_path: None,
                    source_path: absolute_path_buf(&source_path),
                    destination_path: absolute_path_buf(&destination_path),
                    tool_name: None,
                    process_name: process_name.clone(),
                    trusted_identity: None,
                    recursive: false,
                },
                process_name,
                ancestor_name,
                summary: format!(
                    "Endpoint Security blocked move out of protected zone: {} -> {}",
                    source_path.display(),
                    destination_path.display()
                ),
            },
        ),
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
                    let mismatch = legacy_runtime_mismatch(
                        &current_policy,
                        None,
                        Vec::new(),
                        LegacyRuntimeDeniedEffect::ProtectedDelete {
                            target_path: target_path.clone(),
                            process_name: Some("unlink".to_string()),
                            ancestor_name: None,
                        },
                    );
                    tracing::info!(
                        classification = ?mismatch.classification,
                        reason_code = %mismatch.actual_reason_code,
                        summary = %mismatch.summary,
                        "[Codex ES Daemon] Blocked physical deletion of protected path: {}",
                        target_path.display(),
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
                        let mismatch = legacy_runtime_mismatch(
                            &current_policy,
                            None,
                            Vec::new(),
                            LegacyRuntimeDeniedEffect::ProtectedMoveOut {
                                source_path: source_path.clone(),
                                destination_path: destination_path.clone(),
                                process_name: Some("mv".to_string()),
                                ancestor_name: None,
                            },
                        );
                        tracing::info!(
                            classification = ?mismatch.classification,
                            reason_code = %mismatch.actual_reason_code,
                            summary = %mismatch.summary,
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

#[cfg(test)]
mod tests {
    use super::LegacyRuntimeDeniedEffect;
    use super::SecurityPolicy;
    use super::legacy_runtime_capability_snapshot;
    use super::legacy_runtime_mismatch;
    use crate::security_types::PredictedEffect;
    use crate::security_types::PredictedEffectKind;
    use crate::security_types::SecurityMismatch;
    use crate::security_types::SecurityMismatchClassification;
    use crate::security_types::SecurityPermit;
    use crate::security_types::SecurityPermitScope;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn policy_with_documents_zone() -> SecurityPolicy {
        SecurityPolicy {
            protected_zones: vec!["/Users/demo/Documents".to_string()],
            temporary_overrides: Vec::new(),
            temporary_override_expirations: BTreeMap::new(),
        }
    }

    #[test]
    fn legacy_runtime_capability_snapshot_reports_limited_es_scope() {
        let snapshot = legacy_runtime_capability_snapshot(&policy_with_documents_zone());

        assert_eq!(
            snapshot,
            crate::security_types::SecurityCapabilitySnapshot {
                protected_zones: vec![AbsolutePathBuf::try_from("/Users/demo/Documents").unwrap()],
                transfer_gate_enabled: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn legacy_runtime_delete_denial_becomes_true_risk_mismatch() {
        let mismatch = legacy_runtime_mismatch(
            &policy_with_documents_zone(),
            None,
            Vec::new(),
            LegacyRuntimeDeniedEffect::ProtectedDelete {
                target_path: PathBuf::from("/Users/demo/Documents/report.txt"),
                process_name: Some("rm".to_string()),
                ancestor_name: Some("python".to_string()),
            },
        );

        assert_eq!(
            mismatch,
            SecurityMismatch {
                permit_id: None,
                predicted_effects: Vec::new(),
                actual_kind: PredictedEffectKind::ProtectedDelete,
                actual_reason_code: "es_protected_delete".to_string(),
                actual_scope: SecurityPermitScope {
                    target_path: Some(
                        AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
                    ),
                    source_path: None,
                    destination_path: None,
                    tool_name: None,
                    process_name: Some("rm".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                classification: SecurityMismatchClassification::TrueRisk,
                process_name: Some("rm".to_string()),
                ancestor_name: Some("python".to_string()),
                summary:
                    "Endpoint Security blocked protected delete: /Users/demo/Documents/report.txt"
                        .to_string(),
            }
        );
    }

    #[test]
    fn legacy_runtime_move_out_permit_miss_is_underpredicted() {
        let permit = SecurityPermit {
            id: "permit-1".to_string(),
            kind: PredictedEffectKind::ProtectedMoveOut,
            scope: SecurityPermitScope {
                target_path: None,
                source_path: Some(
                    AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
                ),
                destination_path: Some(AbsolutePathBuf::try_from("/tmp/report.txt").unwrap()),
                tool_name: Some("shell".to_string()),
                process_name: Some("mv".to_string()),
                trusted_identity: Some("apple.codesign:Terminal".to_string()),
                recursive: false,
            },
            issued_at: 1_710_000_000,
            expires_at: 1_710_000_120,
            issuer: "security-host".to_string(),
            risk_score: 18,
            justification: "Low-risk narrow smart-access permit.".to_string(),
            thread_id: "thread-123".to_string(),
            turn_id: "turn-456".to_string(),
        };
        let predicted_effect = PredictedEffect {
            kind: PredictedEffectKind::ProtectedMoveOut,
            scope: permit.scope.clone(),
            confidence: 91,
            why: "Moves the protected file into the export zone.".to_string(),
        };

        let mismatch = legacy_runtime_mismatch(
            &policy_with_documents_zone(),
            Some(&permit),
            vec![predicted_effect.clone()],
            LegacyRuntimeDeniedEffect::ProtectedMoveOut {
                source_path: PathBuf::from("/Users/demo/Documents/report.txt"),
                destination_path: PathBuf::from("/Users/demo/Desktop/report.txt"),
                process_name: Some("mv".to_string()),
                ancestor_name: Some("python".to_string()),
            },
        );

        assert_eq!(
            mismatch,
            SecurityMismatch {
                permit_id: Some("permit-1".to_string()),
                predicted_effects: vec![predicted_effect],
                actual_kind: PredictedEffectKind::ProtectedMoveOut,
                actual_reason_code: "permit_miss_protected_move_out".to_string(),
                actual_scope: SecurityPermitScope {
                    target_path: None,
                    source_path: Some(
                        AbsolutePathBuf::try_from("/Users/demo/Documents/report.txt").unwrap(),
                    ),
                    destination_path: Some(
                        AbsolutePathBuf::try_from("/Users/demo/Desktop/report.txt").unwrap(),
                    ),
                    tool_name: None,
                    process_name: Some("mv".to_string()),
                    trusted_identity: None,
                    recursive: false,
                },
                classification: SecurityMismatchClassification::Underpredicted,
                process_name: Some("mv".to_string()),
                ancestor_name: Some("python".to_string()),
                summary:
                    "Endpoint Security blocked move out of protected zone: /Users/demo/Documents/report.txt -> /Users/demo/Desktop/report.txt"
                        .to_string(),
            }
        );
    }
}
