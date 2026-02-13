//! Memory subsystem for startup extraction and consolidation.
//!
//! The startup memory pipeline is split into two phases:
//! - Phase 1: select rollouts, extract stage-1 raw memories, persist stage-1 outputs, and enqueue consolidation.
//! - Phase 2: claim a global consolidation lock, materialize consolidation inputs, and dispatch one consolidation agent.

pub(crate) mod prompts;
mod stage_one;
mod startup;
mod storage;

#[cfg(test)]
mod tests;

use crate::path_utils::normalize_for_path_comparison;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;

/// Subagent source label used to identify consolidation tasks.
const MEMORY_CONSOLIDATION_SUBAGENT_LABEL: &str = "memory_consolidation";
const ROLLOUT_SUMMARIES_SUBDIR: &str = "rollout_summaries";
const RAW_MEMORIES_FILENAME: &str = "raw_memories.md";
/// Maximum number of rollout candidates processed per startup pass.
const MAX_ROLLOUTS_PER_STARTUP: usize = 64;
/// Concurrency cap for startup memory extraction and consolidation scheduling.
const PHASE_ONE_CONCURRENCY_LIMIT: usize = MAX_ROLLOUTS_PER_STARTUP;
/// Maximum number of recent raw memories retained for global consolidation.
const MAX_RAW_MEMORIES_FOR_GLOBAL: usize = 1_024;
/// Fallback stage-1 rollout truncation limit (tokens) when model metadata
/// does not include a valid context window.
const DEFAULT_STAGE_ONE_ROLLOUT_TOKEN_LIMIT: usize = 150_000;
/// Maximum number of tokens from `memory_summary.md` injected into memory tool
/// developer instructions.
const MEMORY_TOOL_DEVELOPER_INSTRUCTIONS_SUMMARY_TOKEN_LIMIT: usize = 5_000;
/// Portion of the model effective input window reserved for the stage-1 rollout
/// input.
///
/// Keeping this below 100% leaves room for system instructions, prompt framing,
/// and model output.
const STAGE_ONE_CONTEXT_WINDOW_PERCENT: i64 = 70;
/// Maximum rollout age considered for phase-1 extraction.
const PHASE_ONE_MAX_ROLLOUT_AGE_DAYS: i64 = 30;
/// Minimum rollout idle time required before phase-1 extraction.
const PHASE_ONE_MIN_ROLLOUT_IDLE_HOURS: i64 = 12;
/// Lease duration (seconds) for phase-1 job ownership.
const PHASE_ONE_JOB_LEASE_SECONDS: i64 = 3_600;
/// Backoff delay (seconds) before retrying a failed stage-1 extraction job.
const PHASE_ONE_JOB_RETRY_DELAY_SECONDS: i64 = 3_600;
/// Lease duration (seconds) for phase-2 consolidation job ownership.
const PHASE_TWO_JOB_LEASE_SECONDS: i64 = 3_600;
/// Backoff delay (seconds) before retrying a failed phase-2 consolidation job.
const PHASE_TWO_JOB_RETRY_DELAY_SECONDS: i64 = 3_600;
/// Heartbeat interval (seconds) for phase-2 running jobs.
const PHASE_TWO_JOB_HEARTBEAT_SECONDS: u64 = 30;
pub(crate) const MEMORY_SCOPE_KIND_CWD: &str = "cwd";
pub(crate) const MEMORY_SCOPE_KIND_USER: &str = "user";
pub(crate) const MEMORY_SCOPE_KIND_GLOBAL: &str = "global";
pub(crate) const MEMORY_SCOPE_KEY_USER: &str = "user";
const MEMORY_SUBDIR: &str = "memory";
const MEMORY_SUMMARY_FILENAME: &str = "memory_summary.md";
const CWD_MEMORY_BUCKET_HEX_LEN: usize = 16;

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
    root.join(ROLLOUT_SUMMARIES_SUBDIR)
}

fn raw_memories_file(root: &Path) -> PathBuf {
    root.join(RAW_MEMORIES_FILENAME)
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

/// Starts the memory startup pipeline for eligible root sessions.
///
/// This is the single entrypoint that `codex` uses to trigger memory startup.
pub(crate) use startup::start_memories_startup_task;
