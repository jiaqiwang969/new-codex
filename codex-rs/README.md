  这个项目是基于 OpenAI Codex CLI 的深度定制 fork，在上游基础上扩展了 6 大核心能力：

  ---
  1. 多模型 Provider 支持

  - OpenAI (GPT-5.3-codex-spark|[pro], GPT-5.3-codex, GPT-5.2-codex)
  - Google Gemini (Gemini 3 Pro/Flash, Gemini 3 Pro Image, 1M context)
  - Grok (Grok 4, Grok 4.1 Fast Reasoning)
  - 本地模型 (Gemma-3n via Ollama/LM Studio)
  - 运行时自动切换 + Account Pool 故障转移 (400/401/403/429 自动换号，支持多轮循环)
  - 模型兼容矩阵：按模型自动启用/禁用 web search、reasoning effort、image 等能力

  2. 跨会话记忆系统

  - SQLite 持久化的 Thread Memory（对话摘要 + trace）
  - Context Packet 注入：自动将记忆、用户指令、项目 memories 注入 MCP 调用和子 agent
  - 会话压缩后自动更新记忆，最小 10 分钟间隔防抖

  3. 多 Agent 协作

  - spawn_agent / send_input / resume_agent / wait / close_agent 完整生命周期
  - Agent Worktrees：每个子 agent 在隔离的 git worktree 中工作
  - Lease 持久化：.codex/leases/ 目录，支持 worktree 恢复
  - Context Packet 自动注入子 agent 启动 prompt

  4. 图像处理流水线

  - /ref-image：设置参考图片
  - /ref-image-batch：批量处理文件夹中的图片
  - /image-quality (1K/2K/4K) + /aspect-ratio (1:1/16:9/9:16/4:3/3:4)
  - /pdf-update：PDF 水印去除 + 批量图片处理
  - Gemini Image 模型专用配置（response_modalities、image_config）

  5. TUI 增强

  - Git Graph (Ctrl+G)：内嵌 git 提交历史可视化，Unicode 圆角风格
  - Session Bar (Ctrl+P)：tmux 风格底部面板，会话导航/新建/删除/重命名
  - 后台预热：2s 空闲后自动加载 session 列表和 git graph，打开时即时显示
  - Ralph Loop (/ralph-loop)：迭代自纠正循环，支持 <promise> 完成检测

  6. 基础设施

  - Gemma 系统 prompt 截断（29K→4K，防止本地小模型空输出）
  - Thought Signature 跨模型泄漏防护（Gemini→GPT 切换时自动清理）
  - WebSocket preconnect 跨 turn 复用
  - MCP 后台初始化（不阻塞启动）
  - Debug CLI：thread-memory backfill、agent-worktrees list/restore

### Entire 过程控制集成

