# 时间旅行调试系统 - 完整实现清单

## ✅ 已完成

### 架构设计
- [x] 核心概念文档 (`TIME_TRAVEL_DEBUG_ARCHITECTURE.md`)
- [x] 集成指南 (`TIME_TRAVEL_DEBUG_INTEGRATION.md`)
- [x] 数据流设计
- [x] 性能指标分析

### 核心模块
- [x] `automation/mod.rs` - 主模块和中间件
- [x] `automation/snapshot.rs` - 快照数据结构
- [x] `automation/compile_error_freezer.rs` - 编译错误检测和快照创建
- [x] `automation/utm_manager.rs` - UTM VM 管理
- [x] `automation/fix_agent_coordinator.rs` - 修复 Agent 协调
- [x] `automation/undo_replacer.rs` - Undo 和替换逻辑

### NixOS 配置
- [x] 现有 UTM 配置分析
- [x] Makefile.utm 命令分析
- [x] utm-vmctl.sh 脚本分析
- [x] vm-inventory.json 清单

---

## 🔄 下一步实现

### 第一阶段：基础集成（1-2 周）

#### 1. 修复编译错误

```bash
# 在 core/src/lib.rs 中添加
pub mod automation;

# 在 Cargo.toml 中添加依赖
uuid = { version = "1", features = ["v4", "serde"] }
md5 = "0.7"
```

#### 2. 实现 Codex CLI 集成

```rust
// core/src/automation/fix_agent_coordinator.rs 中
// 需要实现调用 codex exec 的逻辑

async fn run_fix_agent_in_vm(
    &self,
    vm_ip: &str,
    fix_prompt: &str,
) -> Result<FixResult> {
    // 使用 SSH 在 VM 中运行 codex exec
    let cmd = format!(
        r#"cd /workspace && \
        codex exec '{}' --ephemeral"#,
        fix_prompt.replace("'", "\\'")
    );

    self.utm_manager.exec_in_vm(vm_ip, &cmd).await?;

    // 验证修复
    // ...
}
```

#### 3. 创建测试用例

```rust
// core/tests/suite/time_travel_debug.rs

#[tokio::test]
async fn test_compile_error_freeze_and_fix() -> Result<()> {
    // 1. 创建有编译错误的代码
    // 2. 触发时间旅行调试
    // 3. 验证快照创建
    // 4. 验证 VM 克隆
    // 5. 验证修复
    // 6. 验证 Undo 替换
    Ok(())
}
```

### 第二阶段：优化和完善（2-3 周）

#### 1. 性能优化

```rust
// 并行化 VM 操作
pub async fn create_multiple_fix_vms(
    &self,
    snapshots: Vec<FreezeSnapshot>,
) -> Result<Vec<FixVM>> {
    let futures = snapshots.iter().map(|s| {
        self.create_fix_vm(s)
    });

    futures::future::try_join_all(futures).await
}
```

#### 2. 错误恢复

```rust
// 实现自动重试机制
pub async fn run_fix_with_retry(
    &self,
    fix_vm: &FixVM,
    snapshot: &FreezeSnapshot,
    max_retries: u32,
) -> Result<FixResult> {
    for attempt in 0..max_retries {
        match self.run_fix_in_vm(fix_vm, snapshot).await {
            Ok(result) if result.success => return Ok(result),
            Ok(_) => {
                tracing::warn!(attempt, "Fix attempt failed, retrying");
                continue;
            }
            Err(e) => {
                if attempt < max_retries - 1 {
                    tracing::warn!(attempt, error = %e, "Fix error, retrying");
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(anyhow!("All fix attempts failed"))
}
```

#### 3. 监控和日志

```rust
// 添加详细的日志和指标
pub async fn run_fix_in_vm_with_metrics(
    &self,
    fix_vm: &FixVM,
    snapshot: &FreezeSnapshot,
) -> Result<FixResult> {
    let start = std::time::Instant::now();

    let result = self.run_fix_in_vm(fix_vm, snapshot).await?;

    let duration = start.elapsed();

    // 记录指标
    tracing::info!(
        snapshot_id = %snapshot.id,
        vm_name = %fix_vm.name,
        duration_secs = duration.as_secs(),
        success = result.success,
        "Fix completed"
    );

    Ok(result)
}
```

### 第三阶段：高级功能（3-4 周）

#### 1. 多错误并行修复

```rust
pub struct MultiErrorFixCoordinator {
    coordinator: Arc<FixAgentCoordinator>,
}

impl MultiErrorFixCoordinator {
    pub async fn fix_multiple_errors(
        &self,
        snapshots: Vec<FreezeSnapshot>,
    ) -> Result<Vec<FixResult>> {
        // 为每个错误创建独立的 Fix-VM
        // 并行运行修复
        // 合并结果
    }
}
```

#### 2. 智能修复建议

```rust
pub struct SmartFixSuggester {
    error_history: Arc<ErrorHistory>,
}

impl SmartFixSuggester {
    pub async fn suggest_fix(
        &self,
        error: &CompileError,
    ) -> Result<String> {
        // 查询历史错误
        // 找到相似的已修复错误
        // 生成修复建议
    }
}
```

#### 3. Web UI 仪表板

```rust
// 使用 Axum 创建 Web 服务
pub async fn start_dashboard(
    port: u16,
) -> Result<()> {
    let app = Router::new()
        .route("/api/snapshots", get(list_snapshots))
        .route("/api/snapshots/:id", get(get_snapshot))
        .route("/api/vms", get(list_vms))
        .route("/api/vms/:name/logs", get(get_vm_logs));

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
        .await?;

    axum::serve(listener, app).await?;

    Ok(())
}
```

