use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

#[derive(Serialize, Deserialize, Debug)]
pub struct EphemeralSandboxArgs {
    pub command: String,
    pub justification: String,
}

pub struct EphemeralSandboxHandler;

#[async_trait]
impl ToolHandler for EphemeralSandboxHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolPayload::Function { arguments } = invocation.payload else {
            return Err(FunctionCallError::Fatal(
                "Expected function payload".to_string(),
            ));
        };

        let args: EphemeralSandboxArgs = match serde_json::from_str(&arguments) {
            Ok(args) => args,
            Err(e) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "Invalid arguments: {e}"
                )));
            }
        };

        let vm_name = format!(
            "nixos-agent-sandbox-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        let source_dir_host = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let bash_script = format!(
            r#"#!/usr/bin/env bash
set -e
SOURCE_CODEX_DIR="/mnt/mac{}"
VM_NAME="{}"

orb clone nixos-dev $VM_NAME >/dev/null
orb start $VM_NAME >/dev/null

echo "Running sandboxed command: {}"
output=\$(orb -m $VM_NAME -u jqwang bash -c "cd \$SOURCE_CODEX_DIR && {}" 2>&1 || true)
orb delete $VM_NAME >/dev/null
echo "\$output"
"#,
            source_dir_host, vm_name, args.command, args.command
        );

        let script_path = std::env::temp_dir().join(format!("{vm_name}.sh"));
        if let Err(e) = std::fs::write(&script_path, &bash_script) {
            return Err(FunctionCallError::Fatal(format!(
                "Failed to write script: {e}"
            )));
        }

        let output = match std::process::Command::new("bash")
            .arg(&script_path)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                let _ = std::fs::remove_file(&script_path);
                return Err(FunctionCallError::Fatal(format!(
                    "Failed to run script: {e}"
                )));
            }
        };

        let mut output_str = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.stderr.is_empty() {
            output_str.push_str("\n--- STDERR ---\n");
            output_str.push_str(&String::from_utf8_lossy(&output.stderr));
        }

        let _ = std::fs::remove_file(script_path);

        Ok(FunctionToolOutput::from_text(
            format!(
                "Executed safely in disposable VM sandbox ({vm_name})\n\nOutput:\n{output_str}"
            ),
            Some(output.status.success()),
        ))
    }
}