通过 Codex 的 `notify` hook 集成 [Entire CLI](https://github.com/jiaqiwang969/cli)，在每次 agent turn 完成时自动捕获 AI 会话快照，与 git commit 关联，形成可追溯的开发过程记录。

`~/.codex/config.toml` 中添加：

```toml
notify = ["entire", "hooks", "codex", "notify"]
```

在项目中启用：

```bash
# 安装 Entire CLI
brew tap entireio/tap && brew install entireio/tap/entire

# 在项目中启用（默认 manual-commit 策略）
cd your-project && entire enable --agent codex

# 查看状态
entire status
```

Entire 会在后台自动工作：
- 每次 agent turn 完成后，通过 notify hook 接收 JSON payload（含 thread_id、prompts、model_slug 等）
- 将会话元数据（transcript、prompt、context）保存到 `entire/checkpoints/v1` 分支
- git commit 时通过 `Entire-Checkpoint` trailer 关联代码变更与 AI 会话
- 支持 `entire rewind` 回退到任意检查点，`entire resume` 恢复会话

### API Account Pool (多账户故障转移)

Account Pool 系统支持为每个 provider 配置多个 API 账户，当某个账户遇到认证失败 (400/401/403) 或限流 (429) 时，自动切换到下一个账户。支持多轮循环（默认 2 轮），所有账户都失败后才报错退出。

配置文件与主配置隔离，存放在 `~/.codex/` 目录下：

`~/.codex/config-pool.toml` — Pool 账户配置：

```toml
# ── OpenAI-compatible provider pool ─────────────────────────────────
[model_providers.codex]
base_url = "https://your-openai-proxy.example.com/v1"
env_key = "OPENAI_API_KEY_POOL_1"

[[model_providers.codex.account_pool]]
base_url = "https://your-openai-proxy.example.com/v1"
env_key = "OPENAI_API_KEY_POOL_1"

[[model_providers.codex.account_pool]]
base_url = "https://your-openai-proxy.example.com/v1"
env_key = "OPENAI_API_KEY_POOL_2"

[[model_providers.codex.account_pool]]
base_url = "https://your-openai-proxy.example.com/v1"
env_key = "OPENAI_API_KEY_POOL_3"

# ── Gemini provider pool ───────────────────────────────────────────
[model_providers.gemini]
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GEMINI_API_KEY_POOL_1"

[[model_providers.gemini.account_pool]]
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GEMINI_API_KEY_POOL_1"

[[model_providers.gemini.account_pool]]
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GEMINI_API_KEY_POOL_2"

[[model_providers.gemini.account_pool]]
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GEMINI_API_KEY_POOL_3"

# ── Grok provider pool ─────────────────────────────────────────────
[model_providers.grok]
base_url = "https://api.x.ai/v1"
env_key = "XAI_API_KEY_POOL_1"

[[model_providers.grok.account_pool]]
base_url = "https://api.x.ai/v1"
env_key = "XAI_API_KEY_POOL_1"

[[model_providers.grok.account_pool]]
base_url = "https://api.x.ai/v1"
env_key = "XAI_API_KEY_POOL_2"

[[model_providers.grok.account_pool]]
base_url = "https://api.x.ai/v1"
env_key = "XAI_API_KEY_POOL_3"
```

`~/.codex/auth-pool.json` — Pool API 密钥（与主 `auth.json` 隔离）：

```json
{
  "OPENAI_API_KEY_POOL_1": "sk-your-first-openai-key",
  "OPENAI_API_KEY_POOL_2": "sk-your-second-openai-key",
  "OPENAI_API_KEY_POOL_3": "sk-your-third-openai-key",
  "GEMINI_API_KEY_POOL_1": "AIza-your-first-gemini-key",
  "GEMINI_API_KEY_POOL_2": "AIza-your-second-gemini-key",
  "GEMINI_API_KEY_POOL_3": "AIza-your-third-gemini-key",
  "XAI_API_KEY_POOL_1": "xai-your-first-grok-key",
  "XAI_API_KEY_POOL_2": "xai-your-second-grok-key",
  "XAI_API_KEY_POOL_3": "xai-your-third-grok-key"
}
```

行为说明：
- 每个 `account_pool` 条目可以有不同的 `base_url` 和 `env_key`，支持跨代理/跨区域分布
- 认证失败 (400/401/403) 立即切换，不等待重试
- 限流 (429) 也立即切换
- 可重试错误 (5xx) 先重试到上限，再切换账户
- 默认循环 2 轮（3 账户 × 2 轮 = 最多 6 次切换），全部失败后报错退出
- 成功的账户会持久化到 `config-pool.toml`，下次启动直接使用

#### MCP client

Codex CLI functions as an MCP client that allows the Codex CLI and IDE extension to connect to MCP servers on startup. See the [`configuration documentation`](../docs/config.md#connecting-to-mcp-servers) for details.

##### 预置 MCP Servers

项目预置了两个 MCP server，通过 npx 加载，在 `~/.codex/config.toml` 中配置：

```toml
[mcp_servers.watermark-remover]
command = "npx"
args = ["-y", "github:jiaqiwang969/watermark-removal-mcp"]
startup_timeout_sec = 60

[mcp_servers.claude-code-mcp]
command = "npx"
args = ["-y", "@steipete/claude-code-mcp@latest"]
```

- [watermark-removal-mcp](https://github.com/jiaqiwang969/watermark-removal-mcp)：PDF/图片水印去除工具，需要 Python 3.10+ 和 Poppler（`brew install poppler`）
- [claude-code-mcp](https://github.com/jiaqiwang969/claude-code-mcp)：Claude Code one-shot 执行代理，需要先安装 Claude CLI（`npm install -g @anthropic-ai/claude-code`）

#### MCP server (experimental)

Codex can be launched as an MCP _server_ by running `codex mcp-server`. This allows _other_ MCP clients to use Codex as a tool for another agent.

Use the [`@modelcontextprotocol/inspector`](https://github.com/modelcontextprotocol/inspector) to try it out:

```shell
npx @modelcontextprotocol/inspector codex mcp-server
```

Use `codex mcp` to add/list/get/remove MCP server launchers defined in `config.toml`, and `codex mcp-server` to run the MCP server directly.

### Notifications

You can enable notifications by configuring a script that is run whenever the agent finishes a turn. The [notify documentation](../docs/config.md#notify) includes a detailed example that explains how to get desktop notifications via [terminal-notifier](https://github.com/julienXX/terminal-notifier) on macOS. When Codex detects that it is running under WSL 2 inside Windows Terminal (`WT_SESSION` is set), the TUI automatically falls back to native Windows toast notifications so approval prompts and completed turns surface even though Windows Terminal does not implement OSC 9.

### `codex exec` to run Codex programmatically/non-interactively

To run Codex non-interactively, run `codex exec PROMPT` (you can also pass the prompt via `stdin`) and Codex will work on your task until it decides that it is done and exits. Output is printed to the terminal directly. You can set the `RUST_LOG` environment variable to see more about what's going on.
Use `codex exec --ephemeral ...` to run without persisting session rollout files to disk.

### Experimenting with the Codex Sandbox

To test to see what happens when a command is run under the sandbox provided by Codex, we provide the following subcommands in Codex CLI:

```
# macOS
codex sandbox macos [--full-auto] [--log-denials] [COMMAND]...

# Linux
codex sandbox linux [--full-auto] [COMMAND]...

# Windows
codex sandbox windows [--full-auto] [COMMAND]...

# Legacy aliases
codex debug seatbelt [--full-auto] [--log-denials] [COMMAND]...
codex debug landlock [--full-auto] [COMMAND]...
```

### Selecting a sandbox policy via `--sandbox`

The Rust CLI exposes a dedicated `--sandbox` (`-s`) flag that lets you pick the sandbox policy **without** having to reach for the generic `-c/--config` option:

```shell
# Run Codex with the default, read-only sandbox
codex --sandbox read-only

# Allow the agent to write within the current workspace while still blocking network access
codex --sandbox workspace-write

# Danger! Disable sandboxing entirely (only do this if you are already running in a container or other isolated env)
codex --sandbox danger-full-access
```

The same setting can be persisted in `~/.codex/config.toml` via the top-level `sandbox_mode = "MODE"` key, e.g. `sandbox_mode = "workspace-write"`.

## Code Organization

This folder is the root of a Cargo workspace. It contains quite a bit of experimental code, but here are the key crates:

- [`core/`](./core) contains the business logic for Codex. Ultimately, we hope this to be a library crate that is generally useful for building other Rust/native applications that use Codex.
- [`exec/`](./exec) "headless" CLI for use in automation.
- [`tui/`](./tui) CLI that launches a fullscreen TUI built with [Ratatui](https://ratatui.rs/).
- [`cli/`](./cli) CLI multitool that provides the aforementioned CLIs via subcommands.

If you want to contribute or inspect behavior in detail, start by reading the module-level `README.md` files under each crate and run the project workspace from the top-level `codex-rs` directory so shared config, features, and build scripts stay aligned.
