use crate::team_profile_vouch::TeamProfileTaskBucket;
use crate::team_profile_vouch::TeamProfileVouchVerdict;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tracing::warn;

const MODEL_SUB_VOUCH_REL_PATH: &str = "memories/model_sub_vouch.json";
const MAX_STORED_RECENT_EVENTS: usize = 200;
const RECENT_EVENT_WINDOW: usize = 20;

#[derive(Debug, Clone, Default)]
pub(crate) struct ModelSubVouchSnapshot {
    entries: BTreeMap<String, ModelSubVouchEntry>,
    load_error: Option<String>,
}

impl ModelSubVouchSnapshot {
    pub(crate) fn entry_for(&self, model: &str) -> Option<&ModelSubVouchEntry> {
        self.entries.get(model)
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn has_signal(&self, task_bucket: Option<TeamProfileTaskBucket>) -> bool {
        self.entries.values().any(|entry| {
            if let Some(recent) = entry.recent_signal(task_bucket)
                && recent.sample_count() > 0
            {
                return true;
            }
            if task_bucket.is_none() && entry.sample_count() > 0 {
                return true;
            }
            if let Some(task_bucket) = task_bucket
                && let Some(task_entry) = entry.task_entry(task_bucket)
                && task_entry.sample_count() > 0
            {
                return true;
            }
            false
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ModelSubVouchEntry {
    pub(crate) wins: u32,
    pub(crate) losses: u32,
    pub(crate) note: Option<String>,
    pub(crate) by_task: BTreeMap<String, ModelSubTaskVouchEntry>,
    pub(crate) recent_events: Vec<ModelSubVouchEvent>,
}

impl ModelSubVouchEntry {
    pub(crate) fn net_score(&self) -> i64 {
        i64::from(self.wins) - i64::from(self.losses)
    }

    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }

    pub(crate) fn task_entry(
        &self,
        task_bucket: TeamProfileTaskBucket,
    ) -> Option<&ModelSubTaskVouchEntry> {
        self.by_task.get(task_bucket.key())
    }

    pub(crate) fn recent_signal(
        &self,
        task_bucket: Option<TeamProfileTaskBucket>,
    ) -> Option<ModelSubRecentSignal> {
        let recent_events: Vec<&ModelSubVouchEvent> = self
            .recent_events
            .iter()
            .rev()
            .filter(|event| match task_bucket {
                Some(task_bucket) => event.task_bucket.as_deref() == Some(task_bucket.key()),
                None => true,
            })
            .take(RECENT_EVENT_WINDOW)
            .collect();
        if recent_events.is_empty() {
            return None;
        }

        let mut weighted_score = 0_i64;
        let mut wins = 0_u32;
        let mut losses = 0_u32;
        let total = recent_events.len();
        for (index, event) in recent_events.into_iter().enumerate() {
            let weight = i64::try_from(total.saturating_sub(index)).unwrap_or(i64::MAX);
            match event.verdict {
                TeamProfileVouchVerdict::Win => {
                    wins = wins.saturating_add(1);
                    weighted_score = weighted_score.saturating_add(weight);
                }
                TeamProfileVouchVerdict::Loss => {
                    losses = losses.saturating_add(1);
                    weighted_score = weighted_score.saturating_sub(weight);
                }
            }
        }

        Some(ModelSubRecentSignal {
            weighted_score,
            wins,
            losses,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ModelSubTaskVouchEntry {
    pub(crate) wins: u32,
    pub(crate) losses: u32,
    pub(crate) note: Option<String>,
}

impl ModelSubTaskVouchEntry {
    pub(crate) fn net_score(&self) -> i64 {
        i64::from(self.wins) - i64::from(self.losses)
    }

    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ModelSubVouchEvent {
    pub(crate) verdict: TeamProfileVouchVerdict,
    pub(crate) task_bucket: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ModelSubRecentSignal {
    pub(crate) weighted_score: i64,
    pub(crate) wins: u32,
    pub(crate) losses: u32,
}

impl ModelSubRecentSignal {
    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ModelSubVouchLedger {
    models: BTreeMap<String, ModelSubVouchEntry>,
}

pub(crate) fn load_model_sub_vouch(codex_home: &Path) -> ModelSubVouchSnapshot {
    let path = model_sub_vouch_path(codex_home);
    let raw = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ModelSubVouchSnapshot::default();
        }
        Err(err) => {
            let message = format!("failed to read {}: {err}", path.display());
            warn!("{message}");
            return ModelSubVouchSnapshot {
                entries: BTreeMap::new(),
                load_error: Some(message),
            };
        }
    };

    match serde_json::from_str::<ModelSubVouchLedger>(&raw) {
        Ok(ledger) => ModelSubVouchSnapshot {
            entries: ledger.models,
            load_error: None,
        },
        Err(err) => {
            let message = format!("failed to parse {}: {err}", path.display());
            warn!("{message}");
            ModelSubVouchSnapshot {
                entries: BTreeMap::new(),
                load_error: Some(message),
            }
        }
    }
}

pub(crate) fn record_model_sub_vouch(
    codex_home: &Path,
    model: &str,
    verdict: TeamProfileVouchVerdict,
    task_bucket: Option<TeamProfileTaskBucket>,
    note: Option<&str>,
) -> Result<ModelSubVouchEntry, String> {
    let path = model_sub_vouch_path(codex_home);
    let mut ledger = match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<ModelSubVouchLedger>(&content)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ModelSubVouchLedger::default(),
        Err(err) => {
            return Err(format!("failed to read {}: {err}", path.display()));
        }
    };

    let entry = ledger.models.entry(model.to_string()).or_default();
    match verdict {
        TeamProfileVouchVerdict::Win => entry.wins = entry.wins.saturating_add(1),
        TeamProfileVouchVerdict::Loss => entry.losses = entry.losses.saturating_add(1),
    }
    if let Some(task_bucket) = task_bucket {
        let task_entry = entry
            .by_task
            .entry(task_bucket.key().to_string())
            .or_default();
        match verdict {
            TeamProfileVouchVerdict::Win => task_entry.wins = task_entry.wins.saturating_add(1),
            TeamProfileVouchVerdict::Loss => {
                task_entry.losses = task_entry.losses.saturating_add(1);
            }
        }
        if let Some(note) = note.map(str::trim)
            && !note.is_empty()
        {
            task_entry.note = Some(note.to_string());
        }
    }
    entry.recent_events.push(ModelSubVouchEvent {
        verdict,
        task_bucket: task_bucket.map(|task_bucket| task_bucket.key().to_string()),
    });
    if entry.recent_events.len() > MAX_STORED_RECENT_EVENTS {
        let excess = entry
            .recent_events
            .len()
            .saturating_sub(MAX_STORED_RECENT_EVENTS);
        entry.recent_events.drain(..excess);
    }
    if let Some(note) = note.map(str::trim)
        && !note.is_empty()
    {
        entry.note = Some(note.to_string());
    }

    let updated_entry = entry.clone();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create model vouch directory {}: {err}",
                parent.display()
            )
        })?;
    }
    let serialized = serde_json::to_string_pretty(&ledger)
        .map_err(|err| format!("failed to serialize model-sub vouch ledger: {err}"))?;
    fs::write(&path, format!("{serialized}\n"))
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;

    Ok(updated_entry)
}

pub(crate) fn recommended_model_sub<'a>(
    vouch_snapshot: &ModelSubVouchSnapshot,
    task_bucket: Option<TeamProfileTaskBucket>,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut recommended: Option<String> = None;
    let mut recommended_rank: Option<(i64, u32, u32, i64, u32, u32)> = None;

    for candidate in candidates {
        let rank = if let Some(entry) = vouch_snapshot.entry_for(candidate) {
            let (global_recent_score, global_recent_wins, global_recent_samples) = entry
                .recent_signal(None)
                .map(|signal| (signal.weighted_score, signal.wins, signal.sample_count()))
                .unwrap_or((entry.net_score(), entry.wins, entry.sample_count()));
            let (task_recent_score, task_recent_wins, task_recent_samples) = task_bucket
                .and_then(|bucket| entry.recent_signal(Some(bucket)))
                .map(|signal| (signal.weighted_score, signal.wins, signal.sample_count()))
                .unwrap_or_else(|| {
                    task_bucket
                        .and_then(|bucket| entry.task_entry(bucket))
                        .map(|task_entry| {
                            (
                                task_entry.net_score(),
                                task_entry.wins,
                                task_entry.sample_count(),
                            )
                        })
                        .unwrap_or((0, 0, 0))
                });
            if task_bucket.is_some() {
                (
                    task_recent_score,
                    task_recent_wins,
                    task_recent_samples,
                    global_recent_score,
                    global_recent_wins,
                    global_recent_samples,
                )
            } else {
                (
                    global_recent_score,
                    global_recent_wins,
                    global_recent_samples,
                    task_recent_score,
                    task_recent_wins,
                    task_recent_samples,
                )
            }
        } else {
            (0, 0, 0, 0, 0, 0)
        };
        if recommended_rank.is_none_or(|current| rank > current) {
            recommended = Some(candidate.to_string());
            recommended_rank = Some(rank);
        }
    }

    if let Some(recommended_rank) = recommended_rank
        && recommended_rank == (0, 0, 0, 0, 0, 0)
        && !vouch_snapshot.has_signal(task_bucket)
    {
        return None;
    }

    recommended
}

pub(crate) fn recommended_model_sub_from_snapshot(
    vouch_snapshot: &ModelSubVouchSnapshot,
    task_bucket: Option<TeamProfileTaskBucket>,
) -> Option<String> {
    let candidates = vouch_snapshot
        .entries
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    recommended_model_sub(vouch_snapshot, task_bucket, candidates)
}

fn model_sub_vouch_path(codex_home: &Path) -> PathBuf {
    codex_home.join(MODEL_SUB_VOUCH_REL_PATH)
}

#[cfg(test)]
mod tests {
    use super::load_model_sub_vouch;
    use super::recommended_model_sub;
    use super::recommended_model_sub_from_snapshot;
    use super::record_model_sub_vouch;
    use crate::team_profile_vouch::TeamProfileTaskBucket;
    use crate::team_profile_vouch::TeamProfileVouchVerdict;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_file_returns_empty_snapshot() {
        let home = tempdir().expect("tempdir");
        let snapshot = load_model_sub_vouch(home.path());
        assert_eq!(snapshot.load_error(), None);
        assert!(snapshot.entry_for("claude-sonnet-4-6").is_none());
    }

    #[test]
    fn record_vouch_updates_counts_and_note() {
        let home = tempdir().expect("tempdir");
        let updated = record_model_sub_vouch(
            home.path(),
            "claude-sonnet-4-6",
            TeamProfileVouchVerdict::Win,
            Some(TeamProfileTaskBucket::General),
            Some("fast and good"),
        )
        .expect("record model vouch");
        assert_eq!(updated.wins, 1);
        assert_eq!(updated.losses, 0);
        assert_eq!(updated.note.as_deref(), Some("fast and good"));

        let snapshot = load_model_sub_vouch(home.path());
        let entry = snapshot
            .entry_for("claude-sonnet-4-6")
            .expect("model should exist");
        assert_eq!(entry.wins, 1);
        assert_eq!(entry.losses, 0);
        assert_eq!(entry.note.as_deref(), Some("fast and good"));
        let general = entry
            .task_entry(TeamProfileTaskBucket::General)
            .expect("general bucket should exist");
        assert_eq!(general.wins, 1);
        assert_eq!(general.losses, 0);
        assert_eq!(general.note.as_deref(), Some("fast and good"));
    }

    #[test]
    fn recommended_model_prefers_recent_weighted_score_for_bucket() {
        let home = tempdir().expect("tempdir");
        let memories_dir = home.path().join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(
            memories_dir.join("model_sub_vouch.json"),
            r#"{
  "models": {
    "claude-sonnet-4-6": {
      "wins": 8,
      "losses": 1,
      "by_task": { "debug": { "wins": 6, "losses": 0 } },
      "recent_events": [
        { "verdict": "Loss", "task_bucket": "debug" },
        { "verdict": "Loss", "task_bucket": "debug" }
      ]
    },
    "gpt-5.2-codex": {
      "wins": 2,
      "losses": 2,
      "by_task": { "debug": { "wins": 1, "losses": 1 } },
      "recent_events": [
        { "verdict": "Win", "task_bucket": "debug" },
        { "verdict": "Win", "task_bucket": "debug" }
      ]
    }
  }
}"#,
        )
        .expect("write vouch file");
        let snapshot = load_model_sub_vouch(home.path());
        let candidates = vec!["claude-sonnet-4-6", "gpt-5.2-codex"];
        let recommended = recommended_model_sub(
            &snapshot,
            Some(TeamProfileTaskBucket::Debug),
            candidates.iter().copied(),
        );
        assert_eq!(recommended.as_deref(), Some("gpt-5.2-codex"));
    }

    #[test]
    fn recommended_model_returns_none_without_signal() {
        let home = tempdir().expect("tempdir");
        let snapshot = load_model_sub_vouch(home.path());
        let candidates = vec!["claude-sonnet-4-6", "gpt-5.2-codex"];
        let recommended = recommended_model_sub(&snapshot, None, candidates.iter().copied());
        assert_eq!(recommended, None);
    }

    #[test]
    fn recommended_model_from_snapshot_uses_all_known_entries() {
        let home = tempdir().expect("tempdir");
        let memories_dir = home.path().join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(
            memories_dir.join("model_sub_vouch.json"),
            r#"{
  "models": {
    "claude-sonnet-4-6": {
      "wins": 3,
      "losses": 0,
      "recent_events": [
        { "verdict": "Win" }
      ]
    },
    "gpt-5.2-codex": {
      "wins": 1,
      "losses": 2,
      "recent_events": [
        { "verdict": "Loss" }
      ]
    }
  }
}"#,
        )
        .expect("write vouch file");
        let snapshot = load_model_sub_vouch(home.path());
        let recommended = recommended_model_sub_from_snapshot(&snapshot, None);
        assert_eq!(recommended.as_deref(), Some("claude-sonnet-4-6"));
    }
}