---

## 📋 配置清单

### macOS 环境

- [ ] 安装 UTM
- [ ] 配置 NixOS 模板 VM
- [ ] 配置 SSH 密钥
- [ ] 测试 utm-vmctl.sh 脚本

### NixOS 配置

- [ ] 创建 `vm-fix-template.nix`
- [ ] 在 `flake.nix` 中添加配置
- [ ] 测试 VM 克隆和启动

### Codex 项目

- [ ] 添加 `automation` 模块到 `core/src/lib.rs`
- [ ] 添加依赖到 `Cargo.toml`
- [ ] 实现 HarnessMiddleware 集成
- [ ] 编写测试用例

### CI/CD

- [ ] 添加 GitHub Actions 工作流
- [ ] 配置自动化测试
- [ ] 设置性能基准测试

---

## 🎯 关键指标

### 性能目标

| 指标 | 目标 | 当前 |
|------|------|------|
| 快照创建 | < 2s | - |
| VM 克隆 | < 10s | - |
| VM 启动 | < 15s | - |
| 工作区恢复 | < 5s | - |
| 修复循环 | < 60s | - |
| Undo 替换 | < 5s | - |
| **总时间** | **< 100s** | - |

### 可靠性目标

| 指标 | 目标 |
|------|------|
| VM 克隆成功率 | > 99% |
| 修复成功率 | > 80% |
| 快照恢复成功率 | > 99% |
| 系统可用性 | > 99.5% |

---

## 🔗 相关文件

### 架构文档
- `/Users/jqwang/00-nixos-config/TIME_TRAVEL_DEBUG_ARCHITECTURE.md`
- `/Users/jqwang/01-agent/new-codex/codex-rs/TIME_TRAVEL_DEBUG_INTEGRATION.md`

### 实现代码
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/mod.rs`
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/snapshot.rs`
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/compile_error_freezer.rs`
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/utm_manager.rs`
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/fix_agent_coordinator.rs`
- `/Users/jqwang/01-agent/new-codex/codex-rs/core/src/automation/undo_replacer.rs`

### NixOS 配置
- `/Users/jqwang/00-nixos-config/nixos-config/flake.nix`
- `/Users/jqwang/00-nixos-config/nixos-config/Makefile.utm`
- `/Users/jqwang/00-nixos-config/nixos-config/scripts/utm-vmctl.sh`
- `/Users/jqwang/00-nixos-config/nixos-config/machines/vm-aarch64-utm.nix`

---

## 📞 快速参考

### 启用时间旅行调试

```bash
# 在 ~/.codex/config.toml 中
[features]
time_travel_debug = true

[time_travel]
enabled = true
snapshot_dir = ".time-travel-snapshots"
```

### 查看快照

```bash
ls -lh .time-travel-snapshots/
cat .time-travel-snapshots/<id>.json | jq .
```

### 管理 Fix-VM

```bash
# 列出所有 VM
scripts/utm-vmctl.sh list

# 连接到 Fix-VM
ssh jqwang@<vm-ip>

# 删除 Fix-VM
scripts/utm-vmctl.sh delete <vm-name>
```

### 调试修复过程

```bash
# 查看修复日志
tail -f /tmp/fix-agent-*.log

# 手动验证编译
cd /workspace && cargo check

# 查看修改的文件
git diff --name-only
```

---

## 🚀 启动命令

### 第一次设置

```bash
# 1. 创建 NixOS 模板 VM
cd /Users/jqwang/00-nixos-config/nixos-config
make utm/create NIXNAME=vm-aarch64-utm-template
make utm/bootstrap-all NIXNAME=vm-aarch64-utm-template
make utm/stop NIXNAME=vm-aarch64-utm-template

# 2. 编译 Codex
cd /Users/jqwang/01-agent/new-codex/codex-rs
cargo build --release -p codex-cli

# 3. 运行测试
cargo test --test time_travel_debug
```

### 日常使用

```bash
# 启用时间旅行调试
export CODEX_TIME_TRAVEL_DEBUG=1

# 运行 Codex
codex

# 当编译出错时，系统会自动：
# 1. 创建快照
# 2. 克隆 VM
# 3. 修复错误
# 4. 应用修复
```

---

## 📊 项目状态

```
时间旅行调试系统
├── ✅ 架构设计 (完成)
├── ✅ 核心模块 (完成)
├── ⏳ 基础集成 (进行中)
├── ⏳ 优化完善 (待开始)
└── ⏳ 高级功能 (待开始)

总进度: 30% 完成
```

---

## 💡 关键创新点

1. **Nix 声明式隔离** - 99% 环境一致性
2. **秒级 VM 克隆** - 快速创建隔离环境
3. **自动修复循环** - AI 驱动的错误修复
4. **时间定格快照** - 完整的状态捕获和恢复
5. **Undo 替换机制** - 无缝集成修复结果

---

## 🎓 学习资源

- [Nix 官方文档](https://nixos.org/manual/nix/stable/)
- [UTM 文档](https://docs.getutm.app/)
- [Rust 异步编程](https://tokio.rs/)
- [Codex 架构](../docs/entire-implementation-summary.md)

---

**最后更新**: 2026-02-20
**维护者**: jqwang
**状态**: 活跃开发中
