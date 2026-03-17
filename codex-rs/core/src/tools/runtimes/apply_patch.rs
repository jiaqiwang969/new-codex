//! Apply Patch runtime: executes verified patches under the orchestrator.
//!
//! Assumes `apply_patch` verification/approval happened upstream. Reuses that
//! decision to avoid re-prompting, builds the self-invocation command for
//! `codex --codex-run-as-apply-patch`, and runs under the current
//! `SandboxAttempt` with a minimal environment.
use crate::exec::ExecToolCallOutput;
use crate::guardian::GuardianApprovalRequest;
use crate::guardian::guardian_assessment_action_value;
use crate::guardian::review_approval_request;
use crate::guardian::routes_approval_to_guardian;
use crate::sandboxing::CommandSpec;
use crate::sandboxing::SandboxPermissions;
use crate::sandboxing::execute_env;
use crate::smart_access::SmartAccessApprovalOutcome;
use crate::smart_access::clear_runtime_context;
use crate::smart_access::endpoint_security_daemon_log_size;
use crate::smart_access::is_smart_access_mode;
use crate::smart_access::maybe_record_endpoint_security_runtime_mismatch;
use crate::smart_access::merge_human_approval_reason;
use crate::smart_access::review_smart_access_request;
use crate::tools::sandboxing::Approvable;
use crate::tools::sandboxing::ApprovalCtx;
use crate::tools::sandboxing::ExecApprovalRequirement;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::Sandboxable;
use crate::tools::sandboxing::SandboxablePreference;
use crate::tools::sandboxing::ToolCtx;
use crate::tools::sandboxing::ToolError;
use crate::tools::sandboxing::ToolRuntime;
use crate::tools::sandboxing::with_cached_approval;
use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::CODEX_CORE_APPLY_PATCH_ARG1;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ApplyPatchRequest {
    pub action: ApplyPatchAction,
    pub file_paths: Vec<AbsolutePathBuf>,
    pub changes: std::collections::HashMap<PathBuf, FileChange>,
    pub exec_approval_requirement: ExecApprovalRequirement,
    pub timeout_ms: Option<u64>,
    pub codex_exe: Option<PathBuf>,
}

#[derive(Default)]
pub struct ApplyPatchRuntime;

impl ApplyPatchRuntime {
    pub fn new() -> Self {
        Self
    }

    fn build_guardian_review_request(
        req: &ApplyPatchRequest,
        call_id: &str,
    ) -> GuardianApprovalRequest {
        GuardianApprovalRequest::ApplyPatch {
            id: call_id.to_string(),
            cwd: req.action.cwd.clone(),
            files: req.file_paths.clone(),
            change_count: req.changes.len(),
            patch: req.action.patch.clone(),
        }
    }

    fn build_command_spec(req: &ApplyPatchRequest) -> Result<CommandSpec, ToolError> {
        use std::env;
        let exe = if let Some(path) = &req.codex_exe {
            path.clone()
        } else {
            env::current_exe()
                .map_err(|e| ToolError::Rejected(format!("failed to determine codex exe: {e}")))?
        };
        let program = exe.to_string_lossy().to_string();
        Ok(CommandSpec {
            program,
            args: vec![
                CODEX_CORE_APPLY_PATCH_ARG1.to_string(),
                req.action.patch.clone(),
            ],
            cwd: req.action.cwd.clone(),
            expiration: req.timeout_ms.into(),
            // Run apply_patch with a minimal environment for determinism and to avoid leaks.
            env: HashMap::new(),
            sandbox_permissions: SandboxPermissions::UseDefault,
            additional_permissions: None,
            justification: None,
        })
    }

    fn stdout_stream(ctx: &ToolCtx) -> Option<crate::exec::StdoutStream> {
        Some(crate::exec::StdoutStream {
            sub_id: ctx.turn.sub_id.clone(),
            call_id: ctx.call_id.clone(),
            tx_event: ctx.session.get_tx_event(),
        })
    }
}

impl Sandboxable for ApplyPatchRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
    fn escalate_on_failure(&self) -> bool {
        true
    }
}

impl Approvable<ApplyPatchRequest> for ApplyPatchRuntime {
    type ApprovalKey = AbsolutePathBuf;

    fn approval_keys(&self, req: &ApplyPatchRequest) -> Vec<Self::ApprovalKey> {
        req.file_paths.clone()
    }

