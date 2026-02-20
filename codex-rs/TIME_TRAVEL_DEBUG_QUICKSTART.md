# 时间旅行调试系统 - 快速启动指南

## 🚀 5分钟快速开始

### 前置条件检查

```bash
# 1. 检查 UTM 是否安装
ls /Applications/UTM.app

# 2. 检查 NixOS 镜像
ls /Users/jqwang/00-nixos-config/nixos-image/*.iso

# 3. 检查 Rust 工具链
rustc --version
cargo --version

# 4. 检查 SSH 密钥
ls ~/.ssh/id_ed25519
```

### 第一步：创建模板 VM（15分钟）

```bash
cd /Users/jqwang/00-nixos-config/nixos-config

# 创建 VM
make utm/create NIXNAME=vm-aarch64-utm-template

# 启动并配置（这会自动运行 bootstrap）
make utm/bootstrap-all NIXNAME=vm-aarch64-utm-template

# 停止模板 VM
make utm/stop NIXNAME=vm-aarch64-utm-template

# 验证模板 VM 已创建
scripts/utm-vmctl.sh list | grep template
```

### 第二步：编译 Codex（10分钟）

```bash
cd /Users/jqwang/01-agent/new-codex/codex-rs

# 添加 automation 模块到 lib.rs
cat >> core/src/lib.rs << 'EOF'

pub mod automation;
EOF

# 编译
cargo build --release -p codex-cli

# 验证编译成功
./target/release/codex --version
```

### 第三步：配置时间旅行调试（5分钟）

```bash
# 创建配置目录
mkdir -p ~/.codex

# 创建配置文件
cat > ~/.codex/config.toml << 'EOF'
[features]
time_travel_debug = true

[time_travel]
enabled = true
snapshot_dir = ".time-travel-snapshots"
vm_timeout_secs = 120
fix_timeout_secs = 300
preserve_fix_vm_on_failure = true
EOF

# 创建快照目录
mkdir -p .time-travel-snapshots
```

### 第四步：测试系统（10分钟）

```bash
# 创建测试项目
mkdir -p /tmp/test-project
cd /tmp/test-project
cargo init --name test-app

# 创建有编译错误的代码
cat > src/main.rs << 'EOF'
fn main() {
    let x: i32 = "hello";  // 类型错误
    println!("{}", x);
}
EOF

# 运行 Codex（会自动触发时间旅行调试）
cd /Users/jqwang/01-agent/new-codex/codex-rs
./target/release/codex exec "Fix the type error in /tmp/test-project"

# 验证修复
cd /tmp/test-project
cargo check
```

---

## 🔍 系统架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                   主工作区 (Main)                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ Cargo.toml + src/main.rs + flake.nix               │  │
│  │ ↓                                                     │  │
│  │ cargo check → ❌ Error                               │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                         ↓
        ┌─────────────────────────────────────┐
        │  时间定格 (Freeze)                  │
        ├─────────────────────────────────────┤
        │ 1. 创建快照 (1s)                    │
        │ 2. 克隆 VM (10s)                    │
        │ 3. 启动 VM (15s)                    │
        └─────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────────┐
│                  隔离修复环境 (Fix-VM)                       │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 完全相同的环境 (99% 一致)                            │  │
│  │ ├─ Rust 版本: 相同                                   │  │
│  │ ├─ 依赖版本: 相同 (flake.lock)                       │  │
│  │ ├─ 源代码: 相同                                      │  │
│  │ └─ 编译错误: 相同                                    │  │
│  │                                                      │  │
│  │ AI 修复循环：                                        │  │
│  │ 1. 分析错误 (5s)                                     │  │
│  │ 2. 应用修复 (10s)                                    │  │
│  │ 3. cargo check → ✅ Success (5s)                     │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                         ↓
        ┌─────────────────────────────────────┐
        │  Undo & Replace (替换)              │
        ├─────────────────────────────────────┤
        │ 1. 复制修复文件 (2s)                │
        │ 2. 验证编译 (3s)                    │
        │ 3. 销毁 Fix-VM (2s)                 │
        │ 4. 清理快照 (1s)                    │
        └─────────────────────────────────────┘
                         ↓
