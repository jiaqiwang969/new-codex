use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::with_border_with_inner_width;
use crate::version::CODEX_CLI_VERSION;
use chrono::DateTime;
use chrono::Local;
use codex_core::ModelProviderInfo;
use codex_core::WireApi;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::account::PlanType;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::NetworkAccess;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TokenUsageInfo;
use codex_utils_sandbox_summary::summarize_sandbox_policy;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use url::Url;

use super::account::StatusAccountDisplay;
use super::format::FieldFormatter;
use super::format::line_display_width;
use super::format::push_label;
use super::format::truncate_line_to_width;
use super::helpers::compose_account_display;
use super::helpers::compose_agents_summary;
use super::helpers::compose_model_display;
use super::helpers::format_directory_display;
use super::helpers::format_tokens_compact;
use super::rate_limits::RateLimitSnapshotDisplay;
use super::rate_limits::StatusRateLimitData;
use super::rate_limits::StatusRateLimitRow;
use super::rate_limits::StatusRateLimitValue;
use super::rate_limits::compose_rate_limit_data;
use super::rate_limits::compose_rate_limit_data_many;
use super::rate_limits::format_status_limit_summary;
use super::rate_limits::render_status_limit_progress_bar;
use crate::model_sub_vouch;
use crate::team_profile;
use crate::team_profile_vouch;
use crate::wrapping::RtOptions;
use crate::wrapping::adaptive_wrap_lines;
use codex_core::AuthManager;

