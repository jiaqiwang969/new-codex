//! UTM 虚拟机管理器

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

pub struct UTMManager {
    scripts_dir: PathBuf,
    workspace_root: PathBuf,
}

impl UTMManager {
    pub fn new(scripts_dir: PathBuf, workspace_root: PathBuf) -> Self {
        Self {
            scripts_dir,
            workspace_root,
        }
    }

    /// 列出所有虚拟机
    pub async fn list_vms(&self) -> Result<Vec<String>> {
        let output = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "cd {} && scripts/utm-vmctl.sh list",
                self.workspace_root.display()
            ))
            .output()
            .await
            .context("Failed to list VMs")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let vms = stdout
            .lines()
            .map(|line| line.split('\t').next().unwrap_or("").to_string())
            .filter(|name| !name.is_empty())
            .collect();

        Ok(vms)
    }

    /// 检查虚拟机是否存在
    pub async fn vm_exists(&self, name: &str) -> Result<bool> {
        let vms = self.list_vms().await?;
        Ok(vms.iter().any(|vm| vm == name))
    }

    /// 获取虚拟机 IP
    pub async fn get_vm_ip(&self, name: &str) -> Result<String> {
        // 使用 osascript 查询 VM IP
        let script = format!(
            r#"
tell application "UTM"
    query ip virtual machine named "{}"
end tell
"#,
            name
        );

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .context("Failed to get VM IP")?;

        let ip = String::from_utf8_lossy(&output.stdout)
            .trim()
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        if ip.is_empty() {
            return Err(anyhow!("Could not detect IP for VM '{}'", name));
        }

        Ok(ip)
    }

    /// 等待虚拟机启动
    pub async fn wait_for_vm_ready(&self, name: &str, timeout_secs: u64) -> Result<String> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow!("Timeout waiting for VM '{}' to be ready", name));
            }

            match self.get_vm_ip(name).await {
                Ok(ip) => {
                    // 验证 SSH 连接
                    if self.check_ssh(&ip).await.is_ok() {
                        tracing::info!(vm_name = %name, vm_ip = %ip, "VM is ready");
                        return Ok(ip);
                    }
                }
                Err(_) => {
                    // VM 还没有获得 IP，继续等待
                }
            }

            sleep(Duration::from_secs(2)).await;
        }
    }

    /// 检查 SSH 连接
    async fn check_ssh(&self, ip: &str) -> Result<()> {
        let output = Command::new("ssh")
            .args(&[
                "-o",
                "ConnectTimeout=2",
                "-o",
                "StrictHostKeyChecking=no",
                &format!("jqwang@{}", ip),
                "true",
            ])
            .output()
            .await
            .context("SSH check failed")?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!("SSH connection failed"))
        }
    }

    /// 停止虚拟机
    pub async fn stop_vm(&self, name: &str) -> Result<()> {
        let script = format!(
            r#"
tell application "UTM"
    stop virtual machine named "{}" by request
end tell
"#,
            name
        );

        Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .context("Failed to stop VM")?;

        tracing::info!(vm_name = %name, "VM stopped");
        Ok(())
    }

    /// 删除虚拟机
    pub async fn delete_vm(&self, name: &str) -> Result<()> {
        // 先停止 VM
        let _ = self.stop_vm(name).await;

        // 等待 VM 停止
        sleep(Duration::from_secs(5)).await;

        // 删除 VM
        let script = format!(
            r#"
tell application "UTM"
    delete virtual machine named "{}"
end tell
"#,
            name
        );

        Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .context("Failed to delete VM")?;

        tracing::info!(vm_name = %name, "VM deleted");
        Ok(())
    }

    /// 在虚拟机中执行命令
    pub async fn exec_in_vm(&self, ip: &str, cmd: &str) -> Result<String> {
        let output = Command::new("ssh")
            .args(&[
                "-o",
                "StrictHostKeyChecking=no",
                &format!("jqwang@{}", ip),
                cmd,
            ])
            .output()
            .await
            .context("Failed to execute command in VM")?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(anyhow!(
                "Command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// 从虚拟机复制文件
    pub async fn copy_from_vm(&self, ip: &str, remote_path: &str, local_path: &str) -> Result<()> {
        let remote = format!("jqwang@{}:{}", ip, remote_path);

        let output = Command::new("rsync")
            .args(&[
                "-avz",
                "-e",
                "ssh -o StrictHostKeyChecking=no",
                &remote,
                local_path,
            ])
            .output()
            .await
            .context("Failed to copy from VM")?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "Copy failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    /// 复制文件到虚拟机
    pub async fn copy_to_vm(&self, local_path: &str, ip: &str, remote_path: &str) -> Result<()> {
        let remote = format!("jqwang@{}:{}", ip, remote_path);

        let output = Command::new("rsync")
            .args(&[
                "-avz",
                "-e",
                "ssh -o StrictHostKeyChecking=no",
                local_path,
                &remote,
            ])
            .output()
            .await
            .context("Failed to copy to VM")?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "Copy failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
