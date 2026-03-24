use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tracing::warn;

const TEAM_PROFILE_VOUCH_REL_PATH: &str = "memories/team_profile_vouch.json";
const MAX_STORED_RECENT_EVENTS: usize = 200;
const RECENT_EVENT_WINDOW: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum TeamProfileVouchVerdict {
    Win,
    Loss,
}

impl Default for TeamProfileVouchVerdict {
    fn default() -> Self {
        Self::Win
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamProfileTaskBucket {
    General,
    Debug,
    Review,
}

impl TeamProfileTaskBucket {
    pub(crate) const ALL: [Self; 3] = [Self::General, Self::Debug, Self::Review];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Debug => "debug",
            Self::Review => "review",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Debug => "debug",
            Self::Review => "review",
        }
    }

    pub(crate) fn from_selector(value: &str) -> Option<Self> {
        match value.trim() {
            "general" => Some(Self::General),
            "debug" => Some(Self::Debug),
            "review" => Some(Self::Review),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TeamProfileVouchSnapshot {
    entries: BTreeMap<String, TeamProfileVouchEntry>,
    load_error: Option<String>,
}

impl TeamProfileVouchSnapshot {
    pub(crate) fn entry_for(&self, profile_key: &str) -> Option<&TeamProfileVouchEntry> {
        self.entries.get(profile_key)
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn has_signal(&self) -> bool {
        self.entries.values().any(|entry| {
            entry.sample_count() > 0
                || entry
                    .by_task
                    .values()
                    .any(|task_entry| task_entry.sample_count() > 0)
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct TeamProfileVouchEntry {
    pub(crate) wins: u32,
    pub(crate) losses: u32,
    pub(crate) note: Option<String>,
    pub(crate) by_task: BTreeMap<String, TeamProfileTaskVouchEntry>,
    pub(crate) recent_events: Vec<TeamProfileVouchEvent>,
}

impl TeamProfileVouchEntry {
    pub(crate) fn net_score(&self) -> i64 {
        i64::from(self.wins) - i64::from(self.losses)
    }

    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }

    pub(crate) fn task_entry(
        &self,
        task_bucket: TeamProfileTaskBucket,
    ) -> Option<&TeamProfileTaskVouchEntry> {
        self.by_task.get(task_bucket.key())
    }

    pub(crate) fn recent_signal(
        &self,
        task_bucket: Option<TeamProfileTaskBucket>,
    ) -> Option<TeamProfileRecentSignal> {
        let recent_events: Vec<&TeamProfileVouchEvent> = self
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

        Some(TeamProfileRecentSignal {
            weighted_score,
            wins,
            losses,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct TeamProfileTaskVouchEntry {
    pub(crate) wins: u32,
    pub(crate) losses: u32,
    pub(crate) note: Option<String>,
}

impl TeamProfileTaskVouchEntry {
    pub(crate) fn net_score(&self) -> i64 {
        i64::from(self.wins) - i64::from(self.losses)
    }

    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct TeamProfileVouchEvent {
    pub(crate) verdict: TeamProfileVouchVerdict,
    pub(crate) task_bucket: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct TeamProfileRecentSignal {
    pub(crate) weighted_score: i64,
    pub(crate) wins: u32,
    pub(crate) losses: u32,
}

impl TeamProfileRecentSignal {
    pub(crate) fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct TeamProfileVouchLedger {
    profiles: BTreeMap<String, TeamProfileVouchEntry>,
}

pub(crate) fn load_team_profile_vouch(codex_home: &Path) -> TeamProfileVouchSnapshot {
    let path = team_profile_vouch_path(codex_home);
    let raw = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return TeamProfileVouchSnapshot::default();
        }
        Err(err) => {
            let message = format!("failed to read {}: {err}", path.display());
            warn!("{message}");
            return TeamProfileVouchSnapshot {
                entries: BTreeMap::new(),
                load_error: Some(message),
            };
        }
    };

    match serde_json::from_str::<TeamProfileVouchLedger>(&raw) {
        Ok(ledger) => TeamProfileVouchSnapshot {
            entries: ledger.profiles,
            load_error: None,
        },
        Err(err) => {
            let message = format!("failed to parse {}: {err}", path.display());
            warn!("{message}");
            TeamProfileVouchSnapshot {
                entries: BTreeMap::new(),
                load_error: Some(message),
            }
        }
    }
}

pub(crate) fn record_team_profile_vouch(
    codex_home: &Path,
    profile_key: &str,
    verdict: TeamProfileVouchVerdict,
    task_bucket: Option<TeamProfileTaskBucket>,
    note: Option<&str>,
) -> Result<TeamProfileVouchEntry, String> {
    let path = team_profile_vouch_path(codex_home);
    let mut ledger = match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<TeamProfileVouchLedger>(&content)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => TeamProfileVouchLedger::default(),
        Err(err) => {
            return Err(format!("failed to read {}: {err}", path.display()));
        }
    };

    let entry = ledger.profiles.entry(profile_key.to_string()).or_default();
    match verdict {
        TeamProfileVouchVerdict::Win => {
            entry.wins = entry.wins.saturating_add(1);
        }
        TeamProfileVouchVerdict::Loss => {
            entry.losses = entry.losses.saturating_add(1);
        }
    }
    if let Some(task_bucket) = task_bucket {
        let task_entry = entry
            .by_task
            .entry(task_bucket.key().to_string())
            .or_default();
        match verdict {
            TeamProfileVouchVerdict::Win => {
                task_entry.wins = task_entry.wins.saturating_add(1);
            }
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
    entry.recent_events.push(TeamProfileVouchEvent {
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
                "failed to create vouch directory {}: {err}",
                parent.display()
            )
        })?;
    }
    let serialized = serde_json::to_string_pretty(&ledger)
        .map_err(|err| format!("failed to serialize team profile vouch ledger: {err}"))?;
    fs::write(&path, format!("{serialized}\n"))
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(updated_entry)
}

fn team_profile_vouch_path(codex_home: &Path) -> PathBuf {
    codex_home.join(TEAM_PROFILE_VOUCH_REL_PATH)
}

#[cfg(test)]
mod tests {
    use super::TeamProfileTaskBucket;
    use super::TeamProfileVouchVerdict;
    use super::load_team_profile_vouch;
    use super::record_team_profile_vouch;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_file_returns_empty_snapshot() {
        let home = tempdir().expect("tempdir");
        let snapshot = load_team_profile_vouch(home.path());
        assert_eq!(snapshot.load_error(), None);
        assert!(snapshot.entry_for("leader_quality").is_none());
    }

    #[test]
    fn parses_profiles_from_json_file() {
        let home = tempdir().expect("tempdir");
        let memories_dir = home.path().join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(
            memories_dir.join("team_profile_vouch.json"),
            r#"{
  "profiles": {
    "leader_quality": { "wins": 3, "losses": 1, "note": "稳定" }
  }
}"#,
        )
        .expect("write vouch file");

        let snapshot = load_team_profile_vouch(home.path());
        let entry = snapshot
            .entry_for("leader_quality")
            .expect("profile should exist");
        assert_eq!(entry.wins, 3);
        assert_eq!(entry.losses, 1);
        assert_eq!(entry.note.as_deref(), Some("稳定"));
        assert_eq!(snapshot.load_error(), None);
    }

    #[test]
    fn record_vouch_updates_counts_and_note() {
        let home = tempdir().expect("tempdir");
        let updated = record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Win,
            Some(TeamProfileTaskBucket::General),
            Some("solid run"),
        )
        .expect("record vouch");
        assert_eq!(updated.wins, 1);
        assert_eq!(updated.losses, 0);
        assert_eq!(updated.note.as_deref(), Some("solid run"));
        let general = updated
            .task_entry(TeamProfileTaskBucket::General)
            .expect("general bucket should exist");
        assert_eq!(general.wins, 1);
        assert_eq!(general.losses, 0);
        assert_eq!(general.note.as_deref(), Some("solid run"));

        let updated = record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Loss,
            Some(TeamProfileTaskBucket::Debug),
            None,
        )
        .expect("record second vouch");
        assert_eq!(updated.wins, 1);
        assert_eq!(updated.losses, 1);
        assert_eq!(updated.note.as_deref(), Some("solid run"));
        let debug = updated
            .task_entry(TeamProfileTaskBucket::Debug)
            .expect("debug bucket should exist");
        assert_eq!(debug.wins, 0);
        assert_eq!(debug.losses, 1);
        assert_eq!(debug.note.as_deref(), None);

        let snapshot = load_team_profile_vouch(home.path());
        let entry = snapshot
            .entry_for("leader_quality")
            .expect("profile should exist");
        assert_eq!(entry.wins, 1);
        assert_eq!(entry.losses, 1);
        assert_eq!(entry.note.as_deref(), Some("solid run"));
        let general = entry
            .task_entry(TeamProfileTaskBucket::General)
            .expect("general bucket should exist");
        assert_eq!(general.wins, 1);
        assert_eq!(general.losses, 0);
        let debug = entry
            .task_entry(TeamProfileTaskBucket::Debug)
            .expect("debug bucket should exist");
        assert_eq!(debug.wins, 0);
        assert_eq!(debug.losses, 1);
    }

    #[test]
    fn parses_legacy_file_without_task_breakdown() {
        let home = tempdir().expect("tempdir");
        let memories_dir = home.path().join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(
            memories_dir.join("team_profile_vouch.json"),
            r#"{
  "profiles": {
    "leader_quality": { "wins": 2, "losses": 1, "note": "legacy shape" }
  }
}"#,
        )
        .expect("write vouch file");

        let snapshot = load_team_profile_vouch(home.path());
        let entry = snapshot
            .entry_for("leader_quality")
            .expect("profile should exist");
        assert_eq!(entry.wins, 2);
        assert_eq!(entry.losses, 1);
        assert_eq!(entry.note.as_deref(), Some("legacy shape"));
        assert!(entry.by_task.is_empty());
    }

    #[test]
    fn has_signal_tracks_global_and_task_counts() {
        let home = tempdir().expect("tempdir");
        let snapshot = load_team_profile_vouch(home.path());
        assert_eq!(snapshot.has_signal(), false);

        record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Win,
            Some(TeamProfileTaskBucket::Debug),
            Some("debug win"),
        )
        .expect("record vouch");
        let snapshot = load_team_profile_vouch(home.path());
        assert_eq!(snapshot.has_signal(), true);
    }

    #[test]
    fn recent_signal_uses_weighted_last_events() {
        let home = tempdir().expect("tempdir");
        record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Win,
            Some(TeamProfileTaskBucket::Debug),
            None,
        )
        .expect("record vouch");
        record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Loss,
            Some(TeamProfileTaskBucket::Debug),
            None,
        )
        .expect("record vouch");
        record_team_profile_vouch(
            home.path(),
            "leader_quality",
            TeamProfileVouchVerdict::Loss,
            Some(TeamProfileTaskBucket::Debug),
            None,
        )
        .expect("record vouch");

        let snapshot = load_team_profile_vouch(home.path());
        let entry = snapshot
            .entry_for("leader_quality")
            .expect("profile should exist");
        let recent_debug = entry
            .recent_signal(Some(TeamProfileTaskBucket::Debug))
            .expect("debug recent signal should exist");
        assert_eq!(recent_debug.wins, 1);
        assert_eq!(recent_debug.losses, 2);
        // Events are [win, loss, loss], newest has highest weight (3 + -2 + -1 => -0?).
        // Because newest-first weighting uses [3,2,1], this is -4.
        assert_eq!(recent_debug.weighted_score, -4);
    }

    #[test]
    fn record_vouch_limits_recent_event_log_size() {
        let home = tempdir().expect("tempdir");
        for index in 0..260 {
            let verdict = if index % 2 == 0 {
                TeamProfileVouchVerdict::Win
            } else {
                TeamProfileVouchVerdict::Loss
            };
            record_team_profile_vouch(
                home.path(),
                "leader_quality",
                verdict,
                Some(TeamProfileTaskBucket::General),
                None,
            )
            .expect("record vouch");
        }

        let snapshot = load_team_profile_vouch(home.path());
        let entry = snapshot
            .entry_for("leader_quality")
            .expect("profile should exist");
        assert_eq!(entry.recent_events.len(), 200);
    }
}