#[derive(Debug, Clone)]
struct StatusContextWindowData {
    percent_remaining: i64,
    tokens_in_context: i64,
    window: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusTokenUsageData {
    total: i64,
    input: i64,
    output: i64,
    context_window: Option<StatusContextWindowData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusUtilityRoutingHint {
    pub(crate) model: String,
    pub(crate) source_label: String,
}

#[derive(Debug, Clone)]
struct StatusMemoryModel {
    model: String,
    source_label: String,
}

#[derive(Debug)]
struct StatusHistoryCell {
    model_name: String,
    model_details: Vec<String>,
    team_profile_label: Option<&'static str>,
    team_profile_vouch: Option<String>,
    team_profile_auto: Option<String>,
    utility_model_name: String,
    utility_model_configured: bool,
    utility_model_auto_selected: bool,
    utility_model_source: String,
    utility_model_provider: Option<String>,
    show_utility_model_responses: bool,
    utility_model_responses_name: String,
    utility_model_responses_configured: bool,
    utility_model_responses_provider: Option<String>,
    memory_scope: String,
    entire_tracing: String,
    memory_phase_one: StatusMemoryModel,
    memory_phase_two: StatusMemoryModel,
    entire_summary_model: StatusMemoryModel,
    directory: PathBuf,
    permissions: String,
    agents_summary: String,
    collaboration_mode: Option<String>,
    model_provider: Option<String>,
    account: Option<StatusAccountDisplay>,
    thread_name: Option<String>,
    session_id: Option<String>,
    forked_from: Option<String>,
    token_usage: StatusTokenUsageData,
    rate_limits: StatusRateLimitData,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output(
    config: &Config,
    auth_manager: &AuthManager,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: Option<&RateLimitSnapshotDisplay>,
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
) -> CompositeHistoryCell {
    let snapshots = rate_limits.map(std::slice::from_ref).unwrap_or_default();
    new_status_output_with_rate_limits_and_utility_routing(
        config,
        auth_manager,
        token_info,
        total_usage,
        session_id,
        thread_name,
        forked_from,
        snapshots,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_status_output_with_rate_limits_and_utility_routing(
    config: &Config,
    auth_manager: &AuthManager,
    token_info: Option<&TokenUsageInfo>,
    total_usage: &TokenUsage,
    session_id: &Option<ThreadId>,
    thread_name: Option<String>,
    forked_from: Option<ThreadId>,
    rate_limits: &[RateLimitSnapshotDisplay],
    plan_type: Option<PlanType>,
    now: DateTime<Local>,
    model_name: &str,
    collaboration_mode: Option<&str>,
    reasoning_effort_override: Option<Option<ReasoningEffort>>,
    utility_routing_hint: Option<StatusUtilityRoutingHint>,
) -> CompositeHistoryCell {
    let command = PlainHistoryCell::new(vec!["/status".magenta().into()]);
    let card = StatusHistoryCell::new(
        config,
        auth_manager,
        token_info,
        total_usage,
        session_id,
        thread_name,
        forked_from,
        rate_limits,
        plan_type,
        now,
        model_name,
        collaboration_mode,
        reasoning_effort_override,
        utility_routing_hint,
    );

    CompositeHistoryCell::new(vec![Box::new(command), Box::new(card)])
}

impl StatusHistoryCell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: &Config,
        auth_manager: &AuthManager,
        token_info: Option<&TokenUsageInfo>,
        total_usage: &TokenUsage,
        session_id: &Option<ThreadId>,
        thread_name: Option<String>,
        forked_from: Option<ThreadId>,
        rate_limits: &[RateLimitSnapshotDisplay],
        plan_type: Option<PlanType>,
        now: DateTime<Local>,
        model_name: &str,
        collaboration_mode: Option<&str>,
        reasoning_effort_override: Option<Option<ReasoningEffort>>,
        utility_routing_hint: Option<StatusUtilityRoutingHint>,
    ) -> Self {
        let (active_model_provider_id, active_model_provider) =
            codex_core::utility_provider_for_model_slug(config, model_name).unwrap_or_else(|| {
                (
                    config.model_provider_id.clone(),
                    config.model_provider.clone(),
                )
            });
        let active_model_wire_api = active_model_provider.wire_api;
        let mut config_entries = vec![
            ("workdir", config.cwd.display().to_string()),
            ("model", model_name.to_string()),
            ("provider", active_model_provider_id.clone()),
            (
                "approval",
                config.permissions.approval_policy.value().to_string(),
            ),
            (
                "sandbox",
                summarize_sandbox_policy(config.permissions.sandbox_policy.get()),
            ),
        ];
        if active_model_wire_api == WireApi::Responses {
            let effort_value = reasoning_effort_override
                .unwrap_or(None)
                .map(|effort| effort.to_string())
                .unwrap_or_else(|| "none".to_string());
            config_entries.push(("reasoning effort", effort_value));
            config_entries.push((
                "reasoning summaries",
                config
                    .model_reasoning_summary
                    .map(|summary| summary.to_string())
                    .unwrap_or_else(|| "auto".to_string()),
            ));
        }
        let (model_name, model_details) = compose_model_display(model_name, &config_entries);
        let approval = config_entries
            .iter()
            .find(|(k, _)| *k == "approval")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        let sandbox = match config.permissions.sandbox_policy.get() {
            SandboxPolicy::DangerFullAccess => "danger-full-access".to_string(),
            SandboxPolicy::ReadOnly { .. } => "read-only".to_string(),
            SandboxPolicy::WorkspaceWrite {
                network_access: true,
                ..
            } => "workspace-write with network access".to_string(),
            SandboxPolicy::WorkspaceWrite { .. } => "workspace-write".to_string(),
            SandboxPolicy::ExternalSandbox { network_access } => {
                if matches!(network_access, NetworkAccess::Enabled) {
                    "external-sandbox (network access enabled)".to_string()
                } else {
                    "external-sandbox".to_string()
                }
            }
        };
        let permissions = if config.permissions.approval_policy.value() == AskForApproval::OnRequest
            && *config.permissions.sandbox_policy.get()
                == SandboxPolicy::new_workspace_write_policy()
        {
            if config.approvals_reviewer == ApprovalsReviewer::GuardianSubagent {
                "Smart Approvals".to_string()
            } else {
                "Default".to_string()
            }
        } else if config.permissions.approval_policy.value() == AskForApproval::Never
            && *config.permissions.sandbox_policy.get() == SandboxPolicy::DangerFullAccess
        {
            "Full Access".to_string()
        } else {
            format!("Custom ({sandbox}, {approval})")
        };
        let agents_summary = compose_agents_summary(config);
        let model_provider =
            format_model_provider(&active_model_provider_id, &active_model_provider);
        let account = compose_account_display(auth_manager, plan_type);
        let session_id = session_id.as_ref().map(std::string::ToString::to_string);
        let forked_from = forked_from.map(|id| id.to_string());
        let default_usage = TokenUsage::default();
        let (context_usage, context_window) = match token_info {
            Some(info) => (&info.last_token_usage, info.model_context_window),
            None => (&default_usage, config.model_context_window),
        };
        let context_window = context_window.map(|window| StatusContextWindowData {
            percent_remaining: context_usage.percent_of_context_window_remaining(window),
            tokens_in_context: context_usage.tokens_in_context_window(),
            window,
        });

        let token_usage = StatusTokenUsageData {
            total: total_usage.blended_total(),
            input: total_usage.non_cached_input(),
            output: total_usage.output_tokens,
            context_window,
        };
        let rate_limits = if rate_limits.len() <= 1 {
            compose_rate_limit_data(rate_limits.first(), now)
        } else {
            compose_rate_limit_data_many(rate_limits, now)
        };

        let utility_model_vouch = if config.model_sub.is_none() && utility_routing_hint.is_none() {
            let vouch_snapshot = model_sub_vouch::load_model_sub_vouch(&config.codex_home);
            model_sub_vouch::recommended_model_sub_from_snapshot(
                &vouch_snapshot,
                /*task_bucket*/ None,
            )
        } else {
            None
        };
        let utility_model_runtime = config
            .model_sub
            .clone()
            .or_else(|| utility_routing_hint.as_ref().map(|hint| hint.model.clone()))
            .or_else(|| utility_model_vouch.clone());
        let utility_model_name = utility_model_runtime
            .clone()
            .unwrap_or_else(|| "task defaults".to_string());
        let active_team_profile = team_profile::profile_for_config(config);
        let team_profile_label = active_team_profile.map(|profile| profile.label);
        let vouch_snapshot = team_profile_vouch::load_team_profile_vouch(&config.codex_home);
        let team_profile_vouch = active_team_profile.and_then(|profile| {
            let entry = vouch_snapshot.entry_for(profile.key)?;
            let mut summary = format!(
                "global +{} / -{} (net {:+})",
                entry.wins,
                entry.losses,
                entry.net_score()
            );
            if let Some(recent) = entry.recent_signal(/*task_bucket*/ None)
                && recent.sample_count() > 0
            {
                summary.push_str(" | recent +");
                summary.push_str(&recent.wins.to_string());
                summary.push_str(" / -");
                summary.push_str(&recent.losses.to_string());
                summary.push_str(" (weighted ");
                summary.push_str(&format!("{:+}", recent.weighted_score));
                summary.push(')');
            }
            for task_bucket in team_profile_vouch::TeamProfileTaskBucket::ALL {
                if let Some(task_entry) = entry.task_entry(task_bucket)
                    && task_entry.sample_count() > 0
                {
                    summary.push_str(" | ");
                    summary.push_str(task_bucket.label());
                    summary.push_str(" +");
                    summary.push_str(&task_entry.wins.to_string());
                    summary.push_str(" / -");
                    summary.push_str(&task_entry.losses.to_string());
                    summary.push_str(" (net ");
                    summary.push_str(&format!("{:+}", task_entry.net_score()));
                    summary.push(')');
                    if let Some(recent) = entry.recent_signal(Some(task_bucket))
                        && recent.sample_count() > 0
                    {
                        summary.push_str(" [recent +");
                        summary.push_str(&recent.wins.to_string());
                        summary.push_str(" / -");
                        summary.push_str(&recent.losses.to_string());
                        summary.push_str(", weighted ");
                        summary.push_str(&format!("{:+}", recent.weighted_score));
                        summary.push(']');
                    }
                }
            }
            if let Some(note) = entry.note.as_deref()
                && !note.is_empty()
            {
                summary.push_str(" | note: ");
                summary.push_str(note);
            }
            Some(summary)
        });
        let team_profile_auto = if active_team_profile.is_some() && vouch_snapshot.has_signal() {
            let general =
                team_profile::recommended_profile(&vouch_snapshot, /*task_bucket*/ None).label;
            let debug = team_profile::recommended_profile(
                &vouch_snapshot,
                Some(team_profile_vouch::TeamProfileTaskBucket::Debug),
            )
            .label;
            let review = team_profile::recommended_profile(
                &vouch_snapshot,
                Some(team_profile_vouch::TeamProfileTaskBucket::Review),
            )
            .label;
            Some(if general == debug && debug == review {
                format!("all -> {general}")
            } else {
                format!("general -> {general}; debug -> {debug}; review -> {review}")
            })
        } else {
            None
        };
        let utility_model_configured = config.model_sub.is_some();
        let utility_model_auto_selected = !utility_model_configured
            && (utility_routing_hint.is_some() || utility_model_vouch.is_some());
        let utility_model_source = if utility_model_configured {
            "config.model_sub".to_string()
        } else if let Some(hint) = utility_routing_hint.as_ref() {
            format!("auto ({})", hint.source_label)
        } else if utility_model_vouch.is_some() {
            "auto (model_sub_vouch)".to_string()
        } else {
            "task defaults (parent/role)".to_string()
        };
        let utility_model_provider = utility_model_runtime
            .as_deref()
            .and_then(|model| format_utility_model_provider(config, model));
        let utility_model_responses_name =
            codex_core::effective_responses_utility_model_slug(config).to_string();
        let utility_model_responses_configured = config
            .model_sub_responses
            .as_deref()
            .is_some_and(codex_core::is_openai_model_slug);
        let utility_model_responses_provider =
            format_utility_model_provider(config, &utility_model_responses_name);
        let show_utility_model_responses = utility_model_responses_configured
            || (!utility_model_configured && active_model_wire_api != WireApi::Responses)
            || utility_model_runtime.is_some_and(|_| {
                utility_model_responses_name != utility_model_name
                    || utility_model_responses_provider != utility_model_provider
            });
        let memory_scope = "auto (cwd -> user -> global)".to_string();
        let entire_tracing = describe_entire_tracing(config);
        let memory_phase_one = resolve_memory_model_display(
            config.memories.phase_1_model.as_deref(),
            config.model_sub.as_deref(),
            codex_core::DEFAULT_MEMORY_PHASE_ONE_MODEL,
            "memories.phase_1_model",
        );
        let memory_phase_two = resolve_memory_model_display(
            config.memories.phase_2_model.as_deref(),
            /*model_sub*/ None,
            codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL,
            "memories.phase_2_model",
        );
        let entire_summary_model = resolve_memory_model_display(
            config.memories.entire_summary_model.as_deref(),
            /*model_sub*/ None,
            codex_core::DEFAULT_ENTIRE_SUMMARY_MODEL,
            "memories.entire_summary_model",
        );

        Self {
            model_name,
            model_details,
            team_profile_label,
            team_profile_vouch,
            team_profile_auto,
            utility_model_name,
            utility_model_configured,
            utility_model_auto_selected,
            utility_model_source,
            utility_model_provider,
            show_utility_model_responses,
            utility_model_responses_name,
            utility_model_responses_configured,
            utility_model_responses_provider,
            memory_scope,
            entire_tracing,
            memory_phase_one,
            memory_phase_two,
            entire_summary_model,
            directory: config.cwd.to_path_buf(),
            permissions,
            agents_summary,
            collaboration_mode: collaboration_mode.map(ToString::to_string),
            model_provider,
            account,
            thread_name,
            session_id,
            forked_from,
            token_usage,
            rate_limits,
        }
    }

    fn token_usage_spans(&self) -> Vec<Span<'static>> {
        let total_fmt = format_tokens_compact(self.token_usage.total);
        let input_fmt = format_tokens_compact(self.token_usage.input);
        let output_fmt = format_tokens_compact(self.token_usage.output);

        vec![
            Span::from(total_fmt),
            Span::from(" total "),
            Span::from(" (").dim(),
            Span::from(input_fmt).dim(),
            Span::from(" input").dim(),
            Span::from(" + ").dim(),
            Span::from(output_fmt).dim(),
            Span::from(" output").dim(),
            Span::from(")").dim(),
        ]
    }

    fn context_window_spans(&self) -> Option<Vec<Span<'static>>> {
        let context = self.token_usage.context_window.as_ref()?;
        let percent = context.percent_remaining;
        let used_fmt = format_tokens_compact(context.tokens_in_context);
        let window_fmt = format_tokens_compact(context.window);

        Some(vec![
            Span::from(format!("{percent}% left")),
            Span::from(" (").dim(),
            Span::from(used_fmt).dim(),
            Span::from(" used / ").dim(),
            Span::from(window_fmt).dim(),
            Span::from(")").dim(),
        ])
    }

    fn rate_limit_lines(
        &self,
        available_inner_width: usize,
        formatter: &FieldFormatter,
    ) -> Vec<Line<'static>> {
        match &self.rate_limits {
            StatusRateLimitData::Available(rows_data) => {
                if rows_data.is_empty() {
                    return vec![
                        formatter.line("Limits", vec![Span::from("data not available yet").dim()]),
                    ];
                }

                self.rate_limit_row_lines(rows_data, available_inner_width, formatter)
            }
            StatusRateLimitData::Stale(rows_data) => {
                let mut lines =
                    self.rate_limit_row_lines(rows_data, available_inner_width, formatter);
                lines.push(formatter.line(
                    "Warning",
                    vec![Span::from("limits may be stale - start new turn to refresh.").dim()],
                ));
                lines
            }
            StatusRateLimitData::Missing => {
                vec![formatter.line("Limits", vec![Span::from("data not available yet").dim()])]
            }
        }
    }

