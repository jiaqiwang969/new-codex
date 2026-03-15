# Antigravity (CLIProxyAPI) 集成指南

本文档介绍如何在 codex-rs 中使用 CLIProxyAPI 的 Antigravity 代理模型。

## 概述

Antigravity 集成允许你通过 CLIProxyAPI 使用 Gemini 和 Claude 模型，完整保留原生 API 的所有功能，包括：

- **Gemini 模型**：完整的 `thoughtSignature` 支持，保持多轮对话的推理上下文
- **Claude 模型**：原生 Anthropic Messages API，支持 thinking 模式

## 支持的模型

### Gemini 系列（通过 Gemini 原生 API）

| 模型 ID | 显示名称 | 描述 |
|---------|---------|------|
| `antigravity/gemini-3.1-pro-high` | Antigravity Gemini 3.1 Pro High | 最强推理能力，1M 上下文 |
| `antigravity/gemini-3-pro-high` | Antigravity Gemini 3 Pro High | 深度推理，1M 上下文 |
| `antigravity/gemini-3-flash` | Antigravity Gemini 3 Flash | 快速高效，1M 上下文 |
| `antigravity/gemini-2.5-flash` | Antigravity Gemini 2.5 Flash | 超快速，1M 上下文 |
| `antigravity/gemini-3-pro-image` | Antigravity Gemini 3 Pro Image | 支持图像生成 |

### Claude 系列（通过 Anthropic 原生 API）

| 模型 ID | 显示名称 | 描述 |
|---------|---------|------|
| `antigravity/claude-sonnet-4-6` | Antigravity Claude Sonnet 4.6 | 快速执行，1M 上下文 |
| `antigravity/claude-opus-4-6-thinking` | Antigravity Claude Opus 4.6 Thinking | 深度推理，扩展 thinking |

## 配置步骤

### 1. 启动 CLIProxyAPI 服务

确保 CLIProxyAPI 服务正在运行：

```bash
cd /Users/jqwang/05-api-代理/CLIProxyAPI
./cli-proxy-api -config config.yaml
```

服务默认监听：
- Gemini 原生端点：`http://localhost:8317/v1beta`
- Anthropic 原生端点：`http://localhost:8317`

### 2. 配置环境变量

设置 API Key：

```bash
export ANTIGRAVITY_API_KEY="sk-ea5f4b43076ab87b461eb711e218b43dc972c21b8254df74"
```

或者将其添加到 `~/.bashrc` 或 `~/.zshrc`：

```bash
echo 'export ANTIGRAVITY_API_KEY="sk-ea5f4b43076ab87b461eb711e218b43dc972c21b8254df74"' >> ~/.bashrc
source ~/.bashrc
```

### 3. 配置 Account Pool

先在 `~/.codex/config.toml` 里定义逻辑 provider：

```toml
[model_providers.antigravity-gemini]
name = "Antigravity Gemini"
base_url = "http://localhost:8317/v1beta"
wire_api = "gemini"

[model_providers.antigravity-anthropic]
name = "Antigravity Anthropic"
base_url = "http://localhost:8317"
wire_api = "anthropic"
```

再在 `~/.codex/config-pool.toml` 里定义账户池：

```toml
[[model_providers.antigravity-gemini.account_pool]]
base_url = "http://localhost:8317/v1beta"
env_key = "ANTIGRAVITY_API_KEY_POOL_1"

[[model_providers.antigravity-anthropic.account_pool]]
base_url = "http://localhost:8317"
env_key = "ANTIGRAVITY_API_KEY_POOL_1"
```

### 4. （可选）配置多账号池

如果你有多个 CLIProxyAPI API Key，可以配置账号池实现故障转移。`config-pool.toml`
里只保留 `account_pool` 条目，不要再额外写顶层 `env_key` 作为默认账号：

