//! 快照数据结构

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeSnapshot {
    pub id: String,
    pub timestamp: SystemTime,
    pub git_commit: String,
    pub git_branch: String,
    pub error: super::compile_error_freezer::CompileResult,
    pub environment: serde_json::Value,
    pub ghost_snapshot_id: String,
    pub fix_vm: Option<FixVM>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixVM {
    pub name: String,
    pub snapshot_id: String,
    pub created_at: SystemTime,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileError {
    pub message: String,
    pub severity: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixResult {
    pub success: bool,
    pub error: Option<String>,
    pub fixed_files: Vec<String>,
    pub compile_output: String,
}