    fn start_approval_async<'a>(
        &'a mut self,
        req: &'a ApplyPatchRequest,
        ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision> {
        let session = ctx.session;
        let turn = ctx.turn;
        let call_id = ctx.call_id.to_string();
        let retry_reason = ctx.retry_reason.clone();
        let approval_keys = self.approval_keys(req);
        let changes = req.changes.clone();
        Box::pin(async move {
            let approval_request = Self::build_guardian_review_request(req, ctx.call_id);
            let mut human_reason = retry_reason.clone();
            if let Some(outcome) = review_smart_access_request(
                session,
                turn,
                approval_request.clone(),
                retry_reason.clone(),
            )
            .await
            {
                match outcome {
                    SmartAccessApprovalOutcome::Final(decision) => return decision,
                    SmartAccessApprovalOutcome::FallbackToHuman { rationale } => {
                        human_reason =
                            merge_human_approval_reason(human_reason, rationale.as_str());
                    }
                }
            }
            if routes_approval_to_guardian(turn) {
                return review_approval_request(session, turn, approval_request, retry_reason)
                    .await;
            }
            if retry_reason.is_some() {
                let rx_approve = session
                    .request_patch_approval(turn, call_id, changes.clone(), human_reason, None)
                    .await;
                return rx_approve.await.unwrap_or_default();
            }

            with_cached_approval(
                &session.services,
                "apply_patch",
                approval_keys,
                || async move {
                    let rx_approve = session
                        .request_patch_approval(turn, call_id, changes, human_reason, None)
                        .await;
                    rx_approve.await.unwrap_or_default()
                },
            )
            .await
        })
    }

    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::Never => false,
            AskForApproval::Reject(reject_config) => !reject_config.rejects_sandbox_approval(),
            AskForApproval::OnFailure => true,
            AskForApproval::OnRequest => true,
            AskForApproval::UnlessTrusted => true,
        }
    }

    // apply_patch approvals are decided upstream by assess_patch_safety.
    //
    // This override ensures the orchestrator runs the patch approval flow when required instead
    // of falling back to the global exec approval policy.
    fn exec_approval_requirement(
        &self,
        req: &ApplyPatchRequest,
    ) -> Option<ExecApprovalRequirement> {
        Some(req.exec_approval_requirement.clone())
    }
}

impl ToolRuntime<ApplyPatchRequest, ExecToolCallOutput> for ApplyPatchRuntime {
    async fn run(
        &mut self,
        req: &ApplyPatchRequest,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx,
    ) -> Result<ExecToolCallOutput, ToolError> {
        let daemon_log_offset = endpoint_security_daemon_log_size();
        let guardian_request = Self::build_guardian_review_request(req, &ctx.call_id);
        let spec = match Self::build_command_spec(req) {
            Ok(spec) => spec,
            Err(err) => {
                if is_smart_access_mode(ctx.turn.as_ref()) {
                    clear_runtime_context(ctx.session.as_ref(), &ctx.call_id).await;
                }
                return Err(err);
            }
        };
        let env = match attempt.env_for(spec, None) {
            Ok(env) => env,
            Err(err) => {
                if is_smart_access_mode(ctx.turn.as_ref()) {
                    clear_runtime_context(ctx.session.as_ref(), &ctx.call_id).await;
                }
                return Err(ToolError::Codex(err.into()));
            }
        };
        let mut out = match execute_env(env, Self::stdout_stream(ctx)).await {
            Ok(out) => out,
            Err(err) => {
                if is_smart_access_mode(ctx.turn.as_ref()) {
                    clear_runtime_context(ctx.session.as_ref(), &ctx.call_id).await;
                }
                return Err(ToolError::Codex(err));
            }
        };
        maybe_record_endpoint_security_runtime_mismatch(
            ctx.session.as_ref(),
            ctx.turn.as_ref(),
            &ctx.call_id,
            &format!("{}:smart-access-runtime", ctx.call_id),
            guardian_assessment_action_value(&guardian_request),
            &mut out,
            daemon_log_offset,
        )
        .await;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::RejectConfig;

    #[test]
    fn wants_no_sandbox_approval_reject_respects_sandbox_flag() {
        let runtime = ApplyPatchRuntime::new();
        assert!(runtime.wants_no_sandbox_approval(AskForApproval::OnRequest));
        assert!(
            !runtime.wants_no_sandbox_approval(AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: false,
                mcp_elicitations: false,
            }))
        );
        assert!(
            runtime.wants_no_sandbox_approval(AskForApproval::Reject(RejectConfig {
                sandbox_approval: false,
                rules: false,
                mcp_elicitations: false,
            }))
        );
    }
}