```toml
[[model_providers.antigravity-gemini.account_pool]]
base_url = "http://localhost:8317/v1beta"
env_key = "ANTIGRAVITY_API_KEY_POOL_1"

[[model_providers.antigravity-gemini.account_pool]]
base_url = "http://localhost:8317/v1beta"
env_key = "ANTIGRAVITY_API_KEY_POOL_2"

[[model_providers.antigravity-gemini.account_pool]]
base_url = "http://localhost:8317/v1beta"
env_key = "ANTIGRAVITY_API_KEY_POOL_3"
```

然后设置对应的环境变量：

```bash
export ANTIGRAVITY_API_KEY_POOL_1="sk-key1..."
export ANTIGRAVITY_API_KEY_POOL_2="sk-key2..."
export ANTIGRAVITY_API_KEY_POOL_3="sk-key3..."
```

## 使用方法

### 基本用法

```bash
# 使用 Gemini 模型
codex exec -c 'model_provider="antigravity-gemini"' \
  --model "antigravity/gemini-3.1-pro-high" \
  "Your prompt here"

# 使用 Claude 模型
codex exec -c 'model_provider="antigravity-anthropic"' \
  --model "antigravity/claude-sonnet-4-6" \
  "Your prompt here"
```

### 交互式会话

```bash
# 启动 Gemini 交互式会话
codex -c 'model_provider="antigravity-gemini"' \
  --model "antigravity/gemini-3.1-pro-high"

# 启动 Claude 交互式会话
codex -c 'model_provider="antigravity-anthropic"' \
  --model "antigravity/claude-sonnet-4-6"
```

### 设置默认模型

编辑 `~/.codex/config.toml`：

```toml
model = "antigravity/gemini-3.1-pro-high"
model_provider = "antigravity-gemini"
```

然后直接使用：

```bash
codex "Your prompt here"
```

### 调整推理强度

```bash
# 使用高强度推理
codex exec -c 'model_provider="antigravity-gemini"' \
  -c 'model_reasoning_effort="high"' \
  --model "antigravity/gemini-3.1-pro-high" \
  "Complex problem here"

# 使用低强度推理（更快）
codex exec -c 'model_provider="antigravity-gemini"' \
  -c 'model_reasoning_effort="low"' \
  --model "antigravity/gemini-3-flash" \
  "Simple task here"
```

## 技术细节

### thoughtSignature 机制

Gemini 模型使用 `thoughtSignature` 来保持多轮对话中的推理上下文：

1. **请求侧**：codex-rs 自动为 function call 和 inline image 注入 `thoughtSignature`
2. **响应侧**：从 Gemini 响应中提取 `thoughtSignature` 并关联到 function call
3. **缓存机制**：CLIProxyAPI 缓存 `thoughtSignature`（TTL 3 小时），用于多轮对话

### 协议对齐

- **Gemini 模型**：使用 Gemini 原生 JSON API (`WireApi::Gemini`)
  - 端点：`/v1beta/models/{model}:streamGenerateContent`
  - 完整保留 `thoughtSignature`、`<thinking>` 标签等原生功能

- **Claude 模型**：使用 Anthropic Messages API (`WireApi::Anthropic`)
  - 端点：`/v1/messages`
  - 支持原生 thinking 模式和 extended thinking

### 模型名称映射

codex-rs 会自动去除 `antigravity/` 前缀，然后发送给 CLIProxyAPI：

```
antigravity/gemini-3.1-pro-high  →  gemini-3.1-pro-high
antigravity/claude-sonnet-4-6    →  claude-sonnet-4-6
```

## 故障排查

### 问题：连接被拒绝

```
ERROR: Connection refused
```

**解决方案**：
1. 检查 CLIProxyAPI 是否正在运行：`ps aux | grep cli-proxy-api`
2. 检查端口是否正确：`curl http://localhost:8317/health`
3. 检查防火墙设置

### 问题：API Key 无效

```
ERROR: 401 Unauthorized
```

