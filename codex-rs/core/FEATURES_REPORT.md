# Codex Features 全功能报告

> 配置文件: `~/.codex/config.toml`
> 生成时间: 2026-02-18

---

## 一、功能总览

### 已开启的功能清单

| 功能 | 配置 Key | 阶段 | 默认 |
|------|----------|------|------|
| Ghost Commit (Undo) | `undo` | Stable | off |
| Multi-Agent | `multi_agent` | Experimental | off |
| Agent Worktrees | `agent_worktrees` | Experimental | off |
| Apps (ChatGPT Connectors) | `apps` | Experimental | off |
| Prevent Idle Sleep | `prevent_idle_sleep` | Experimental | off |
| Memory Tool | `memory_tool` | UnderDevelopment | off |
| Codex Git Commit | `codex_git_commit` | UnderDevelopment | off |
| SQLite Persistence | `sqlite` | UnderDevelopment | off |
| JavaScript REPL | `js_repl` | UnderDevelopment | off |
| Child Agents MD | `child_agents_md` | UnderDevelopment | off |
| Skill Env Var Prompt | `skill_env_var_dependency_prompt` | UnderDevelopment | off |
| WebSocket Transport | `responses_websockets` | UnderDevelopment | off |
| WebSocket V2 | `responses_websockets_v2` | UnderDevelopment | off |

### 默认已开启（无需配置）

| 功能 | 配置 Key | 说明 |
|------|----------|------|
| Shell Tool | `shell_tool` | 默认 shell 执行工具 |
| Unified Exec | `unified_exec` | PTY-backed 统一执行 (非 Windows) |
| Shell Snapshot | `shell_snapshot` | Shell 状态快照 |
| Request Compression | `enable_request_compression` | zstd 请求压缩 |
| Skill MCP Install | `skill_mcp_dependency_install` | 自动安装 MCP 依赖 |
| Steer | `steer` | Enter 直接提交 |
| Collaboration Modes | `collaboration_modes` | Plan/Default 模式切换 |
| Personality | `personality` | TUI 个性化选择 |

---

## 二、核心功能详解

---

### 1. Memory Tool（记忆系统）⭐ 最重要的功能

**配置**: `memory_tool = true`

**用户可见效果**: 对话过程中会出现 "Memory updated" 提示（如截图所示），表示 Codex 正在从当前对话中提取关键信息并持久化保存。

#### 工作原理

记忆系统分为两个阶段：

**Phase 1 — 单次对话记忆提取**
```
用户对话 → 过滤关键信息 → 调用模型提取记忆 → 存入 SQLite
```
- 每次对话结束后，异步扫描对话历史
- 提取：用户偏好、项目结构、常用命令、调试经验、架构决策
- 并发处理最多 8 个线程，每个线程使用 70% 上下文窗口
- 输出 JSON：`raw_memory`（详细记忆）+ `rollout_summary`（摘要）
- 自动脱敏：密钥、token 等敏感信息会被 `[REDACTED_SECRET]` 替换

**Phase 2 — 全局记忆整合**
```
多次对话记忆 → 合并去重 → 生成结构化记忆文件
```
- 全局单例任务，Phase 1 完成后自动触发
- 启动一个子 Agent 来整合所有原始记忆
- 产出三类文件：
  - `MEMORY.md` — 按主题聚类的记忆条目
  - `memory_summary.md` — 用户画像 + 通用建议 + 记忆索引
  - `skills/` — 可复用的操作流程

#### 记忆存储结构
```
~/.codex/memories/
├── memory_summary.md              # 全局记忆摘要（每次对话注入）
├── MEMORY.md                      # 全局记忆手册
├── raw_memories.md                # Phase 1 原始输出
├── rollout_summaries/             # 每次对话的摘要
│   └── <thread_id>-<slug>.md
├── skills/                        # 学到的可复用技能
│   └── <skill-name>/SKILL.md
└── <cwd-bucket>/memory/           # 按项目目录隔离的记忆
    ├── memory_summary.md
    └── MEMORY.md
```

#### 记忆如何影响后续对话