    fn rate_limit_row_lines(
        &self,
        rows: &[StatusRateLimitRow],
        available_inner_width: usize,
        formatter: &FieldFormatter,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::with_capacity(rows.len().saturating_mul(2));

        for row in rows {
            match &row.value {
                StatusRateLimitValue::Window {
                    percent_used,
                    resets_at,
                } => {
                    let percent_remaining = (100.0 - percent_used).clamp(0.0, 100.0);
                    let value_spans = vec![
                        Span::from(render_status_limit_progress_bar(percent_remaining)),
                        Span::from(" "),
                        Span::from(format_status_limit_summary(percent_remaining)),
                    ];
                    let base_spans = formatter.full_spans(row.label.as_str(), value_spans);
                    let base_line = Line::from(base_spans.clone());

                    if let Some(resets_at) = resets_at.as_ref() {
                        let resets_span = Span::from(format!("(resets {resets_at})")).dim();
                        let mut inline_spans = base_spans.clone();
                        inline_spans.push(Span::from(" ").dim());
                        inline_spans.push(resets_span.clone());

                        if line_display_width(&Line::from(inline_spans.clone()))
                            <= available_inner_width
                        {
                            lines.push(Line::from(inline_spans));
                        } else {
                            lines.push(base_line);
                            lines.push(formatter.continuation(vec![resets_span]));
                        }
                    } else {
                        lines.push(base_line);
                    }
                }
                StatusRateLimitValue::Text(text) => {
                    let label = row.label.clone();
                    let spans =
                        formatter.full_spans(label.as_str(), vec![Span::from(text.clone())]);
                    lines.push(Line::from(spans));
                }
            }
        }

        lines
    }

