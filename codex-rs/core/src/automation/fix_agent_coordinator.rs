//! 修复 Agent 协调器 - 在隔离环境中运行修复

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::time::{sleep, Duration};

use super::snapshot::{FreezeSnapshot, FixVM, FixResult};
use super::utm_manager::UTMManager;

pub struct FixAgentCoordinator {
    utm_manager: Arc<UTMManager>,
    workspace_root: PathBuf,
}

impl FixAgentCoordinator {
    pub fn new(utm_manager: Arc<UTMManager>, workspace_root: PathBuf) -> Self {
        Self {
            utm_manager,
            workspace_root,
        }
    }

    /// 在隔离 VM 中运行修复
    pub async fn run_fix_in_vm(
        &self,
        fix_vm: &FixVM,
        snapshot: &FreezeSnapshot,
    ) -> Result<FixResult> {
        tracing::info!(
            vm_name = %fix_vm.name,
            snapshot_id = %snapshot.id,
            "Starting fix agent in VM"
        );

        // 1. 等待 VM 启动
        let vm_ip = self
            .utm_manager
            .wait_for_vm_ready(&fix_vm.name, 120)
            .await
            .context("Failed to wait for VM")?;

        tracing::info!(vm_name = %fix_vm.name, vm_ip = %vm_ip, "VM is ready");

        // 2. 恢复工作区
        self.restore_workspace(&vm_ip, snapshot)
            .await
            .context("Failed to restore workspace")?;

        tracing::info!(vm_ip = %vm_ip, "Workspace restored");

        // 3. 验证编译错误存在
        let verify_result = self
            .utm_manager
            .exec_in_vm(&vm_ip, "cd /workspace && cargo check 2>&1")
            .await
            .context("Failed to verify compile error")?;

        if !verify_result.contains("error") {
            return Err(anyhow!(
                "Expected compile error not found in VM. Output: {}",
                verify_result
            ));
        }

        tracing::info!("Compile error verified in VM");

        // 4. 生成修复 prompt
        let fix_prompt = self.generate_fix_prompt(snapshot);

        // 5. 在 VM 中运行修复 Agent
        let fix_result = self
            .run_fix_agent_in_vm(&vm_ip, &fix_prompt, snapshot)
            .await
            .context("Failed to run fix agent")?;

        tracing::info!(
            success = fix_result.success,
            "Fix agent completed"
        );

        Ok(fix_result)
    }

    /// 恢复工作区到出错时刻
    async fn restore_workspace(&self, vm_ip: &str, snapshot: &FreezeSnapshot) -> Result<()> {
        // 1. 复制源代码到 VM
        tracing::debug!("Copying workspace to VM");
        self.utm_manager
            .copy_to_vm(
                &self.workspace_root.to_string_lossy(),
                vm_ip,
                "/workspace",
            )
            .await
            .context("Failed to copy workspace")?;

        // 2. 在 VM 中恢复到特定 git commit
        let restore_cmd = format!(
            "cd /workspace && git checkout {} 2>&1",
            snapshot.git_commit
        );

        self.utm_manager
            .exec_in_vm(vm_ip, &restore_cmd)
            .await
            .context("Failed to checkout git commit")?;

        tracing::debug!(commit = %snapshot.git_commit, "Git commit restored");

        // 3. 应用 ghost snapshot 中的修改
        // 这里需要从 session history 中获取 ghost snapshot 的 patch
        // 暂时跳过，实际实现时需要集成 ghost snapshot 恢复

        Ok(())
    }

    /// 生成修复 prompt
    fn generate_fix_prompt(&self, snapshot: &FreezeSnapshot) -> String {
        let error_summary = snapshot
            .error
            .errors
            .iter()
            .map(|e| {
                format!(
                    "{}:{}: {} ({})",
                    e.file, e.line, e.message, e.severity
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"You are in an isolated development environment. Your task is to fix the following compilation errors:

{}

Full compiler output:
{}

Instructions:
1. Analyze the errors carefully
2. Make minimal changes to fix them
3. Run `cargo check` to verify the fix
4. Do NOT make any unnecessary changes
5. Focus only on fixing the compilation errors

After fixing, the compilation should succeed with `cargo check`."#,
            error_summary, snapshot.error.full_output
        )
    }

    /// 在 VM 中运行修复 Agent
    async fn run_fix_agent_in_vm(
        &self,
        vm_ip: &str,
        fix_prompt: &str,
        snapshot: &FreezeSnapshot,
    ) -> Result<FixResult> {
        // 创建修复脚本
        let fix_script = self.create_fix_script(fix_prompt);

        // 复制脚本到 VM
        let script_path = format!("/tmp/fix-agent-{}.sh", snapshot.id);
        self.utm_manager
            .copy_to_vm(&fix_script, vm_ip, &script_path)
            .await
            .context("Failed to copy fix script")?;

        // 在 VM 中执行修复脚本
        let exec_cmd = format!("bash {} 2>&1", script_path);
        let output = self
            .utm_manager
            .exec_in_vm(vm_ip, &exec_cmd)
            .await
            .context("Failed to execute fix script")?;

        // 验证修复是否成功
        let verify_cmd = "cd /workspace && cargo check 2>&1";
        let verify_output = self
            .utm_manager
            .exec_in_vm(vm_ip, verify_cmd)
            .await
            .context("Failed to verify fix")?;

        let success = !verify_output.contains("error");

        // 获取修改的文件列表
        let files_cmd = "cd /workspace && git diff --name-only";
        let files_output = self
            .utm_manager
            .exec_in_vm(vm_ip, files_cmd)
            .await
            .unwrap_or_default();

        let fixed_files = files_output
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(FixResult {
            success,
            error: if success {
                None
            } else {
                Some(verify_output.clone())
            },
            fixed_files,
            compile_output: verify_output,
        })
    }

    /// 创建修复脚本
    fn create_fix_script(&self, fix_prompt: &str) -> String {
        // 这里应该调用 Codex CLI 来运行修复 Agent
        // 暂时返回一个占位符脚本
        format!(
            r#"#!/bin/bash
set -e

cd /workspace

# 运行 Codex 修复 Agent
# 这里需要集成 Codex CLI 的 exec 命令
# codex exec "{}"

# 验证修复
cargo check

echo "Fix completed"
"#,
            fix_prompt.replace("\"", "\\\"")
        )
    }
}