```
新对话启动
  ↓
选择记忆作用域（项目目录 > 用户级 > 全局）
  ↓
加载 memory_summary.md（最多 5000 tokens）
  ↓
注入到 Developer Instructions
  ↓
模型获得历史上下文，无需用户重复说明
```

**实际效果举例**：
- 第一次告诉 Codex "我们项目用 pnpm，不要用 npm" → 记忆保存
- 后续所有对话，Codex 自动使用 pnpm，无需再次提醒
- 记忆跨会话持久化，重启 Codex 后依然有效

#### 记忆作用域优先级
1. **项目级** (`<cwd-bucket>/memory/`) — 最高优先级，按工作目录隔离
2. **用户级** (`user/memory/`) — 适用于所有项目
3. **全局级** (`memories/`) — 兜底

---

### 2. Multi-Agent（多智能体协作）

**配置**: `multi_agent = true`

**用户可见效果**: Codex 可以同时派生多个子 Agent 并行工作，TUI 中会显示各 Agent 的状态（运行中/已完成/出错）。

#### 工作原理

```
用户请求 "重构这 5 个文件"
  ↓
主 Agent 分析任务，决定拆分
  ↓
spawn_agent × 5（并行创建子 Agent）
  ↓
每个子 Agent 独立工作
  ↓
wait（等待所有子 Agent 完成）
  ↓
主 Agent 汇总结果，返回给用户
```

**五个核心操作**：
| 操作 | 说明 |
|------|------|
| `spawn_agent` | 创建子 Agent，分配独立上下文 |
| `send_input` | 向子 Agent 发送消息（可中断当前任务） |
| `wait` | 等待子 Agent 完成（10s~300s 超时） |
| `resume_agent` | 从 rollout 文件恢复已关闭的 Agent |
| `close_agent` | 优雅关闭子 Agent |

**安全限制**：
- 最大嵌套深度 = 1（子 Agent 不能再派生子 Agent）
- 子 Agent 自动获得 `AskForApproval::Never`（不会弹出审批弹窗阻塞流程）
- 记忆上下文自动传递给子 Agent

**TUI 显示**：
- 🔵 cyan = 运行中
- 🟢 green = 已完成
- 🔴 red = 出错
- 显示每个 Agent 的提示词预览和状态摘要

---

### 3. Agent Worktrees（Agent 工作树隔离）

**配置**: `agent_worktrees = true`

**用户可见效果**: 每个子 Agent 在独立的 git worktree 中工作，避免文件冲突。

#### 工作原理

```
repo_root/
  .codex/
    worktrees/
      agent/
        <uuid-1>/    ← Agent 1 的独立工作目录
        <uuid-2>/    ← Agent 2 的独立工作目录
    leases/
      <thread_id>.json  ← 元数据租约（支持崩溃恢复）
```

- 每个子 Agent 自动创建分支 `codex/agent/<uuid>`
- 基于当前 HEAD 创建 `git worktree add`
- Agent 完成后，主 Agent 可以 merge 结果
- 租约系统支持崩溃后恢复未完成的 Agent

**与 Multi-Agent 配合**：Multi-Agent 负责任务调度，Worktrees 负责文件隔离。两者配合使用效果最佳。

---

### 4. Ghost Commit / Undo（幽灵提交 / 撤销）

**配置**: `undo = true`

**用户可见效果**: 每轮对话开始时自动创建一个隐藏的 git 快照，用户可以随时撤销 Codex 的修改。

#### 工作原理

```
用户发送消息
  ↓
Turn 开始 → 异步创建 Ghost Commit（不影响任何分支）
  ↓
工具执行被 Gate 阻塞，等待快照完成
  ↓
快照完成 → 工具开始执行（apply_patch, shell 等）
  ↓
用户不满意 → 调用 Undo
  ↓
git restore --source <ghost_commit> --worktree
  ↓
工作目录恢复到 Turn 开始前的状态
```

**关键特性**：
- Ghost Commit 是脱离分支的孤立提交，不污染 git 历史
- 自动忽略大文件（>10MB）和大目录（>200 文件）
- 自动忽略 `node_modules`、`.venv`、`dist`、`build` 等
- 恢复时保留用户已 staged 的更改（数据安全优先）
- 快照超过 240 秒会发出警告