┌─────────────────────────────────────────────────────────────┐
│                    主工作区 (Main)                           │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 修复后的代码                                         │  │
│  │ cargo check → ✅ Success                             │  │
│  │ 继续下一个 Turn                                      │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 📊 核心组件说明

### 1. CompileErrorFreezer（编译错误冻结器）

**职责**：检测编译错误并创建快照

```rust
pub struct CompileErrorFreezer {
    snapshot_dir: PathBuf,
    utm_manager: Arc<UTMManager>,
    workspace_root: PathBuf,
}

// 主要方法
- detect_compile_errors()      // 检测编译错误
- freeze_on_error()            // 创建快照并克隆 VM
- create_fix_vm()              // 创建隔离 VM
- save_snapshot()              // 保存快照到磁盘
```

**工作流程**：
```
cargo check 失败
    ↓
解析编译错误信息
    ↓
捕获环境信息 (Rust版本、flake.lock等)
    ↓
获取 git commit hash
    ↓
创建快照 JSON
    ↓
使用 utm-vmctl.sh 克隆 VM
    ↓
返回 FreezeSnapshot
```

### 2. UTMManager（UTM 虚拟机管理器）

**职责**：管理 UTM 虚拟机的生命周期

```rust
pub struct UTMManager {
    scripts_dir: PathBuf,
    workspace_root: PathBuf,
}

// 主要方法
- list_vms()                   // 列出所有 VM
- vm_exists()                  // 检查 VM 是否存在
- get_vm_ip()                  // 获取 VM IP
- wait_for_vm_ready()          // 等待 VM 启动
- exec_in_vm()                 // 在 VM 中执行命令
- copy_to_vm()                 // 复制文件到 VM
- copy_from_vm()               // 从 VM 复制文件
- stop_vm()                    // 停止 VM
- delete_vm()                  // 删除 VM
```

**工作流程**：
```
使用 osascript 与 UTM 通信
    ↓
通过 SSH 连接到 VM
    ↓
使用 rsync 传输文件
    ↓
执行远程命令
    ↓
返回结果
```

### 3. FixAgentCoordinator（修复 Agent 协调器）

**职责**：在隔离环境中运行修复 Agent

```rust
pub struct FixAgentCoordinator {
    utm_manager: Arc<UTMManager>,
    workspace_root: PathBuf,
}

// 主要方法
- run_fix_in_vm()              // 在 VM 中运行修复
- restore_workspace()          // 恢复工作区
- generate_fix_prompt()        // 生成修复 prompt
- run_fix_agent_in_vm()        // 运行修复 Agent
```

**工作流程**：
```
等待 VM 启动
    ↓
获取 VM IP
    ↓
恢复工作区 (git checkout + 复制文件)
    ↓
验证编译错误存在
    ↓
生成修复 prompt
    ↓
在 VM 中运行 codex exec
    ↓
验证编译成功
    ↓
返回 FixResult
```

### 4. UndoReplacer（Undo 替换器）

**职责**：将修复应用到主工作区

```rust
pub struct UndoReplacer {
    utm_manager: Arc<UTMManager>,
    main_workspace: PathBuf,
}

// 主要方法
- apply_fix_and_undo()         // 应用修复并替换
- copy_fixed_files()           // 复制修复文件
- verify_compile()             // 验证编译
- cleanup_snapshot()           // 清理快照
- restore_from_snapshot()      // 恢复到快照
```

**工作流程**：
```
从 Fix-VM 复制修复文件
    ↓
使用 rsync 同步到主工作区
    ↓
验证编译成功
    ↓
销毁 Fix-VM
    ↓
清理快照文件
    ↓
完成
```

