# 时间旅行调试系统 - 集成指南

## 🎯 快速集成步骤

### 1. 添加到 `core/src/lib.rs`

```rust
pub mod automation;
```

### 2. 在 Cargo.toml 中添加依赖

```toml
[dependencies]
uuid = { version = "1", features = ["v4", "serde"] }
md5 = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tracing = "0.1"
```

### 3. 初始化中间件

```rust
// 在 codex 初始化时
use std::sync::Arc;
use codex_core::automation::{
    TimeTravelMiddleware,
    CompileErrorFreezer,
    FixAgentCoordinator,
    UndoReplacer,
    UTMManager,
};

let utm_manager = Arc::new(UTMManager::new(
    PathBuf::from("scripts"),
    PathBuf::from("."),
));

let freezer = Arc::new(CompileErrorFreezer::new(
    PathBuf::from(".time-travel-snapshots"),
    utm_manager.clone(),
    PathBuf::from("."),
));

let coordinator = Arc::new(FixAgentCoordinator::new(
    utm_manager.clone(),
    PathBuf::from("."),
));

let replacer = Arc::new(UndoReplacer::new(
    utm_manager.clone(),
    PathBuf::from("."),
));

let time_travel_middleware = TimeTravelMiddleware::new(
    freezer,
    coordinator,
    replacer,
).with_enabled(true);

// 注册到 harness
harness.register_middleware(Box::new(time_travel_middleware));
```

### 4. 配置 NixOS VM 模板

在 `/Users/jqwang/00-nixos-config/nixos-config/machines/` 中创建：

```nix
# machines/vm-fix-template.nix
{ config, pkgs, lib, ... }: {
  imports = [
    ./vm-aarch64-utm.nix
  ];

  # 最小化配置以加快启动
  services.xserver.enable = false;
  services.displayManager.sddm.enable = false;

  # 只保留必要的开发工具
  environment.systemPackages = with pkgs; [
    cargo
    rustc
    git
    openssh
    rsync
  ];

  # 禁用不必要的服务
  services.tailscale.enable = false;
  services.flatpak.enable = false;
  services.snap.enable = false;

  # 快速启动
  boot.kernelParams = [ "quiet" ];
  systemd.services.getty@tty1.enable = false;
}
```

### 5. 创建 UTM 模板 VM

```bash
# 在 macOS 上
cd /Users/jqwang/00-nixos-config/nixos-config

# 创建模板 VM
make utm/create NIXNAME=vm-aarch64-utm-template

# 启动并配置
make utm/bootstrap-all NIXNAME=vm-aarch64-utm-template

# 停止模板 VM
make utm/stop NIXNAME=vm-aarch64-utm-template
```

---

## 📊 工作流程

### 正常情况（编译成功）

```
Turn N
  ↓
cargo check → ✅ Success
  ↓
继续 Turn N+1
```

### 异常情况（编译失败）

```
Turn N
  ↓
cargo check → ❌ Error
  ↓
[时间旅行调试启动]
  ├─ 1. 时间定格 (1s)
  │   └─ 创建快照 + 克隆 VM
  │
  ├─ 2. 隔离修复 (30-60s)
  │   ├─ VM 启动 (15s)
  │   ├─ 恢复工作区 (5s)
  │   └─ AI 修复 (10-40s)
  │
  └─ 3. Undo 替换 (5s)
      ├─ 复制修复文件
      ├─ 验证编译
      ├─ 销毁 Fix-VM
      └─ 清理快照
  ↓
继续 Turn N+1（代码已修复）
```

---

## 🔧 配置选项

### 环境变量

```bash
# 启用/禁用时间旅行调试
export CODEX_TIME_TRAVEL_DEBUG=1

# 快照目录
export CODEX_SNAPSHOT_DIR=.time-travel-snapshots

# VM 超时时间（秒）
export CODEX_VM_TIMEOUT=120

# 修复超时时间（秒）
export CODEX_FIX_TIMEOUT=300
```

### 配置文件 (`~/.codex/config.toml`)

