use crate::team_profile_vouch::TeamProfileTaskBucket;
use crate::team_profile_vouch::TeamProfileVouchSnapshot;
use codex_core::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamProfilePreset {
    ClaudeFirst,
    Balanced,
    CostSave,
    DeepReasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeamProfile {
    pub(crate) key: &'static str,
    pub(crate) preset: TeamProfilePreset,
    pub(crate) popup_name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) strengths: &'static [&'static str],
    pub(crate) tradeoffs: &'static [&'static str],
    pub(crate) evidence: &'static str,
    pub(crate) leader_model: &'static str,
    pub(crate) model_sub: &'static str,
    pub(crate) model_sub_responses: &'static str,
    pub(crate) phase_1_model: &'static str,
    pub(crate) phase_2_model: &'static str,
}

// Interim static presets; roadmap is to evolve these from memory-backed vouch data.
pub(crate) const TEAM_PROFILES: [TeamProfile; 4] = [
    TeamProfile {
        key: "leader_quality",
        preset: TeamProfilePreset::ClaudeFirst,
        popup_name: "Leader-Quality",
        label: "Leader-Quality",
        description: "gpt-5.3-codex leads; Claude Sonnet handles default sub-agents; Claude Opus handles deeper consolidation.",
        strengths: &[
            "Best overall leader quality for complex coding threads",
            "Claude Sonnet provides fast and long-context sub-agent coverage",
            "Claude Opus strengthens deep memory consolidation",
        ],
        tradeoffs: &[
            "Higher token cost than cost-save profile",
            "Lower throughput than the fast profile",
        ],
        evidence: "Heuristic profile from recent team practice; vouch scoreboard integration is next.",
        leader_model: "gpt-5.3-codex",
        model_sub: "claude-sonnet-4-6",
        model_sub_responses: "gpt-5.2-codex",
        phase_1_model: "claude-sonnet-4-6",
        phase_2_model: "claude-opus-4-6",
    },
    TeamProfile {
        key: "leader_fast",
        preset: TeamProfilePreset::Balanced,
        popup_name: "Leader-Fast",
        label: "Leader-Fast",
        description: "gpt-5.3-codex-spark|[pro] leads for fast iteration; Claude Sonnet stays the default sub-agent and memory worker.",
        strengths: &[
            "Fastest end-to-end iteration in this profile set",
            "Keeps Claude Sonnet as large-context default sub-agent",
            "Good fit for high-frequency exploration loops",
        ],
        tradeoffs: &[
            "May lose depth versus quality/deep-reasoning profiles",
            "Not the lowest-cost steady-state profile",
        ],
        evidence: "Heuristic profile from observed latency behavior; vouch scoreboard integration is next.",
        leader_model: "gpt-5.3-codex-spark|[pro]",
        model_sub: "claude-sonnet-4-6",
        model_sub_responses: "gpt-5.3-codex-spark|[pro]",
        phase_1_model: "claude-sonnet-4-6",
        phase_2_model: "claude-sonnet-4-6",
    },
    TeamProfile {
        key: "leader_cost_save",
        preset: TeamProfilePreset::CostSave,
        popup_name: "Leader-Cost-Save",
        label: "Leader-Cost-Save",
        description: "gpt-5.2-codex leads for lower cost; Claude Sonnet remains default for broad-context sub-agent tasks.",
        strengths: &[
            "Lowest expected leader cost among codex leader options",
            "Retains Claude Sonnet for broad-context delegation",
            "Stable default for long-running budget-sensitive sessions",
        ],
        tradeoffs: &[
            "Lower peak reasoning depth than quality/deep profiles",
            "Slower convergence on the hardest architecture tasks",
        ],
        evidence: "Heuristic profile from budget-focused usage; vouch scoreboard integration is next.",
        leader_model: "gpt-5.2-codex",
        model_sub: "claude-sonnet-4-6",
        model_sub_responses: "gpt-5.2-codex",
        phase_1_model: "claude-sonnet-4-6",
        phase_2_model: "claude-sonnet-4-6",
    },
    TeamProfile {
        key: "deep_reasoning",
        preset: TeamProfilePreset::DeepReasoning,
        popup_name: "Deep-Reasoning",
        label: "Deep-Reasoning",
        description: "gpt-5.3-codex leads while Claude Opus is prioritized for heavyweight delegation and consolidation.",
        strengths: &[
            "Best depth for hard root-cause and architecture tasks",
            "Prioritizes Claude Opus for difficult sub-agent work",
            "Strongest profile for high-complexity memory consolidation",
        ],
        tradeoffs: &["Highest latency profile", "Highest expected cost profile"],
        evidence: "Heuristic profile from complexity-first workflows; vouch scoreboard integration is next.",
        leader_model: "gpt-5.3-codex",
        model_sub: "claude-opus-4-6",
        model_sub_responses: "gpt-5.3-codex",
        phase_1_model: "claude-sonnet-4-6",
        phase_2_model: "claude-opus-4-6",
    },
];