### 5. TimeTravelMiddleware（时间旅行中间件）

**职责**：集成所有组件，自动触发修复流程

```rust
pub struct TimeTravelMiddleware {
    freezer: Arc<CompileErrorFreezer>,
    coordinator: Arc<FixAgentCoordinator>,
    replacer: Arc<UndoReplacer>,
    enabled: bool,
}

// 实现 HarnessMiddleware trait
#[async_trait]
impl HarnessMiddleware for TimeTravelMiddleware {
    async fn after_turn(
        &self,
        ctx: &Arc<TurnContext>,
        last_message: Option<String>,
    ) -> CodexResult<Option<String>> {
        // 自动检测编译错误并修复
    }
}
```

---

## 🔧 配置详解

### 环境变量

```bash
# 启用/禁用时间旅行调试
export CODEX_TIME_TRAVEL_DEBUG=1

# 快照目录（默认：.time-travel-snapshots）
export CODEX_SNAPSHOT_DIR=/path/to/snapshots

# VM 超时时间（秒，默认：120）
export CODEX_VM_TIMEOUT=180

# 修复超时时间（秒，默认：300）
export CODEX_FIX_TIMEOUT=600

# 日志级别
export RUST_LOG=codex_core::automation=debug
```

### 配置文件 (~/.codex/config.toml)

```toml
[features]
# 启用时间旅行调试
time_travel_debug = true

[time_travel]
# 是否启用
enabled = true

# 快照目录
snapshot_dir = ".time-travel-snapshots"

# VM 超时时间（秒）
vm_timeout_secs = 120

# 修复超时时间（秒）
fix_timeout_secs = 300

# 修复失败时是否保留 Fix-VM 供调试
preserve_fix_vm_on_failure = true

# 是否自动清理旧快照
auto_cleanup_old_snapshots = true

# 保留快照的天数
snapshot_retention_days = 7

# 最大并发 Fix-VM 数量
max_concurrent_fix_vms = 3
```

---

## 📈 监控和调试

### 查看快照

```bash
# 列出所有快照
ls -lh .time-travel-snapshots/

# 查看快照详情
cat .time-travel-snapshots/<snapshot-id>.json | jq .

# 查看特定快照的错误
cat .time-travel-snapshots/<snapshot-id>.json | jq '.error'

# 查看快照的 git 信息
cat .time-travel-snapshots/<snapshot-id>.json | jq '.git_commit, .git_branch'
```

### 查看 VM 状态

```bash
# 列出所有 VM
scripts/utm-vmctl.sh list

# 查看特定 VM 的 IP
osascript -e 'tell application "UTM" to query ip virtual machine named "vm-fix-xxx"'

# SSH 连接到 Fix-VM
ssh jqwang@<vm-ip>

# 查看 VM 中的工作区
ssh jqwang@<vm-ip> "ls -la /workspace"

# 查看修复日志
ssh jqwang@<vm-ip> "cat /tmp/fix-agent-*.log"
```

### 查看修复过程

```bash
# 启用详细日志
export RUST_LOG=codex_core::automation=debug

# 运行 Codex
codex

# 查看日志输出
# [DEBUG] Freeze snapshot created: snapshot-abc123
# [DEBUG] VM created: vm-fix-abc123
# [DEBUG] Waiting for VM to be ready...
# [DEBUG] VM is ready at 192.168.64.10
# [DEBUG] Restoring workspace...
# [DEBUG] Running fix agent...
# [DEBUG] Fix completed successfully
# [DEBUG] Copying fixed files...
# [DEBUG] Destroying Fix-VM...
```

---

## 🐛 故障排查

### 问题 1：VM 克隆失败

```bash
# 检查 UTM 是否运行
ps aux | grep UTM

# 检查模板 VM 是否存在
scripts/utm-vmctl.sh list | grep template

# 手动创建 VM 测试
scripts/utm-vmctl.sh create --template vm-aarch64-utm-template --start

# 查看 UTM 日志
log stream --predicate 'process == "UTM"' --level debug
```