```toml
[features]
time_travel_debug = true

[time_travel]
enabled = true
snapshot_dir = ".time-travel-snapshots"
vm_timeout_secs = 120
fix_timeout_secs = 300
preserve_fix_vm_on_failure = true  # 失败时保留 VM 供调试
```

---

## 🐛 调试和故障排查

### 查看快照

```bash
# 列出所有快照
ls -lh .time-travel-snapshots/

# 查看快照内容
cat .time-travel-snapshots/<snapshot-id>.json | jq .
```

### 查看 Fix-VM 日志

```bash
# 连接到 Fix-VM
ssh jqwang@<fix-vm-ip>

# 查看修复日志
cat /tmp/fix-agent-*.log

# 手动验证编译
cd /workspace && cargo check
```

### 恢复到快照

```bash
# 使用 UndoReplacer 恢复
make time-travel/restore SNAPSHOT_ID=<id>

# 或手动恢复
git checkout <commit-hash>
```

### 清理失败的 Fix-VM

```bash
# 列出所有 VM
scripts/utm-vmctl.sh list

# 删除特定 VM
scripts/utm-vmctl.sh delete vm-fix-<id>

# 清理所有 Fix-VM
scripts/utm-vmctl.sh list | grep "vm-fix-" | awk '{print $1}' | \
  xargs -I {} scripts/utm-vmctl.sh delete {}
```

---

## 📈 性能优化

### 1. 预热模板 VM

```bash
# 定期启动和停止模板 VM 以预热缓存
make utm/start NIXNAME=vm-aarch64-utm-template
sleep 30
make utm/stop NIXNAME=vm-aarch64-utm-template
```

### 2. 使用 Nix 缓存

```bash
# 配置 Attic 缓存以加速 Nix 包下载
# 在 vm-shared.nix 中已配置
```

### 3. 并行化 VM 操作

```rust
// 同时创建多个 Fix-VM（如果有多个编译错误）
let futures = errors.iter().map(|error| {
    self.create_fix_vm_for_error(error)
});

let results = futures::future::join_all(futures).await;
```

---

## 🔐 安全考虑

### 1. SSH 密钥管理

```bash
# 确保 SSH 密钥已配置
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519

# 将公钥添加到 VM 配置
# 在 vm-shared.nix 中配置
```

### 2. 网络隔离

```nix
# 在 vm-fix-template.nix 中
networking.firewall.enable = true;
networking.firewall.allowedTCPPorts = [ 22 ];  # 仅允许 SSH
```

### 3. 快照加密

```rust
// 可选：加密快照文件
use aes_gcm::{Aes256Gcm, Key, Nonce};

fn encrypt_snapshot(snapshot: &FreezeSnapshot, key: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(b"unique nonce");

    let json = serde_json::to_vec(snapshot)?;
    cipher.encrypt(nonce, json.as_ref())
        .map_err(|e| anyhow!("Encryption failed: {}", e))
}
```

---

## 📚 测试

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compile_error_detection() {
        let freezer = CompileErrorFreezer::new(
            PathBuf::from("/tmp"),
            Arc::new(UTMManager::new(PathBuf::from("."), PathBuf::from("."))),
            PathBuf::from("."),
        );

        // 创建有编译错误的代码
        let result = freezer.detect_compile_errors(Path::new(".")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_snapshot_creation() {
        // 测试快照创建
    }

    #[tokio::test]
    async fn test_vm_operations() {
        // 测试 VM 操作
    }
}
```

### 集成测试

```bash
# 运行完整的时间旅行调试流程
cargo test --test time_travel_integration -- --nocapture
```

---

## 🚀 下一步

1. **实现 Codex CLI 集成** - 在 Fix-VM 中调用 `codex exec`
2. **添加 Web UI** - 可视化快照和修复过程
3. **支持多个错误** - 并行修复多个编译错误
4. **性能优化** - 减少 VM 启动时间
5. **文档完善** - 添加更多示例和最佳实践

---

## 📞 支持

如有问题，请查看：
- 快照文件：`.time-travel-snapshots/`
- 日志：`~/.codex/logs/`
- VM 日志：`/tmp/fix-agent-*.log`
