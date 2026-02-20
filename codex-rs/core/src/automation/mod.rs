//! 时间旅行调试系统 (Time Travel Debug System)
//!
//! 当编译出错时，自动：
//! 1. 时间定格 - 创建快照
//! 2. Clone 隔离环境 - 使用 Nix + UTM
//! 3. 修复错误 - 在隔离环境中运行修复 Agent
//! 4. Undo 替换 - 将修复应用到主工作区

pub mod compile_error_freezer;
pub mod fix_agent_coordinator;
pub mod undo_replacer;
pub mod snapshot;
pub mod utm_manager;

pub use compile_error_freezer::CompileErrorFreezer;
pub use fix_agent_coordinator::FixAgentCoordinator;
pub use undo_replacer::UndoReplacer;
pub use snapshot::{FreezeSnapshot, FixVM};
pub use utm_manager::UTMManager;

use async_trait::async_trait;
use std::sync::Arc;
use crate::codex::TurnContext;
use crate::harness::HarnessMiddleware;
use crate::protocol::EventMsg;
use crate::error::Result as CodexResult;
use codex_protocol::user_input::UserInput;

/// 时间旅行调试中间件
pub struct TimeTravelMiddleware {
    freezer: Arc<CompileErrorFreezer>,
    coordinator: Arc<FixAgentCoordinator>,
    replacer: Arc<UndoReplacer>,
    enabled: bool,
}

impl TimeTravelMiddleware {
    pub fn new(
        freezer: Arc<CompileErrorFreezer>,
        coordinator: Arc<FixAgentCoordinator>,
        replacer: Arc<UndoReplacer>,
    ) -> Self {
        Self {
            freezer,
            coordinator,
            replacer,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[async_trait]
impl HarnessMiddleware for TimeTravelMiddleware {
    async fn after_turn(
        &self,
        ctx: &Arc<TurnContext>,
        last_message: Option<String>,
    ) -> CodexResult<Option<String>> {
        if !self.enabled {
            return Ok(last_message);
        }

        // 检查编译错误
        match self.freezer.detect_compile_errors(&ctx.cwd).await {
            Ok(compile_result) if !compile_result.success => {
                tracing::info!("Compile error detected, initiating time travel debug");

                // 时间定格
                match self.freezer.freeze_on_error(ctx, &compile_result).await {
                    Ok(snapshot) => {
                        tracing::info!("Snapshot created: {}", snapshot.id);

                        // 在隔离环境中修复
                        match self.coordinator.run_fix_in_vm(&snapshot.fix_vm, &snapshot).await {
                            Ok(fix_result) if fix_result.success => {
                                tracing::info!("Fix successful, applying undo replacement");

                                // Undo 替换
                                if let Err(e) = self.replacer
                                    .apply_fix_and_undo(&snapshot.fix_vm, &snapshot)
                                    .await
                                {
                                    tracing::error!("Failed to apply fix: {}", e);
                                }
                            }
                            Ok(fix_result) => {
                                tracing::warn!("Fix failed: {:?}", fix_result.error);
                                // 保留 Fix-VM 供调试
                            }
                            Err(e) => {
                                tracing::error!("Fix agent error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to create snapshot: {}", e);
                    }
                }
            }
            Ok(_) => {
                // 编译成功，继续
            }
            Err(e) => {
                tracing::warn!("Failed to check compile: {}", e);
            }
        }

        Ok(last_message)
    }
}