    fn collect_rate_limit_labels(&self, seen: &mut BTreeSet<String>, labels: &mut Vec<String>) {
        match &self.rate_limits {
            StatusRateLimitData::Available(rows) => {
                if rows.is_empty() {
                    push_label(labels, seen, "Limits");
                } else {
                    for row in rows {
                        push_label(labels, seen, row.label.as_str());
                    }
                }
            }
            StatusRateLimitData::Stale(rows) => {
                for row in rows {
                    push_label(labels, seen, row.label.as_str());
                }
                push_label(labels, seen, "Warning");
            }
            StatusRateLimitData::Missing => push_label(labels, seen, "Limits"),
        }
    }
}

impl HistoryCell for StatusHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(vec![
            Span::from(format!("{}>_ ", FieldFormatter::INDENT)).dim(),
            Span::from("OpenAI Codex").bold(),
            Span::from(" ").dim(),
            Span::from(format!("(v{CODEX_CLI_VERSION})")).dim(),
        ]));
        lines.push(Line::from(Vec::<Span<'static>>::new()));

        let available_inner_width = usize::from(width.saturating_sub(4));
        if available_inner_width == 0 {
            return Vec::new();
        }

        let account_value = self.account.as_ref().map(|account| match account {
            StatusAccountDisplay::ChatGpt { email, plan } => match (email, plan) {
                (Some(email), Some(plan)) => format!("{email} ({plan})"),
                (Some(email), None) => email.clone(),
                (None, Some(plan)) => plan.clone(),
                (None, None) => "ChatGPT".to_string(),
            },
            StatusAccountDisplay::ApiKey => {
                "API key configured (run codex login to use ChatGPT)".to_string()
            }
        });

        let mut labels: Vec<String> = vec!["Model", "Directory", "Permissions", "Agents.md"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut seen: BTreeSet<String> = labels.iter().cloned().collect();
        let thread_name = self.thread_name.as_deref().filter(|name| !name.is_empty());

        if self.team_profile_label.is_some() {
            push_label(&mut labels, &mut seen, "Team profile");
        }
        if self.team_profile_vouch.is_some() {
            push_label(&mut labels, &mut seen, "Team profile vouch");
        }
        if self.team_profile_auto.is_some() {
            push_label(&mut labels, &mut seen, "Team profile auto");
        }
        if self.model_provider.is_some() {
            push_label(&mut labels, &mut seen, "Model provider");
        }
        push_label(&mut labels, &mut seen, "Utility/sub-agent");
        push_label(&mut labels, &mut seen, "Utility source");
        if self.utility_model_provider.is_some() {
            push_label(&mut labels, &mut seen, "Utility provider");
        }
        if self.show_utility_model_responses {
            push_label(&mut labels, &mut seen, "Resp. util model");
            if self.utility_model_responses_provider.is_some() {
                push_label(&mut labels, &mut seen, "Resp. util prov");
            }
        }
        push_label(&mut labels, &mut seen, "Memory scope");
        push_label(&mut labels, &mut seen, "Entire tracing");
        push_label(&mut labels, &mut seen, "Memory phase-1");
        push_label(&mut labels, &mut seen, "Memory phase-2");
        if account_value.is_some() {
            push_label(&mut labels, &mut seen, "Account");
        }
        if thread_name.is_some() {
            push_label(&mut labels, &mut seen, "Thread name");
        }
        if self.session_id.is_some() {
            push_label(&mut labels, &mut seen, "Session");
        }
        if self.session_id.is_some() && self.forked_from.is_some() {
            push_label(&mut labels, &mut seen, "Forked from");
        }
        if self.collaboration_mode.is_some() {
            push_label(&mut labels, &mut seen, "Collaboration mode");
        }
        push_label(&mut labels, &mut seen, "Token usage");
        if self.token_usage.context_window.is_some() {
            push_label(&mut labels, &mut seen, "Context window");
        }

        self.collect_rate_limit_labels(&mut seen, &mut labels);

        let formatter = FieldFormatter::from_labels(labels.iter().map(String::as_str));
        let value_width = formatter.value_width(available_inner_width);

        let note_first_line = Line::from(vec![
            Span::from("Visit ").cyan(),
            "https://chatgpt.com/codex/settings/usage"
                .cyan()
                .underlined(),
            Span::from(" for up-to-date").cyan(),
        ]);
        let note_second_line = Line::from(vec![
            Span::from("information on rate limits and credits").cyan(),
        ]);
        let note_lines = adaptive_wrap_lines(
            [note_first_line, note_second_line],
            RtOptions::new(available_inner_width),
        );
        lines.extend(note_lines);
        lines.push(Line::from(Vec::<Span<'static>>::new()));

        let mut model_spans = vec![Span::from(self.model_name.clone())];
        if !self.model_details.is_empty() {
            model_spans.push(Span::from(" (").dim());
            model_spans.push(Span::from(self.model_details.join(", ")).dim());
            model_spans.push(Span::from(")").dim());
        }

        let directory_value = format_directory_display(&self.directory, Some(value_width));

        lines.push(formatter.line("Model", model_spans));
        if let Some(model_provider) = self.model_provider.as_ref() {
            lines.push(formatter.line("Model provider", vec![Span::from(model_provider.clone())]));
        }
        if let Some(team_profile_label) = self.team_profile_label {
            lines.push(formatter.line("Team profile", vec![Span::from(team_profile_label)]));
        }
        if let Some(team_profile_vouch) = self.team_profile_vouch.as_ref() {
            lines.push(formatter.line(
                "Team profile vouch",
                vec![Span::from(team_profile_vouch.clone())],
            ));
        }
        if let Some(team_profile_auto) = self.team_profile_auto.as_ref() {
            lines.push(formatter.line(
                "Team profile auto",
                vec![Span::from(team_profile_auto.clone())],
            ));
        }
        let mut utility_model_spans = vec![Span::from(self.utility_model_name.clone())];
        if !self.utility_model_configured {
            if self.utility_model_auto_selected {
                utility_model_spans.push(Span::from(" (auto)").dim());
            } else {
                utility_model_spans.push(Span::from(" (inherit)").dim());
            }
        }
        lines.push(formatter.line("Utility/sub-agent", utility_model_spans));
        lines.push(formatter.line(
            "Utility source",
            vec![Span::from(self.utility_model_source.clone())],
        ));
        if let Some(utility_model_provider) = self.utility_model_provider.as_ref() {
            lines.push(formatter.line(
                "Utility provider",
                vec![Span::from(utility_model_provider.clone())],
            ));
        }
        if self.show_utility_model_responses {
            let mut utility_model_responses_spans =
                vec![Span::from(self.utility_model_responses_name.clone())];
            if !self.utility_model_responses_configured {
                utility_model_responses_spans.push(Span::from(" (inherit)").dim());
            }
            lines.push(formatter.line("Resp. util model", utility_model_responses_spans));
            if let Some(utility_model_responses_provider) =
                self.utility_model_responses_provider.as_ref()
            {
                lines.push(formatter.line(
                    "Resp. util prov",
                    vec![Span::from(utility_model_responses_provider.clone())],
                ));
            }
        }
        lines.push(formatter.line("Memory scope", vec![Span::from(self.memory_scope.clone())]));
        lines.push(formatter.line(
            "Entire tracing",
            vec![Span::from(self.entire_tracing.clone())],
        ));
        let mut memory_phase_one_spans = vec![Span::from(self.memory_phase_one.model.clone())];
        memory_phase_one_spans.push(Span::from(" (").dim());
        memory_phase_one_spans.push(Span::from(self.memory_phase_one.source_label.clone()).dim());
        memory_phase_one_spans.push(Span::from(")").dim());
        lines.push(formatter.line("Memory phase-1", memory_phase_one_spans));
        let mut memory_phase_two_spans = vec![Span::from(self.memory_phase_two.model.clone())];
        memory_phase_two_spans.push(Span::from(" (").dim());
        memory_phase_two_spans.push(Span::from(self.memory_phase_two.source_label.clone()).dim());
        memory_phase_two_spans.push(Span::from(")").dim());
        lines.push(formatter.line("Memory phase-2", memory_phase_two_spans));
        let mut entire_summary_spans = vec![Span::from(self.entire_summary_model.model.clone())];
        entire_summary_spans.push(Span::from(" (").dim());
        entire_summary_spans.push(Span::from(self.entire_summary_model.source_label.clone()).dim());
        entire_summary_spans.push(Span::from(")").dim());
        lines.push(formatter.line("Entire summary", entire_summary_spans));
        lines.push(formatter.line("Directory", vec![Span::from(directory_value)]));
        lines.push(formatter.line("Permissions", vec![Span::from(self.permissions.clone())]));
        lines.push(formatter.line("Agents.md", vec![Span::from(self.agents_summary.clone())]));

        if let Some(account_value) = account_value {
            lines.push(formatter.line("Account", vec![Span::from(account_value)]));
        }

        if let Some(thread_name) = thread_name {
            lines.push(formatter.line("Thread name", vec![Span::from(thread_name.to_string())]));
        }
        if let Some(collab_mode) = self.collaboration_mode.as_ref() {
            lines.push(formatter.line("Collaboration mode", vec![Span::from(collab_mode.clone())]));
        }
        if let Some(session) = self.session_id.as_ref() {
            lines.push(formatter.line("Session", vec![Span::from(session.clone())]));
        }
        if self.session_id.is_some()
            && let Some(forked_from) = self.forked_from.as_ref()
        {
            lines.push(formatter.line("Forked from", vec![Span::from(forked_from.clone())]));
        }

        lines.push(Line::from(Vec::<Span<'static>>::new()));
        // Hide token usage only for ChatGPT subscribers
        if !matches!(self.account, Some(StatusAccountDisplay::ChatGpt { .. })) {
            lines.push(formatter.line("Token usage", self.token_usage_spans()));
        }

        if let Some(spans) = self.context_window_spans() {
            lines.push(formatter.line("Context window", spans));
        }

        lines.extend(self.rate_limit_lines(available_inner_width, &formatter));

        let content_width = lines.iter().map(line_display_width).max().unwrap_or(0);
        let inner_width = content_width.min(available_inner_width);
        let truncated_lines: Vec<Line<'static>> = lines
            .into_iter()
            .map(|line| truncate_line_to_width(line, inner_width))
            .collect();

        with_border_with_inner_width(truncated_lines, inner_width)
    }
}

fn format_model_provider(provider_id: &str, provider: &ModelProviderInfo) -> Option<String> {
    let name = provider.name.trim();
    let provider_name = if name.is_empty() { provider_id } else { name };
    let base_url = provider.base_url.as_deref().and_then(sanitize_base_url);
    let is_default_openai = provider.is_openai() && base_url.is_none();
    if is_default_openai {
        return None;
    }

    Some(match base_url {
        Some(base_url) => format!("{provider_name} - {base_url}"),
        None => provider_name.to_string(),
    })
}

fn format_utility_model_provider(config: &Config, model_slug: &str) -> Option<String> {
    let (provider_id, provider) = codex_core::utility_provider_for_model_slug(config, model_slug)?;

    let name = provider.name.trim();
    let provider_name = if name.is_empty() {
        provider_id.as_str()
    } else {
        name
    };
    let base_url = provider.base_url.as_deref().and_then(sanitize_base_url);
    let is_default_openai = provider.is_openai() && base_url.is_none();
    if is_default_openai {
        return None;
    }

    Some(match base_url {
        Some(base_url) => format!("{provider_name} - {base_url}"),
        None => provider_name.to_string(),
    })
}

fn describe_entire_tracing(config: &Config) -> String {
    let Some(notify_cmd) = config.notify.as_ref() else {
        return "not configured (set notify=[\"entire\", \"hooks\", \"codex\", \"notify\"])"
            .to_string();
    };

    if notify_cmd
        .iter()
        .any(|part| part.eq_ignore_ascii_case("entire"))
    {
        "notify hook -> git checkpoints (entire/*, Entire-Checkpoint trailers)".to_string()
    } else {
        "notify hook configured (non-entire command)".to_string()
    }
}

fn resolve_memory_model_display(
    explicit_model: Option<&str>,
    model_sub: Option<&str>,
    default_model: &str,
    explicit_source_label: &str,
) -> StatusMemoryModel {
    if let Some(model) = explicit_model {
        return StatusMemoryModel {
            model: model.to_string(),
            source_label: explicit_source_label.to_string(),
        };
    }

    if let Some(model) = model_sub {
        return StatusMemoryModel {
            model: model.to_string(),
            source_label: "config.model_sub".to_string(),
        };
    }

    StatusMemoryModel {
        model: default_model.to_string(),
        source_label: "memory default".to_string(),
    }
}

fn sanitize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let Ok(mut url) = Url::parse(trimmed) else {
        return None;
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_string()).filter(|value| !value.is_empty())
}
