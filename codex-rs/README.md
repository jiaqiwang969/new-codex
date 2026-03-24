  这个项目是基于 OpenAI Codex CLI 的深度定制 fork，在上游基础上扩展了 7 大核心能力（含 Entire 过程控制）：

  ---
  1. 多模型 Provider 支持

  - OpenAI (GPT-5.3-codex-spark|[pro], GPT-5.3-codex, GPT-5.2-codex)
  - Anthropic Claude (Claude Opus 4.6, Claude Sonnet 4.6 — 原生 Messages API，1M context，支持 vision/图片输入、extended thinking)
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

  7. Entire 过程控制

  - 通过 Codex notify hook 自动捕获 AI 会话快照
  - 会话元数据存储在 `entire/checkpoints/v1` orphan 分支，不污染代码历史
  - git commit 时通过 `Entire-Checkpoint` trailer 双向关联代码与 AI 会话
  - 支持 rewind（回退检查点）、resume（恢复会话）、explain（解释上下文）

### Entire 过程控制集成

通过 Codex 的 `notify` hook 集成 [Entire CLI](https://github.com/jiaqiwang969/cli)，在每次 agent turn 完成时自动捕获 AI 会话快照，存到 git 的隐藏分支里，不污染代码历史。本质上就是给 AI 辅助编程加了个"录屏"——录的不是屏幕，而是对话 + 代码变更。

**安装 Entire CLI：**

```bash
curl -fsSL https://entire.io/install.sh | bash
```

**配置 Codex notify hook（`~/.codex/config.toml`）：**

```toml
notify = ["entire", "hooks", "codex", "notify"]
```

**在项目中启用：**

```bash
cd your-project && entire enable --agent codex
entire status   # 确认显示 "Enabled (manual-commit)"
```

**工作原理：**

1. 你用 Codex 让 AI 改代码，每次 agent turn 完成后，Codex 通过 notify hook 把对话信息（prompt、AI 回复、model_slug、thread_id）发给 Entire
2. Entire 把当前工作区的文件变更 + 对话记录打包成一个 commit，存到 `entire/<hash>` shadow 分支上
3. 会话元数据（`prompt.txt`、`summary.txt`、`context.md`）保存在 `entire/checkpoints/v1` orphan 分支
4. git commit 时通过 `Entire-Checkpoint` trailer 双向关联代码变更与 AI 会话

**常用命令：**

```bash
entire status                # 查看当前状态
entire explain <commit>      # 解释某次 commit 的 AI 会话上下文
entire rewind                # 回退到之前的检查点
entire resume <branch>       # 切换分支并恢复 AI 会话
entire doctor                # 修复卡住的 session
```

**在 git graph 中查看：** Entire 的 checkpoint 会以独立分支线出现在 `git log --all --graph` 和 TUI 的 Ctrl+G 中，例如：

```
* 5ab4a56 (HEAD -> main) docs: add Entire mention
| * 3409e82 (entire/6f4cb91-e3b0c4) Test entire integration
| * 4909e3b (entire/checkpoints/v1) Initialize metadata branch
```

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

# ── Anthropic provider pool ───────────────────────────────────────
[model_providers.anthropic]
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY_POOL_1"

[[model_providers.anthropic.account_pool]]
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY_POOL_1"

[[model_providers.anthropic.account_pool]]
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY_POOL_2"

[[model_providers.anthropic.account_pool]]
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY_POOL_3"
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
  "XAI_API_KEY_POOL_3": "xai-your-third-grok-key",
  "ANTHROPIC_API_KEY_POOL_1": "sk-ant-your-first-anthropic-key",
  "ANTHROPIC_API_KEY_POOL_2": "sk-ant-your-second-anthropic-key",
  "ANTHROPIC_API_KEY_POOL_3": "sk-ant-your-third-anthropic-key"
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

### 审批配置（推荐）

上游方向是把“何时审批”和“谁审批”分开配置，而不是继续使用旧的本地特殊审批流。

- `approval_policy`：定义什么时候需要审批
- `approvals_reviewer`：定义谁来审核审批请求（`user` 或 `guardian_subagent`）
- `smart_approvals`：只是 rollout / UI 开关，不会替代 `approval_policy`

详细说明见 [`docs/config.md`](../docs/config.md#approvals-reviewer)。

### `~/.codex/config.toml` 的 `[features]` 配置说明

> 如果你说的 `~/.config.toml` 指的是 Codex 用户配置，实际文件路径是 `~/.codex/config.toml`。

你可以在 `config.toml` 里通过 `[features]` 开关覆盖默认行为；未配置的项会继续使用默认值。

```toml
[features]
# Stable (off by default)
undo = true                          # Ghost commit at each turn for undo

# Experimental
multi_agent = true
agent_worktrees = true
apps = true
prevent_idle_sleep = true            # Keep macOS awake while running

# Under Development
sqlite = true
memory_tool = true
codex_git_commit = true
js_repl = true
child_agents_md = true
skill_env_var_dependency_prompt = true
responses_websockets = true
responses_websockets_v2 = true
```

补充说明：
- `js_repl_tools_only` 依赖 `js_repl = true`，否则会被自动关闭。
- `web_search_request` / `web_search_cached` 已废弃，建议用顶层 `web_search = "live" | "cached" | "disabled"`。
- 下面的 key 均为 canonical key（即推荐写法）。

| feature key | 默认值 | 状态 | 作用 |
| --- | --- | --- | --- |
| `undo` | `true` ⚠️ | Stable | 每轮生成 ghost commit，支持撤销型工作流。 |
| `shell_tool` | `true` | Stable | 启用默认 shell 工具。 |
| `unified_exec` | 非 Windows `true`；Windows `false` | Stable | 使用统一的 PTY exec 工具路径。 |
| `shell_snapshot` | `true` | Stable | 记录 shell 输出快照，供后续上下文使用。 |
| `enable_request_compression` | `true` | Stable | 发送流式请求时启用 zstd 压缩。 |
| `skill_mcp_dependency_install` | `true` | Stable | 允许提示并安装缺失的 skill MCP 依赖。 |
| `steer` | `true` | Stable | 启用 steer 行为（Enter 立即提交而非排队）。 |
| `collaboration_modes` | `true` | Stable | 启用协作模式（Default / Plan）。 |
| `personality` | `true` | Stable | 启用 TUI personality 选择。 |
| `powershell_utf8` | Windows `true`；非 Windows `false` | Windows: Stable；其他: UnderDevelopment | 强制 PowerShell 使用 UTF-8 输出。 |
| `js_repl` | `true` ⚠️ | UnderDevelopment | 启用基于持久 Node 内核的 `js_repl` 工具。 |
| `js_repl_tools_only` | `false` | UnderDevelopment | 仅暴露 `js_repl` 工具给模型。 |
| `codex_git_commit` | `true` ⚠️ | UnderDevelopment | 在模型指令中启用 git commit 归因提示。 |
| `runtime_metrics` | `false` | UnderDevelopment | 启用 runtime metrics 快照采集。 |
| `sqlite` | `true` ⚠️ | UnderDevelopment | 将 rollout 元数据持久化到本地 SQLite。 |
| `memory_tool` | `true` ⚠️ | UnderDevelopment | 启用记忆提取与跨会话记忆归并能力。 |
| `child_agents_md` | `true` ⚠️ | UnderDevelopment | 将额外 AGENTS.md 指令附加到子 agent 指令中。 |
| `apply_patch_freeform` | `false` | UnderDevelopment | 启用 freeform `apply_patch` 工具。 |
| `apps_mcp_gateway` | `false` | UnderDevelopment | 让 Apps MCP 调用走 gateway。 |
| `skill_env_var_dependency_prompt` | `true` ⚠️ | UnderDevelopment | 提示缺失的 skill 环境变量依赖。 |
| `responses_websockets` | `true` ⚠️ | UnderDevelopment | 默认通过 Responses WebSocket 传输。 |
| `responses_websockets_v2` | `true` ⚠️ | UnderDevelopment | 启用 Responses WebSocket v2 模式。 |
| `multi_agent` | `true` ⚠️ | Experimental | 启用多 agent 协作工具（如 `spawn_agent`）。 |
| `agent_worktrees` | `true` ⚠️ | Experimental | 为 fork/子 agent 使用隔离 git worktree。 |
| `apps` | `true` ⚠️ | Experimental | 启用 Apps/Connectors（`$` 提及）。 |
| `use_linux_sandbox_bwrap` | `false` | Linux: Experimental；其他: UnderDevelopment | Linux 下启用 bubblewrap 沙箱链路。 |
| `prevent_idle_sleep` | `true` ⚠️ | macOS: Experimental；其他: UnderDevelopment | 任务运行期间阻止系统闲置睡眠。 |
| `web_search_request` | `false` | Deprecated | 旧版在线搜索开关（已废弃）。 |
| `web_search_cached` | `false` | Deprecated | 旧版缓存搜索开关（已废弃）。 |
| `search_tool` | `false` | Removed | 旧版搜索工具标志（已移除）。 |
| `request_rule` | `false` | Removed | 旧审批规则请求开关（已移除）。 |
| `experimental_windows_sandbox` | `false` | Removed | 旧 Windows sandbox 开关（已移除）。 |
| `elevated_windows_sandbox` | `false` | Removed | 旧 elevated Windows sandbox 开关（已移除）。 |
| `remote_models` | `false` | Removed | 旧远程模型开关（已移除）。 |

**注意**：标记 ⚠️ 的 feature 在你的配置中已启用（`true`），但上游默认值仍为 `false`。这是本 fork 的定制配置。

兼容老配置时，以下 legacy key 仍可被识别，但建议迁移到上表 key：
- `connectors` -> `apps`
- `experimental_use_unified_exec_tool` -> `unified_exec`
- `experimental_use_freeform_apply_patch` / `include_apply_patch_tool` -> `apply_patch_freeform`
- `collab` -> `multi_agent`
- `web_search` -> `web_search_request`（该路径本身也已废弃）
- `enable_experimental_windows_sandbox` -> `experimental_windows_sandbox`（已移除）

### `~/.codex/config.toml` 的 utility model 配置（`model_sub` / `model_sub_responses`）

为了让 Claude / Gemini 等非-Responses provider 作为 leader 时，memory 等内部能力不会降级，Codex 提供了两条 “utility model” 配置：

注意：如果不设置（unset），Codex 会对不同的内部任务使用各自的内置默认模型；设置这些字段仅表示“覆盖（override）”内部任务的默认选择。

```toml
# 通用内部任务使用的 utility model（memory phase-1/2 等）
# 同时用于 spawn_agent 的默认角色与 explorer 角色（未显式传 model 覆盖时）
model_sub = "claude-sonnet-4-6"

# 仅用于 Responses-only 内部任务的 utility model（例如 memory trace summarize）
# 注意：必须是 OpenAI slug（gpt-* / o1-* / o3-* / o4-*，或带 openai/ 前缀），否则会被忽略并给出启动 warning。
model_sub_responses = "gpt-5.1-codex-mini"
```

在 TUI 中也可以通过 slash commands 交互设置：
- `/team-profile`（同时设置 leader model + `model_sub` + `model_sub_responses` + memories phase models）
- `/model-sub`
- `/model-sub auto|recommended|auto:general|auto:debug|auto:review`（按 model vouch 自动选择 utility/sub-agent model）
- `/model-sub-responses`
- `/team-profile auto`（直接应用当前 vouch 评分最高的推荐 profile）
- `/team-profile auto:general|auto:debug|auto:review`（按任务桶应用推荐 profile）
- `/team-vouch <win|loss> [general|debug|review] [note]`（手动记录当前 team profile 的功/过）
- `/team-vouch duel <winner> <loser> [general|debug|review] [note]`（记录同题对比的胜负）
- `/team-vouch model <win|loss> <model> [general|debug|review] [note]`（记录某个 utility/sub-agent model 的功/过）
- `/team-vouch model-duel <winner_model> <loser_model> [general|debug|review] [note]`（记录 utility/sub-agent model 的同题对比胜负）

`/team-profile` 会尝试读取 `~/.codex/memories/team_profile_vouch.json`，在弹窗中显示每个 profile 的
`vouch`（功/过统计）与备注，便于后续沉淀为自动路由依据。
若已有功过数据，`/team-profile` 会优先按 recent-weighted signal（最近样本加权）动态标记 `Recommended`，并回退到累计净胜负（`net = wins - losses`）。
当你提交 `/feedback` 时，Codex 会把 `Good result` 记为该 profile 的一次 `win`，把
`Bad result / Bug / Safety check` 记为一次 `loss`（仅在当前路由命中某个 `/team-profile` preset 时生效）。
其中 `Bug` 会记入 `debug` 桶，`Safety check` 会记入 `review` 桶，其它反馈进入 `general` 桶。
也可以用 `/team-vouch` 直接记录 leader 评价（例如 `/team-vouch win debug fixed flaky parser`）。
或用 `/team-vouch duel ...` 记录同一任务在不同 profile/agent 路由下的胜负结果。
对于 utility/sub-agent 模型本身，可用 `/team-vouch model ...` 与 `/team-vouch model-duel ...`
记录功过，再用 `/model-sub auto[:bucket]` 让系统按功过自动选择。
当 `model_sub` 未显式配置时，`spawn_agent` 的 `default/explorer` 角色会尝试读取
`~/.codex/memories/model_sub_vouch.json` 自动选择推荐模型，并在当前 session 内缓存该选择。
若当前 session 既没有 `model_sub` 显式值、也没有缓存/功过推荐，`spawn_agent` 会触发一次同题校准（`calibrate_model_sub`）并在返回中附带 `auto_calibration` 信息（候选 run 摘要 + `recommended_for_session` / `recommended_for_latency`）。
建议 leader 在查看候选输出后调用 `record_model_sub_winner`（或 `record_model_sub_duel`）落盘功过，这样同会话后续协作可直接复用最优小弟。
可手工调用：
- `calibrate_model_sub`（同题跑多模型）
- `record_model_sub_winner`（一次记录 winner vs 所有候选；可省略 `winner_model` 与 `compared_models`，自动复用本 session 最近一次 `calibrate_model_sub` 的推荐 winner 与候选集）
- `record_model_sub_duel`（单对单记分）
`/status` 会在有功过数据时展示 `Team profile auto`，给出 `general/debug/review` 的推荐 profile。

## Code Organization

This folder is the root of a Cargo workspace. It contains quite a bit of experimental code, but here are the key crates:

- [`core/`](./core) contains the business logic for Codex. Ultimately, we hope this to be a library crate that is generally useful for building other Rust/native applications that use Codex.
- [`exec/`](./exec) "headless" CLI for use in automation.
- [`tui/`](./tui) CLI that launches a fullscreen TUI built with [Ratatui](https://ratatui.rs/).
- [`cli/`](./cli) CLI multitool that provides the aforementioned CLIs via subcommands.

If you want to contribute or inspect behavior in detail, start by reading the module-level `README.md` files under each crate and run the project workspace from the top-level `codex-rs` directory so shared config, features, and build scripts stay aligned.
