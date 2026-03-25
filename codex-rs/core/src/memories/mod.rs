//! Memory subsystem for startup extraction and consolidation.
//!
//! The startup memory pipeline is split into two phases:
//! - Phase 1: select rollouts, extract stage-1 raw memories, persist stage-1 outputs, and enqueue consolidation.
//! - Phase 2: claim a global consolidation lock, materialize consolidation inputs, and dispatch one consolidation agent.

pub(crate) mod citations;
mod control;
mod phase1;
mod phase2;
pub(crate) mod prompts;
mod start;
mod storage;
#[cfg(test)]
mod tests;
pub(crate) mod usage;

use codex_protocol::openai_models::ReasoningEffort;

use crate::path_utils::normalize_for_path_comparison;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;

pub(crate) const MEMORY_SCOPE_KIND_CWD: &str = "cwd";
pub(crate) const MEMORY_SCOPE_KIND_USER: &str = "user";
pub(crate) const MEMORY_SCOPE_KIND_GLOBAL: &str = "global";
pub(crate) const MEMORY_SCOPE_KEY_USER: &str = "user";
const MEMORY_SUBDIR: &str = "memory";
const MEMORY_SUMMARY_FILENAME: &str = "memory_summary.md";
const CWD_MEMORY_BUCKET_HEX_LEN: usize = 16;
pub(crate) use control::clear_memory_root_contents;
/// Starts the memory startup pipeline for eligible root sessions.
/// This is the single entrypoint that `codex` uses to trigger memory startup.
///
/// This is the entry point to read and understand this module.
pub(crate) use start::start_memories_startup_task;

mod artifacts {
    pub(super) const ROLLOUT_SUMMARIES_SUBDIR: &str = "rollout_summaries";
    pub(super) const RAW_MEMORIES_FILENAME: &str = "raw_memories.md";
}

/// Phase 1 (startup extraction).
mod phase_one {
    /// Default model used for phase 1.
    pub(super) const MODEL: &str = "gpt-5.1-codex-mini";
    /// Default reasoning effort used for phase 1.
    pub(super) const REASONING_EFFORT: super::ReasoningEffort = super::ReasoningEffort::Low;
    /// Prompt used for phase 1.
    pub(super) const PROMPT: &str = include_str!("../../templates/memories/stage_one_system.md");
    /// Concurrency cap for startup memory extraction and consolidation scheduling.
    pub(super) const CONCURRENCY_LIMIT: usize = 8;
    /// Fallback stage-1 rollout truncation limit (tokens) when model metadata
    /// does not include a valid context window.
    pub(super) const DEFAULT_STAGE_ONE_ROLLOUT_TOKEN_LIMIT: usize = 150_000;
    /// Maximum number of tokens from `memory_summary.md` injected into memory
    /// tool developer instructions.
    pub(super) const MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_SUMMARY_TOKEN_LIMIT: usize = 5_000;
    /// Portion of the model effective input window reserved for the stage-1
    /// rollout input.
    ///
    /// Keeping this below 100% leaves room for system instructions, prompt
    /// framing, and model output.
    pub(super) const CONTEXT_WINDOW_PERCENT: i64 = 70;
    /// Lease duration (seconds) for phase-1 job ownership.
    pub(super) const JOB_LEASE_SECONDS: i64 = 3_600;
    /// Backoff delay (seconds) before retrying a failed stage-1 extraction job.
    pub(super) const JOB_RETRY_DELAY_SECONDS: i64 = 3_600;
    /// Maximum number of threads to scan.
    pub(super) const THREAD_SCAN_LIMIT: usize = 5_000;
    /// Size of the batches when pruning old thread memories.
    pub(super) const PRUNE_BATCH_SIZE: usize = 200;
}

/// Phase 2 (aka `Consolidation`).
mod phase_two {
    /// Default model used for phase 2.
    pub(super) const MODEL: &str = "gpt-5.3-codex";
    /// Default reasoning effort used for phase 2.
    pub(super) const REASONING_EFFORT: super::ReasoningEffort = super::ReasoningEffort::Medium;
    /// Lease duration (seconds) for phase-2 consolidation job ownership.
    pub(super) const JOB_LEASE_SECONDS: i64 = 3_600;
    /// Backoff delay (seconds) before retrying a failed phase-2 consolidation
    /// job.
    pub(super) const JOB_RETRY_DELAY_SECONDS: i64 = 3_600;
    /// Heartbeat interval (seconds) for phase-2 running jobs.
    pub(super) const JOB_HEARTBEAT_SECONDS: u64 = 90;
}

mod entire_summary {
    /// Default model used for Entire checkpoint summarization.
    pub(super) const MODEL: &str = "claude-sonnet-4-6";
}

