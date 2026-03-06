use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tracing::warn;

const MODEL_SUB_VOUCH_REL_PATH: &str = "memories/model_sub_vouch.json";
const MAX_STORED_RECENT_EVENTS: usize = 200;
const RECENT_EVENT_WINDOW: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelSubVouchVerdict {
    Win,
    Loss,
}

impl ModelSubVouchVerdict {
    fn key(self) -> &'static str {
        match self {
            Self::Win => "Win",
            Self::Loss => "Loss",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModelSubVouchStats {
    pub(crate) wins: u32,
    pub(crate) losses: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ModelSubVouchLedger {
    models: BTreeMap<String, ModelSubVouchEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ModelSubVouchEntry {
    wins: u32,
    losses: u32,
    note: Option<String>,
    by_task: BTreeMap<String, ModelSubTaskVouchEntry>,
    recent_events: Vec<ModelSubVouchEvent>,
}

impl ModelSubVouchEntry {
    fn net_score(&self) -> i64 {
        i64::from(self.wins) - i64::from(self.losses)
    }

    fn sample_count(&self) -> u32 {
        self.wins.saturating_add(self.losses)
    }

    fn recent_signal(&self) -> Option<ModelSubRecentSignal> {
        let recent_events: Vec<&ModelSubVouchEvent> = self
            .recent_events
            .iter()
            .rev()
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
            if event.verdict.eq_ignore_ascii_case("win") {
                wins = wins.saturating_add(1);
                weighted_score = weighted_score.saturating_add(weight);
            } else {
                losses = losses.saturating_add(1);
                weighted_score = weighted_score.saturating_sub(weight);
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
struct ModelSubTaskVouchEntry {
    wins: u32,
    losses: u32,
    note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct ModelSubVouchEvent {
    verdict: String,
    task_bucket: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ModelSubRecentSignal {
    weighted_score: i64,
    wins: u32,
    losses: u32,
}

impl ModelSubRecentSignal {
    fn sample_count(self) -> u32 {
        self.wins.saturating_add(self.losses)
    }
}

pub(crate) fn ranked_model_sub_candidates(codex_home: &Path) -> Vec<String> {
    let path = codex_home.join(MODEL_SUB_VOUCH_REL_PATH);
    let raw = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            warn!("failed to read {}: {err}", path.display());
            return Vec::new();
        }
    };
    let ledger = match serde_json::from_str::<ModelSubVouchLedger>(&raw) {
        Ok(ledger) => ledger,
        Err(err) => {
            warn!("failed to parse {}: {err}", path.display());
            return Vec::new();
        }
    };

    let mut ranked = ledger
        .models
        .into_iter()
        .map(|(model, entry)| {
            let (score, wins, samples) = entry
                .recent_signal()
                .map(|recent| (recent.weighted_score, recent.wins, recent.sample_count()))
                .unwrap_or((entry.net_score(), entry.wins, entry.sample_count()));
            (model, score, wins, samples)
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| right.3.cmp(&left.3))
            .then_with(|| left.0.cmp(&right.0))
    });

    if ranked
        .first()
        .is_some_and(|(_model, score, wins, samples)| *score == 0 && *wins == 0 && *samples == 0)
    {
        return Vec::new();
    }

    ranked
        .into_iter()
        .map(|(model, _score, _wins, _samples)| model)
        .collect()
}

pub(crate) fn record_model_sub_vouch(
    codex_home: &Path,
    model: &str,
    verdict: ModelSubVouchVerdict,
    task_bucket: Option<&str>,
    note: Option<&str>,
) -> Result<ModelSubVouchStats, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model slug cannot be empty".to_string());
    }

    let task_bucket = task_bucket
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let note = note.map(str::trim).filter(|value| !value.is_empty());

    let path = codex_home.join(MODEL_SUB_VOUCH_REL_PATH);
    let mut ledger = match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<ModelSubVouchLedger>(&content)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ModelSubVouchLedger::default(),
        Err(err) => {
            return Err(format!("failed to read {}: {err}", path.display()));
        }
    };

    let updated = {
        let entry = ledger.models.entry(model.to_string()).or_default();
        match verdict {
            ModelSubVouchVerdict::Win => entry.wins = entry.wins.saturating_add(1),
            ModelSubVouchVerdict::Loss => entry.losses = entry.losses.saturating_add(1),
        }

        if let Some(task_bucket) = task_bucket.as_deref() {
            let task_entry = entry.by_task.entry(task_bucket.to_string()).or_default();
            match verdict {
                ModelSubVouchVerdict::Win => task_entry.wins = task_entry.wins.saturating_add(1),
                ModelSubVouchVerdict::Loss => {
                    task_entry.losses = task_entry.losses.saturating_add(1);
                }
            }
            if let Some(note) = note {
                task_entry.note = Some(note.to_string());
            }
        }

        entry.recent_events.push(ModelSubVouchEvent {
            verdict: verdict.key().to_string(),
            task_bucket,
        });
        if entry.recent_events.len() > MAX_STORED_RECENT_EVENTS {
            let excess = entry
                .recent_events
                .len()
                .saturating_sub(MAX_STORED_RECENT_EVENTS);
            entry.recent_events.drain(..excess);
        }
        if let Some(note) = note {
            entry.note = Some(note.to_string());
        }

        ModelSubVouchStats {
            wins: entry.wins,
            losses: entry.losses,
        }
    };

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

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::ModelSubVouchVerdict;
    use super::ranked_model_sub_candidates;
    use super::record_model_sub_vouch;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn returns_empty_when_file_missing() {
        let home = tempdir().expect("tempdir");
        let ranked = ranked_model_sub_candidates(home.path());
        assert_eq!(ranked, Vec::<String>::new());
    }

    #[test]
    fn ranks_models_by_recent_weighted_signal() {
        let home = tempdir().expect("tempdir");
        let memories_dir = home.path().join("memories");
        fs::create_dir_all(&memories_dir).expect("create memories dir");
        fs::write(
            memories_dir.join("model_sub_vouch.json"),
            r#"{
  "models": {
    "claude-sonnet-4-6": {
      "wins": 8,
      "losses": 0,
      "recent_events": [{ "verdict": "Loss" }, { "verdict": "Loss" }]
    },
    "gpt-5.2-codex": {
      "wins": 1,
      "losses": 1,
      "recent_events": [{ "verdict": "Win" }, { "verdict": "Win" }]
    }
  }
}"#,
        )
        .expect("write vouch file");

        let ranked = ranked_model_sub_candidates(home.path());
        assert_eq!(
            ranked,
            vec!["gpt-5.2-codex".to_string(), "claude-sonnet-4-6".to_string()]
        );
    }

    #[test]
    fn record_updates_global_and_task_scores() {
        let home = tempdir().expect("tempdir");

        let first = record_model_sub_vouch(
            home.path(),
            "claude-sonnet-4-6",
            ModelSubVouchVerdict::Win,
            Some("debug"),
            Some("good fix quality"),
        )
        .expect("record should succeed");
        assert_eq!(first.wins, 1);
        assert_eq!(first.losses, 0);

        let second = record_model_sub_vouch(
            home.path(),
            "claude-sonnet-4-6",
            ModelSubVouchVerdict::Loss,
            Some("debug"),
            None,
        )
        .expect("record should succeed");
        assert_eq!(second.wins, 1);
        assert_eq!(second.losses, 1);

        let raw = fs::read_to_string(home.path().join("memories/model_sub_vouch.json"))
            .expect("vouch file should exist");
        let ledger: serde_json::Value =
            serde_json::from_str(&raw).expect("ledger should parse as json");
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["wins"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["losses"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["by_task"]["debug"]["wins"],
            serde_json::Value::from(1)
        );
        assert_eq!(
            ledger["models"]["claude-sonnet-4-6"]["by_task"]["debug"]["losses"],
            serde_json::Value::from(1)
        );
    }

    #[test]
    fn record_rejects_empty_model_slug() {
        let home = tempdir().expect("tempdir");
        let err = record_model_sub_vouch(home.path(), "   ", ModelSubVouchVerdict::Win, None, None)
            .expect_err("empty model slug should fail");
        assert_eq!(err, "model slug cannot be empty");
    }
}