---

### 5. Apps（ChatGPT 应用集成）

**配置**: `apps = true`

**用户可见效果**: 可以在对话中用 `$` 提及 ChatGPT Apps，调用外部工具。

#### 工作原理

```
用户输入: "用 $calendar 查看今天的日程"
  ↓
解析 $ 提及 → 提取 connector_id
  ↓
激活对应 App 的 MCP 工具
  ↓
模型调用 App 提供的工具
  ↓
返回结果
```

- Apps 通过 MCP 协议暴露工具
- 通过 `/apps` 命令管理（安装/启用/禁用）
- 连接器列表缓存 1 小时
- 支持 markdown 链接语法：`[$app-name](app://connector-id)`

---

### 6. JavaScript REPL（持久化 Node.js 环境）

**配置**: `js_repl = true`

**用户可见效果**: Codex 可以在持久化的 Node.js 环境中执行代码，变量和状态跨多次执行保持。

#### 工作原理

```
模型决定执行 JS 代码
  ↓
发送到持久化 Node.js 内核进程
  ↓
在 VM 上下文中执行（支持 top-level await）
  ↓
变量绑定保留到下次执行
```

**暴露给模型的工具**：
- `codex.tool(toolName, args)` — 调用任意可用工具（shell、MCP 等）
- `codex.state` — 跨执行的可变状态存储
- `codex.tmpDir` — 临时文件目录

**安全限制**：
- 屏蔽 `node:process`、`node:child_process`、`node:worker_threads`
- 防止递归调用 js_repl
- 默认 30 秒超时（可通过 pragma 配置）

---

### 7. WebSocket Transport（WebSocket 传输）

**配置**: `responses_websockets = true` + `responses_websockets_v2 = true`

**用户可见效果**: 使用 WebSocket 替代 HTTP SSE 进行流式传输，降低延迟。

#### V1 vs V2 对比

| 方面 | V1 | V2 |
|------|----|----|
| Beta Header | `2026-02-04` | `2026-02-06` |
| 增量请求 | `response.append` | `response.create` + `previous_response_id` |
| 消息类型 | 两种（create + append） | 统一（create） |
| 追加判断 | 依赖服务端 `can_append` 标志 | 始终用 `previous_response_id` |

**优势**：
- 双向持久连接，无需每轮重建
- 支持连接预热（prewarm），首轮响应更快
- 自动降级：WebSocket 失败时 fallback 到 HTTP SSE

**自定义 Provider 要求**：
- 后端需实现 `wss://` 端点
- 配置 `supports_websockets = true`
- 不支持时自动降级，无风险

---

### 8. 其他功能

#### Codex Git Commit (`codex_git_commit = true`)
- 在模型指令中注入 git commit 归属指导
- 帮助模型生成规范的 commit message
- 自动添加 Co-Authored-By 信息

#### SQLite Persistence (`sqlite = true`)
- 将 rollout 元数据持久化到本地 SQLite
- 支持记忆系统的 Phase 1/2 数据存储
- 支持 Agent 状态恢复

#### Child Agents MD (`child_agents_md = true`)
- 将 AGENTS.md 的指导信息传递给子 Agent
- 确保子 Agent 遵循项目约定
- 与 Multi-Agent 配合使用

#### Skill Env Var Prompt (`skill_env_var_dependency_prompt = true`)
- 当 Skill 需要环境变量但未设置时，提示用户
- 避免因缺少配置导致的静默失败

#### Prevent Idle Sleep (`prevent_idle_sleep = true`)
- macOS 专属：Turn 运行期间阻止系统休眠
- 长时间任务不会因休眠中断

---

## 三、功能协作关系图