mod metrics {
    /// Number of phase-1 startup jobs grouped by status.
    pub(super) const MEMORY_PHASE_ONE_JOBS: &str = "codex.memory.phase1";
    /// End-to-end latency for a single phase-1 startup run.
    pub(super) const MEMORY_PHASE_ONE_E2E_MS: &str = "codex.memory.phase1.e2e_ms";
    /// Number of raw memories produced by phase-1 startup extraction.
    pub(super) const MEMORY_PHASE_ONE_OUTPUT: &str = "codex.memory.phase1.output";
    /// Histogram for aggregate token usage across one phase-1 startup run.
    pub(super) const MEMORY_PHASE_ONE_TOKEN_USAGE: &str = "codex.memory.phase1.token_usage";
    /// Number of phase-2 startup jobs grouped by status.
    pub(super) const MEMORY_PHASE_TWO_JOBS: &str = "codex.memory.phase2";
    /// End-to-end latency for a single phase-2 consolidation run.
    pub(super) const MEMORY_PHASE_TWO_E2E_MS: &str = "codex.memory.phase2.e2e_ms";
    /// Number of stage-1 memories included in each phase-2 consolidation step.
    pub(super) const MEMORY_PHASE_TWO_INPUT: &str = "codex.memory.phase2.input";
    /// Histogram for aggregate token usage across one phase-2 consolidation run.
    pub(super) const MEMORY_PHASE_TWO_TOKEN_USAGE: &str = "codex.memory.phase2.token_usage";
}

pub const DEFAULT_MEMORY_PHASE_ONE_MODEL: &str = phase_one::MODEL;
pub const DEFAULT_MEMORY_PHASE_TWO_MODEL: &str = phase_two::MODEL;
pub const DEFAULT_ENTIRE_SUMMARY_MODEL: &str = entire_summary::MODEL;

pub fn memory_root(codex_home: &Path) -> PathBuf {
    codex_home.join("memories")
}

pub(crate) fn memory_root_for_cwd(codex_home: &Path, cwd: &Path) -> PathBuf {
    let bucket = memory_bucket_for_cwd(cwd);
    codex_home.join("memories").join(bucket).join(MEMORY_SUBDIR)
}

pub(crate) fn memory_scope_key_for_cwd(cwd: &Path) -> String {
    normalize_cwd_for_memory(cwd).display().to_string()
}

pub(crate) fn memory_root_for_user(codex_home: &Path) -> PathBuf {
    codex_home
        .join("memories")
        .join(MEMORY_SCOPE_KEY_USER)
        .join(MEMORY_SUBDIR)
}

pub(crate) fn memory_summary_sha256(memory_summary: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(memory_summary.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn memory_scope_version(scope_kind: &str, summary_sha256: &str) -> String {
    let short_hash = summary_sha256.get(..12).unwrap_or(summary_sha256);
    format!("{scope_kind}:{short_hash}")
}

pub(crate) fn memory_binding_key(scope_version: &str, summary_sha256: &str) -> String {
    format!("{scope_version}:{summary_sha256}")
}

pub(crate) fn memory_summary_file(root: &Path) -> PathBuf {
    root.join(MEMORY_SUMMARY_FILENAME)
}

fn rollout_summaries_dir(root: &Path) -> PathBuf {
    root.join(artifacts::ROLLOUT_SUMMARIES_SUBDIR)
}

fn raw_memories_file(root: &Path) -> PathBuf {
    root.join(artifacts::RAW_MEMORIES_FILENAME)
}

async fn ensure_layout(root: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(rollout_summaries_dir(root)).await
}

fn memory_bucket_for_cwd(cwd: &Path) -> String {
    let normalized = normalize_cwd_for_memory(cwd);
    let normalized = normalized.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let full_hash = format!("{:x}", hasher.finalize());
    full_hash[..CWD_MEMORY_BUCKET_HEX_LEN].to_string()
}

fn normalize_cwd_for_memory(cwd: &Path) -> PathBuf {
    normalize_for_path_comparison(cwd).unwrap_or_else(|_| cwd.to_path_buf())
}

#[derive(Clone, Debug)]
pub(crate) struct MemoryReadPathSource {
    pub(crate) scope_kind: &'static str,
    pub(crate) memory_root: PathBuf,
    pub(crate) memory_summary_path: PathBuf,
    pub(crate) memory_summary: String,
    pub(crate) memory_summary_sha256: String,
    pub(crate) memory_scope_version: String,
    pub(crate) memory_binding_key: String,
}

pub(crate) async fn select_memory_read_path_source(
    codex_home: &Path,
    cwd: &Path,
) -> Option<MemoryReadPathSource> {
    async fn try_load_source(
        scope_kind: &'static str,
        memory_root: PathBuf,
    ) -> Option<MemoryReadPathSource> {
        let memory_summary_path = memory_summary_file(&memory_root);
        let memory_summary = tokio::fs::read_to_string(&memory_summary_path)
            .await
            .ok()?
            .trim()
            .to_string();
        if memory_summary.is_empty() {
            return None;
        }

        let summary_sha256 = memory_summary_sha256(&memory_summary);
        let scope_version = memory_scope_version(scope_kind, &summary_sha256);
        let binding_key = memory_binding_key(&scope_version, &summary_sha256);

        Some(MemoryReadPathSource {
            scope_kind,
            memory_root,
            memory_summary_path,
            memory_summary,
            memory_summary_sha256: summary_sha256,
            memory_scope_version: scope_version,
            memory_binding_key: binding_key,
        })
    }

    let cwd_memory_root = memory_root_for_cwd(codex_home, cwd);
    if let Some(source) = try_load_source(MEMORY_SCOPE_KIND_CWD, cwd_memory_root).await {
        return Some(source);
    }

    let user_memory_root = memory_root_for_user(codex_home);
    if let Some(source) = try_load_source(MEMORY_SCOPE_KIND_USER, user_memory_root).await {
        return Some(source);
    }

    try_load_source(MEMORY_SCOPE_KIND_GLOBAL, memory_root(codex_home)).await
}
