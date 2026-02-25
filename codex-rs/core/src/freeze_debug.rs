use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Installs a global panic hook that automatically clones the Codex state into an isolated NixOS VM when a crash occurs.
pub const FREEZE_SCRIPT: &str = include_str!("../../scripts/freeze-debug-vm.sh");

pub fn install_freeze_panic_hook(config: &crate::config::Config) {
    if !config
        .features
        .enabled(crate::features::Feature::FreezeSandboxDebug)
    {
        return;
    }
    if HOOK_INSTALLED.swap(true, Ordering::Relaxed) {
        return; // Already installed
    }

    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // First, call the original hook so the user still sees the panic stack trace
        default_hook(panic_info);

        eprintln!("\n========================================================");
        eprintln!("💀 Codex Panic Detected! Initiating Time-Freeze Sandbox Cloner...");
        eprintln!("========================================================");

        // Find the current workspace dynamically if possible
        let repo_root = std::env::current_dir()
            .ok()
            .and_then(|cwd| crate::git_info::resolve_root_git_project_for_trust(&cwd));

        // Write the panic info to a log file so the sandbox Codex knows what happened
        if let Some(ref root) = repo_root {
            let panic_msg = format!("{panic_info:#?}");
            let _ = std::fs::write(root.join("last_panic.log"), panic_msg);
        }

        // Use absolute path for the script for now (tailored to jqwang's machine)
        let script_path = "/Users/jqwang/01-agent/new-codex/scripts/freeze-debug-vm.sh";

        let mut cmd = Command::new("bash");
        cmd.arg(script_path);

        if let Some(root) = repo_root {
            cmd.arg(root.to_string_lossy().as_ref());
        }

        match cmd.status() {
            Ok(status) if status.success() => {
                eprintln!(
                    "✅ Sandbox cloning completed successfully! You can now debug in isolation."
                );
            }
            Ok(status) => {
                eprintln!("⚠️ Sandbox script exited with {status}.");
            }
            Err(e) => {
                eprintln!("❌ Failed to execute freeze-debug-vm.sh: {e}");
            }
        }
        eprintln!("========================================================");
    }));
}

// Force rebuild to pick up new bash script

// Force rebuild to pick up new bash script with --noprofile --norc

// Force rebuild to pick up new bash script with unescaped vars

// Force rebuild to pick up new bash script with nixos-debug-base