```
┌─────────────────────────────────────────────────────┐
│                    用户对话                           │
└──────────────────────┬──────────────────────────────┘
                       │
          ┌────────────┼────────────┐
          ▼            ▼            ▼
    ┌──────────┐ ┌──────────┐ ┌──────────┐
    │ Ghost    │ │ Memory   │ │ WebSocket│
    │ Commit   │ │ Tool     │ │ Transport│
    │ (快照)   │ │ (记忆)   │ │ (低延迟) │
    └──────────┘ └──────────┘ └──────────┘
          │            │
          │            ▼
          │      ┌──────────┐
          │      │ SQLite   │
          │      │ (持久化) │
          │      └──────────┘
          │
          ▼
    ┌──────────────────────────────────┐
    │         Multi-Agent              │
    │    ┌─────┐ ┌─────┐ ┌─────┐     │
    │    │ A1  │ │ A2  │ │ A3  │     │
    │    └──┬──┘ └──┬──┘ └──┬──┘     │
    │       │       │       │         │
    │    ┌──┴──┐ ┌──┴──┐ ┌──┴──┐     │
    │    │ WT1 │ │ WT2 │ │ WT3 │     │  ← Agent Worktrees
    │    └─────┘ └─────┘ └─────┘     │
    └──────────────────────────────────┘
          │
          ▼
    ┌──────────────────────────────────┐
    │  Child Agents MD (项目约定传递)   │
    │  Memory 上下文自动继承            │
    │  JS REPL (代码执行环境)          │
    │  Apps (外部工具集成)             │
    └──────────────────────────────────┘
```

---

## 四、配置文件完整参考

```toml
# ~/.codex/config.toml

[features]
# Stable (默认关闭)
undo = true                          # 每轮自动快照，支持撤销

# Experimental
multi_agent = true                   # 多 Agent 并行协作
agent_worktrees = true               # Agent 独立 git worktree
apps = true                          # ChatGPT Apps 集成
prevent_idle_sleep = true            # macOS 防休眠

# Under Development
memory_tool = true                   # 跨会话记忆系统
codex_git_commit = true              # Git commit 归属指导
sqlite = true                        # 元数据持久化
js_repl = true                       # 持久化 Node.js REPL
child_agents_md = true               # 子 Agent 项目约定传递
skill_env_var_dependency_prompt = true # 缺失环境变量提示
responses_websockets = true          # WebSocket 传输
responses_websockets_v2 = true       # WebSocket V2
```

---

## 五、"Memory updated" 效果说明

截图中显示的 "Memory updated" 是 Memory Tool 的核心用户体验：

1. **触发时机**: 对话过程中，当 Codex 检测到值得记忆的信息时
2. **提取内容**: 用户偏好、项目约定、调试经验、架构决策、常用命令
3. **存储位置**: `~/.codex/memories/` 下的结构化文件
4. **后续影响**: 下次启动 Codex 时，记忆自动加载到上下文中
5. **跨项目隔离**: 不同项目目录的记忆互不干扰

**典型场景**：
```
Session 1: "我们用 Rust nightly，cargo +nightly build"
           → Memory updated: 记录构建工具链偏好

Session 2: "帮我编译这个项目"
           → Codex 自动使用 cargo +nightly build（无需再次说明）

Session 3: "这个项目的测试怎么跑？"
           → Codex 结合记忆中的项目结构信息，给出精确命令
```

---

## 六、未开启的功能（及原因）

| 功能 | Key | 原因 |
|------|-----|------|
| UseLinuxSandboxBwrap | `use_linux_sandbox_bwrap` | Linux 专属，macOS 不适用 |
| WindowsSandbox | `experimental_windows_sandbox` | Windows 专属，已 Removed |
| WindowsSandboxElevated | `elevated_windows_sandbox` | Windows 专属，已 Removed |
| PowershellUtf8 | `powershell_utf8` | Windows 专属 |
| WebSearchRequest | `web_search_request` | 已 Deprecated |
| WebSearchCached | `web_search_cached` | 已 Deprecated |
| SearchTool | `search_tool` | 已 Removed |
| RemoteModels | `remote_models` | 已 Removed |
| RequestRule | `request_rule` | 已 Removed |
| AppsMcpGateway | `apps_mcp_gateway` | 需要网关配置，暂不需要 |
| ApplyPatchFreeform | `apply_patch_freeform` | 实验性 patch 工具，风险较高 |
| RuntimeMetrics | `runtime_metrics` | 开发调试用，日常不需要 |
| JsReplToolsOnly | `js_repl_tools_only` | 已开启完整 js_repl，无需此限制模式 |