**解决方案**：
1. 检查环境变量：`echo $ANTIGRAVITY_API_KEY`
2. 验证 API Key 是否正确
3. 检查 CLIProxyAPI 的 `config.yaml` 中的 `api-keys` 配置

### 问题：模型不可用

```
ERROR: unknown provider for model antigravity/gemini-3.1-pro-high
```

**解决方案**：
1. 检查模型名称是否正确
2. 确认 CLIProxyAPI 支持该模型：`curl http://localhost:8317/v1/models`
3. 检查 `config-pool.toml` 中的 `wire_api` 配置

### 问题：推理内容丢失

如果你发现推理内容（thinking）没有显示：

1. **确认使用原生端点**：检查 `base_url` 是否为 `/v1beta`（Gemini）或 `/v1`（Claude）
2. **检查 wire_api 配置**：确保设置为 `"gemini"` 或 `"anthropic"`
3. **查看日志**：检查 CLIProxyAPI 的日志输出

## 性能优化

### 1. 使用本地缓存

CLIProxyAPI 自动缓存 `thoughtSignature`，无需额外配置。

### 2. 选择合适的模型

- **快速任务**：使用 `gemini-3-flash` 或 `gemini-2.5-flash`
- **复杂推理**：使用 `gemini-3.1-pro-high` 或 `claude-opus-4-6-thinking`
- **平衡性能**：使用 `gemini-3-pro-high` 或 `claude-sonnet-4-6`

### 3. 调整推理强度

```bash
# 快速响应（低推理强度）
-c 'model_reasoning_effort="low"'

# 平衡（中等推理强度）
-c 'model_reasoning_effort="medium"'

# 深度推理（高推理强度）
-c 'model_reasoning_effort="high"'
```

## 与其他 Provider 的对比

| 特性 | Antigravity Gemini | 原生 Gemini | Antigravity Claude | 原生 Claude |
|------|-------------------|-------------|-------------------|-------------|
| thoughtSignature | ✅ 完整支持 | ✅ 完整支持 | N/A | N/A |
| Thinking 模式 | ✅ 完整支持 | ✅ 完整支持 | ✅ 完整支持 | ✅ 完整支持 |
| 多轮对话上下文 | ✅ 自动保持 | ✅ 自动保持 | ✅ 自动保持 | ✅ 自动保持 |
| 账号池故障转移 | ✅ 支持 | ❌ 不支持 | ✅ 支持 | ❌ 不支持 |
| 本地部署 | ✅ 支持 | ❌ 云端 | ✅ 支持 | ❌ 云端 |
| API Key 管理 | ✅ 统一管理 | ❌ 分散管理 | ✅ 统一管理 | ❌ 分散管理 |

## 示例

### 示例 1：代码审查

```bash
codex exec -c 'model_provider="antigravity-gemini"' \
  --model "antigravity/gemini-3.1-pro-high" \
  "Review the code in src/main.rs and suggest improvements"
```

### 示例 2：复杂问题求解

```bash
codex exec -c 'model_provider="antigravity-anthropic"' \
  -c 'model_reasoning_effort="xhigh"' \
  --model "antigravity/claude-opus-4-6-thinking" \
  "Design a distributed system architecture for handling 1M requests/second"
```

### 示例 3：快速代码生成

```bash
codex exec -c 'model_provider="antigravity-gemini"' \
  -c 'model_reasoning_effort="low"' \
  --model "antigravity/gemini-3-flash" \
  "Write a function to calculate fibonacci numbers"
```

## 更多信息

- CLIProxyAPI 文档：https://github.com/router-for-me/CLIProxyAPI
- codex-rs 文档：https://github.com/openai/codex
- Gemini API 文档：https://ai.google.dev/docs
- Anthropic API 文档：https://docs.anthropic.com

## 贡献

如果你发现问题或有改进建议，请提交 Issue 或 Pull Request。