impl TeamProfile {
    pub(crate) fn matches(
        self,
        leader_model: Option<&str>,
        model_sub: Option<&str>,
        model_sub_responses: Option<&str>,
        phase_1_model: Option<&str>,
        phase_2_model: Option<&str>,
    ) -> bool {
        leader_model == Some(self.leader_model)
            && model_sub == Some(self.model_sub)
            && model_sub_responses == Some(self.model_sub_responses)
            && phase_1_model == Some(self.phase_1_model)
            && phase_2_model == Some(self.phase_2_model)
    }
}

pub(crate) fn profile_for_preset(preset: TeamProfilePreset) -> TeamProfile {
    TEAM_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.preset == preset)
        .unwrap_or_else(|| unreachable!("team profile preset should always exist"))
}

pub(crate) fn profile_for_values(
    leader_model: Option<&str>,
    model_sub: Option<&str>,
    model_sub_responses: Option<&str>,
    phase_1_model: Option<&str>,
    phase_2_model: Option<&str>,
) -> Option<TeamProfile> {
    TEAM_PROFILES.iter().copied().find(|profile| {
        profile.matches(
            leader_model,
            model_sub,
            model_sub_responses,
            phase_1_model,
            phase_2_model,
        )
    })
}

pub(crate) fn profile_for_config(config: &Config) -> Option<TeamProfile> {
    profile_for_values(
        config.model.as_deref(),
        config.model_sub.as_deref(),
        config.model_sub_responses.as_deref(),
        config.memories.phase_1_model.as_deref(),
        config.memories.phase_2_model.as_deref(),
    )
}

fn profile_rank_tuple(
    profile: TeamProfile,
    vouch_snapshot: &TeamProfileVouchSnapshot,
    task_bucket: Option<TeamProfileTaskBucket>,
) -> (i64, u32, u32, i64, u32, u32) {
    let entry = vouch_snapshot.entry_for(profile.key);
    let (global_score, global_wins, global_samples) = entry
        .map(|value| (value.net_score(), value.wins, value.sample_count()))
        .unwrap_or((0, 0, 0));
    let (recent_global_score, recent_global_wins, recent_global_samples) = entry
        .and_then(|value| value.recent_signal(/*task_bucket*/ None))
        .map(|signal| (signal.weighted_score, signal.wins, signal.sample_count()))
        .unwrap_or((global_score, global_wins, global_samples));
    let (task_score, task_wins, task_samples) = match (entry, task_bucket) {
        (Some(value), Some(bucket)) => value
            .recent_signal(Some(bucket))
            .map(|signal| (signal.weighted_score, signal.wins, signal.sample_count()))
            .or_else(|| {
                value.task_entry(bucket).map(|task_entry| {
                    (
                        task_entry.net_score(),
                        task_entry.wins,
                        task_entry.sample_count(),
                    )
                })
            })
            .unwrap_or((0, 0, 0)),
        _ => (0, 0, 0),
    };
    let (fallback_task_score, fallback_task_wins, fallback_task_samples) =
        match (entry, task_bucket) {
            (Some(value), Some(bucket)) => value
                .task_entry(bucket)
                .map(|task_entry| {
                    (
                        task_entry.net_score(),
                        task_entry.wins,
                        task_entry.sample_count(),
                    )
                })
                .unwrap_or((0, 0, 0)),
            _ => (0, 0, 0),
        };
    if task_bucket.is_some() {
        (
            task_score,
            task_wins,
            task_samples,
            recent_global_score,
            recent_global_wins,
            recent_global_samples,
        )
    } else {
        (
            recent_global_score,
            recent_global_wins,
            recent_global_samples,
            fallback_task_score,
            fallback_task_wins,
            fallback_task_samples,
        )
    }
}

pub(crate) fn recommended_profile(
    vouch_snapshot: &TeamProfileVouchSnapshot,
    task_bucket: Option<TeamProfileTaskBucket>,
) -> TeamProfile {
    let mut recommended = TEAM_PROFILES[0];
    for candidate in TEAM_PROFILES.iter().copied().skip(1) {
        if profile_rank_tuple(candidate, vouch_snapshot, task_bucket)
            > profile_rank_tuple(recommended, vouch_snapshot, task_bucket)
        {
            recommended = candidate;
        }
    }
    recommended
}