### 问题 2：SSH 连接失败

```bash
# 检查 SSH 密钥
ls -la ~/.ssh/id_ed25519

# 测试 SSH 连接
ssh -v jqwang@<vm-ip>

# 检查 VM 中的 SSH 服务
ssh jqwang@<vm-ip> "sudo systemctl status sshd"

# 重启 SSH 服务
ssh jqwang@<vm-ip> "sudo systemctl restart sshd"
```

### 问题 3：修复失败

```bash
# 查看修复日志
ssh jqwang@<vm-ip> "cat /tmp/fix-agent-*.log"

# 手动验证编译
ssh jqwang@<vm-ip> "cd /workspace && cargo check"

# 查看修改的文件
ssh jqwang@<vm-ip> "cd /workspace && git diff"

# 查看编译错误
ssh jqwang@<vm-ip> "cd /workspace && cargo check 2>&1 | head -50"
```

### 问题 4：VM 无法启动

```bash
# 检查 NixOS 配置
cat /Users/jqwang/00-nixos-config/nixos-config/machines/vm-aarch64-utm.nix

# 检查 VM 配置
osascript -e 'tell application "UTM" to return configuration of virtual machine named "vm-fix-xxx"'

# 查看 VM 启动日志
# 在 UTM UI 中查看 VM 的控制台输出

# 重新创建 VM
scripts/utm-vmctl.sh delete vm-fix-xxx
scripts/utm-vmctl.sh create --template vm-aarch64-utm-template --start
```

---

## 🎯 最佳实践

### 1. 定期维护模板 VM

```bash
# 每周更新一次模板 VM
cd /Users/jqwang/00-nixos-config/nixos-config

# 启动模板 VM
make utm/start NIXNAME=vm-aarch64-utm-template

# 更新 NixOS
ssh jqwang@<template-ip> "sudo nixos-rebuild switch --flake ."

# 停止模板 VM
make utm/stop NIXNAME=vm-aarch64-utm-template
```

### 2. 监控快照大小

```bash
# 定期清理旧快照
find .time-travel-snapshots -mtime +7 -delete

# 查看快照总大小
du -sh .time-travel-snapshots/

# 设置自动清理
# 在 crontab 中添加
0 2 * * * cd /path/to/project && find .time-travel-snapshots -mtime +7 -delete
```

### 3. 性能优化

```bash
# 预热模板 VM 缓存
make utm/start NIXNAME=vm-aarch64-utm-template
sleep 30
make utm/stop NIXNAME=vm-aarch64-utm-template

# 使用 Nix 缓存加速包下载
# 在 vm-shared.nix 中已配置

# 并行化 VM 操作
# 在 FixAgentCoordinator 中实现
```

### 4. 安全性

```bash
# 定期更新 SSH 密钥
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519 -N ""

# 限制 VM 网络访问
# 在 vm-fix-template.nix 中配置防火墙

# 加密快照文件
# 实现快照加密功能
```

---

## 📚 相关文档

- **架构设计**: `TIME_TRAVEL_DEBUG_ARCHITECTURE.md`
- **集成指南**: `TIME_TRAVEL_DEBUG_INTEGRATION.md`
- **完整清单**: `TIME_TRAVEL_DEBUG_CHECKLIST.md`
- **NixOS 配置**: `/Users/jqwang/00-nixos-config/nixos-config/README-UTM.md`
- **Codex 文档**: `/Users/jqwang/01-agent/new-codex/codex-rs/README.md`

---

## 🚀 下一步

1. **完成基础集成** - 修复编译错误，运行测试
2. **性能优化** - 减少 VM 启动时间
3. **错误恢复** - 实现自动重试机制
4. **监控仪表板** - 创建 Web UI
5. **文档完善** - 添加更多示例

---

**最后更新**: 2026-02-20
**维护者**: jqwang
**状态**: 活跃开发中
