//! 编译错误冻结器 - 创建快照并克隆隔离环境

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use uuid::Uuid;
use tokio::process::Command as TokioCommand;

use super::snapshot::{FreezeSnapshot, FixVM, CompileError};
use super::utm_manager::UTMManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    pub success: bool,
    pub errors: Vec<CompileError>,
    pub warnings: Vec<CompileError>,
    pub full_output: String,
}

pub struct CompileErrorFreezer {
    snapshot_dir: PathBuf,
    utm_manager: Arc<UTMManager>,
    workspace_root: PathBuf,
}

impl CompileErrorFreezer {
    pub fn new(
        snapshot_dir: PathBuf,
        utm_manager: Arc<UTMManager>,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            snapshot_dir,
            utm_manager,
            workspace_root,
        }
    }

    /// 检测编译错误
    pub async fn detect_compile_errors(&self, cwd: &Path) -> Result<CompileResult> {
        let output = TokioCommand::new("cargo")
            .arg("check")
            .arg("--message-format=json")
            .current_dir(cwd)
            .output()
            .await
            .context("Failed to run cargo check")?;

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(reason) = msg.get("reason").and_then(|r| r.as_str()) {
                    if reason == "compiler-message" {
                        if let Some(message) = msg.get("message") {
                            if let Ok(error) = self.parse_compiler_message(message) {
                                match error.severity.as_str() {
                                    "error" => errors.push(error),
                                    "warning" => warnings.push(error),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        let success = errors.is_empty();
        let full_output = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(CompileResult {
            success,
            errors,
            warnings,
            full_output,
        })
    }

    /// 解析编译器消息
    fn parse_compiler_message(&self, msg: &serde_json::Value) -> Result<CompileError> {
        let message = msg
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();

        let severity = msg
            .get("level")
            .and_then(|l| l.as_str())
            .unwrap_or("error")
            .to_string();

        let spans = msg.get("spans").and_then(|s| s.as_array()).unwrap_or(&vec![]);
        let (file, line) = if let Some(span) = spans.first() {
            let file = span
                .get("file_name")
                .and_then(|f| f.as_str())
                .unwrap_or("unknown")
                .to_string();
            let line = span
                .get("line_start")
                .and_then(|l| l.as_u64())
                .unwrap_or(0) as u32;
            (file, line)
        } else {
            ("unknown".to_string(), 0)
        };

        Ok(CompileError {
            message,
            severity,
            file,
            line,
        })
    }

    /// 时间定格 - 创建快照
    pub async fn freeze_on_error(
        &self,
        ctx: &std::sync::Arc<crate::codex::TurnContext>,
        compile_result: &CompileResult,
    ) -> Result<FreezeSnapshot> {
        let snapshot_id = Uuid::new_v4().to_string();

        // 获取 git 信息
        let git_commit = self.get_git_commit(&ctx.cwd)?;
        let git_branch = self.get_git_branch(&ctx.cwd)?;

        // 捕获环境信息
        let environment = self.capture_environment(&ctx.cwd).await?;

        // 获取最新的 ghost snapshot
        let ghost_snapshot_id = self.get_latest_ghost_snapshot(ctx).await?;

        // 创建快照对象
        let snapshot = FreezeSnapshot {
            id: snapshot_id.clone(),
            timestamp: SystemTime::now(),
            git_commit,
            git_branch,
            error: compile_result.clone(),
            environment,
            ghost_snapshot_id,
            fix_vm: None,
        };

        // 保存快照到磁盘
        self.save_snapshot(&snapshot).await?;

        // 创建隔离 VM
        let fix_vm = self.create_fix_vm(&snapshot).await?;

        // 更新快照中的 VM 信息
        let mut snapshot = snapshot;
        snapshot.fix_vm = Some(fix_vm.clone());
        self.save_snapshot(&snapshot).await?;

        tracing::info!(
            snapshot_id = %snapshot.id,
            fix_vm = %fix_vm.name,
            "Freeze snapshot created"
        );

        Ok(snapshot)
    }

    /// 获取 git commit hash
    fn get_git_commit(&self, cwd: &Path) -> Result<String> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(cwd)
            .output()
            .context("Failed to get git commit")?;

        Ok(String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string())
    }

    /// 获取 git branch
    fn get_git_branch(&self, cwd: &Path) -> Result<String> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .current_dir(cwd)
            .output()
            .context("Failed to get git branch")?;

        Ok(String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string())
    }

    /// 捕获环境信息
    async fn capture_environment(&self, cwd: &Path) -> Result<serde_json::Value> {
        // 读取 flake.lock 的 hash
        let flake_lock_path = cwd.join("flake.lock");
        let flake_lock_hash = if flake_lock_path.exists() {
            let content = tokio::fs::read_to_string(&flake_lock_path).await?;
            format!("{:x}", md5::compute(content.as_bytes()))
        } else {
            "none".to_string()
        };

        // 获取 Rust 版本
        let rustc_output = TokioCommand::new("rustc")
            .arg("--version")
            .output()
            .await
            .ok();

        let rust_version = rustc_output
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        Ok(serde_json::json!({
            "flake_lock_hash": flake_lock_hash,
            "rust_version": rust_version,
            "timestamp": SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
        }))
    }

    /// 获取最新的 ghost snapshot ID
    async fn get_latest_ghost_snapshot(
        &self,
        _ctx: &std::sync::Arc<crate::codex::TurnContext>,
    ) -> Result<String> {
        // 这里应该从 session history 中获取最新的 ghost snapshot
        // 暂时返回占位符
        Ok("ghost-snapshot-latest".to_string())
    }

    /// 创建隔离 VM
    async fn create_fix_vm(&self, snapshot: &FreezeSnapshot) -> Result<FixVM> {
        let vm_name = format!("vm-fix-{}", &snapshot.id[..8]);

        // 使用 utm-vmctl.sh 克隆 VM
        let output = TokioCommand::new("bash")
            .arg("-c")
            .arg(format!(
                "cd {} && scripts/utm-vmctl.sh create --template vm-aarch64-utm --start",
                self.workspace_root.display()
            ))
            .output()
            .await
            .context("Failed to create VM")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to create VM: {}", stderr));
        }

        let cloned_name = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        tracing::info!(
            snapshot_id = %snapshot.id,
            vm_name = %cloned_name,
            "VM created"
        );

        Ok(FixVM {
            name: cloned_name,
            snapshot_id: snapshot.id.clone(),
            created_at: SystemTime::now(),
            status: "starting".to_string(),
        })
    }

    /// 保存快照到磁盘
    async fn save_snapshot(&self, snapshot: &FreezeSnapshot) -> Result<()> {
        tokio::fs::create_dir_all(&self.snapshot_dir)
            .await
            .context("Failed to create snapshot directory")?;

        let snapshot_path = self.snapshot_dir.join(format!("{}.json", snapshot.id));
        let json = serde_json::to_string_pretty(snapshot)?;

        tokio::fs::write(&snapshot_path, json)
            .await
            .context("Failed to write snapshot")?;

        tracing::debug!(
            snapshot_id = %snapshot.id,
            path = %snapshot_path.display(),
            "Snapshot saved"
        );

        Ok(())
    }
}

use std::sync::Arc;
