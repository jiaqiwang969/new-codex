//! Undo 替换器 - 将修复应用到主工作区

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::process::Command;

use super::snapshot::{FreezeSnapshot, FixVM};
use super::utm_manager::UTMManager;

pub struct UndoReplacer {
    utm_manager: Arc<UTMManager>,
    main_workspace: PathBuf,
}

impl UndoReplacer {
    pub fn new(utm_manager: Arc<UTMManager>, main_workspace: PathBuf) -> Self {
        Self {
            utm_manager,
            main_workspace,
        }
    }

    /// 应用修复并通过 undo 替换
    pub async fn apply_fix_and_undo(
        &self,
        fix_vm: &FixVM,
        snapshot: &FreezeSnapshot,
    ) -> Result<()> {
        tracing::info!(
            vm_name = %fix_vm.name,
            snapshot_id = %snapshot.id,
            "Applying fix and undo replacement"
        );

        // 1. 获取 VM IP
        let vm_ip = self
            .utm_manager
            .get_vm_ip(&fix_vm.name)
            .await
            .context("Failed to get VM IP")?;

        // 2. 从 Fix-VM 复制修复文件到主工作区
        self.copy_fixed_files(&vm_ip, &self.main_workspace)
            .await
            .context("Failed to copy fixed files")?;

        tracing::info!("Fixed files copied to main workspace");

        // 3. 验证编译成功
        let compile_result = self
            .verify_compile(&self.main_workspace)
            .await
            .context("Failed to verify compile")?;

        if !compile_result {
            return Err(anyhow!("Compile verification failed after applying fix"));
        }

        tracing::info!("Compile verification passed");

        // 4. 创建新的 ghost snapshot（可选，取决于是否启用 undo 功能）
        // self.create_new_snapshot(&self.main_workspace).await?;

        // 5. 销毁 Fix-VM
        self.utm_manager
            .delete_vm(&fix_vm.name)
            .await
            .context("Failed to delete Fix-VM")?;

        tracing::info!(vm_name = %fix_vm.name, "Fix-VM destroyed");

        // 6. 清理快照文件
        self.cleanup_snapshot(snapshot)
            .await
            .context("Failed to cleanup snapshot")?;

        tracing::info!(snapshot_id = %snapshot.id, "Snapshot cleaned up");

        Ok(())
    }

    /// 从 Fix-VM 复制修复文件到主工作区
    async fn copy_fixed_files(&self, vm_ip: &str, target_dir: &Path) -> Result<()> {
        // 使用 rsync 复制修复后的文件
        // 排除 .git 和其他不必要的目录
        let output = Command::new("rsync")
            .args(&[
                "-avz",
                "--delete",
                "--exclude=.git",
                "--exclude=target",
                "--exclude=.codex",
                "-e",
                "ssh -o StrictHostKeyChecking=no",
                &format!("jqwang@{}:/workspace/", vm_ip),
                &target_dir.to_string_lossy(),
            ])
            .output()
            .await
            .context("Failed to run rsync")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("rsync failed: {}", stderr));
        }

        tracing::debug!("Files synced from VM to main workspace");
        Ok(())
    }

    /// 验证编译成功
    async fn verify_compile(&self, workspace: &Path) -> Result<bool> {
        let output = Command::new("cargo")
            .arg("check")
            .current_dir(workspace)
            .output()
            .await
            .context("Failed to run cargo check")?;

        Ok(output.status.success())
    }

    /// 创建新的 ghost snapshot
    async fn create_new_snapshot(&self, workspace: &Path) -> Result<()> {
        // 这里应该调用 ghost snapshot 创建逻辑
        // 暂时跳过，实际实现时需要集成 ghost snapshot 系统

        tracing::debug!("New ghost snapshot created");
        Ok(())
    }

    /// 清理快照文件
    async fn cleanup_snapshot(&self, snapshot: &FreezeSnapshot) -> Result<()> {
        // 删除快照文件
        let snapshot_dir = PathBuf::from(".time-travel-snapshots");
        let snapshot_path = snapshot_dir.join(format!("{}.json", snapshot.id));

        if snapshot_path.exists() {
            tokio::fs::remove_file(&snapshot_path)
                .await
                .context("Failed to remove snapshot file")?;

            tracing::debug!(
                snapshot_id = %snapshot.id,
                "Snapshot file removed"
            );
        }

        Ok(())
    }

    /// 恢复到特定快照（用于调试失败的修复）
    pub async fn restore_from_snapshot(&self, snapshot_id: &str) -> Result<()> {
        tracing::info!(snapshot_id = %snapshot_id, "Restoring from snapshot");

        // 1. 读取快照文件
        let snapshot_path = PathBuf::from(".time-travel-snapshots")
            .join(format!("{}.json", snapshot_id));

        if !snapshot_path.exists() {
            return Err(anyhow!("Snapshot not found: {}", snapshot_id));
        }

        let snapshot_json = tokio::fs::read_to_string(&snapshot_path)
            .await
            .context("Failed to read snapshot")?;

        let snapshot: FreezeSnapshot = serde_json::from_str(&snapshot_json)
            .context("Failed to parse snapshot")?;

        // 2. 恢复 git 状态
        let output = Command::new("git")
            .args(&["checkout", &snapshot.git_commit])
            .current_dir(&self.main_workspace)
            .output()
            .await
            .context("Failed to checkout git commit")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to restore git state"));
        }

        tracing::info!(
            snapshot_id = %snapshot.id,
            commit = %snapshot.git_commit,
            "Restored to snapshot"
        );

        Ok(())
    }
}
