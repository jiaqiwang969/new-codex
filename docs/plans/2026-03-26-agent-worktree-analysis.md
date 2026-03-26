# Agent Worktree Merge Analysis

## 目的

给 Block 6 的 agent worktree 线补一份更精确的判断，区分：

- 现在线上真实生效的隔离/恢复语义
- 代码和文档里已经预留、但尚未真正接通的部分

## 结论

这条线值得保留，但要按“已落地语义”保，不要把文档里的理想态当成当前不可退让的 contract。

## 相对 upstream 的核心增量

本地分支新增了完整的 `agent_worktree` 模块：

- `codex-rs/core/src/agent_worktree.rs`

它做了四件事：

1. 为 thread 创建独立 git worktree
2. 生成并持久化 thread -> worktree 的 lease
3. resume thread 时按 lease 恢复缺失的 worktree
4. 提供 debug CLI 做人工巡检和恢复

## 真实在线的链路

### 1. fork 会话时创建隔离 worktree

在 TUI fork 路径上，如果打开 `Feature::AgentWorktrees`：

- 先调用 `create_agent_worktree(..., ForkedSession)`
- 把 fork 后 thread 的 `cwd` 切到新 worktree
- thread 创建成功后写 lease
- 失败时清理刚创建的 worktree

关键位置：

- `codex-rs/tui/src/app.rs:2525`

这说明当前真正在线的“创建”语义，首先是 fork session，不是子 agent。

### 2. resume thread 时按 lease 切回并自动恢复 worktree

在线程初始化路径中，只要是 `InitialHistory::Resumed`：

- 读取 lease
- 如果 worktree 缺失，则 `git worktree prune` + `git worktree add --force`
- 成功后把 `config.cwd` 指向 `lease.worktree_path`

关键位置：

- `codex-rs/core/src/thread_manager.rs:849`
- `codex-rs/core/src/agent_worktree.rs:220`

这块很重要，因为它把“历史 thread”重新绑定回正确的代码工作区，而不只是恢复 transcript。

### 3. debug CLI 支持人工巡检和恢复

当前还有一组实际可用的 debug 命令：

- `codex debug agent-worktrees list`
- `codex debug agent-worktrees ensure --thread`
- `codex debug agent-worktrees ensure --all`

关键位置：

- `codex-rs/cli/src/main.rs:628`

这使得 feature 不只是自动化路径，还具备运维/排障入口。

## 最关键的现实判断

### `SpawnedAgent` 目前更像预留，不是已接通主路径

虽然模块里定义了：

- `WorktreePurpose::SpawnedAgent`
- `codex/agent/<uuid>` 分支前缀
- `agent/` worktree 子目录

但当前仓内实际创建 worktree 的业务路径，只搜到 fork session：

- `codex-rs/tui/src/app.rs:2527`

`WorktreePurpose::SpawnedAgent` 的使用点，当前只出现在：

- 类型定义
- 模块单测

没有找到真正把 spawned sub-agent 放进独立 worktree 的运行时接线。

这和对外文档/feature 描述存在落差：

- `codex-rs/features/src/lib.rs:143`
- `codex-rs/docs/design/claude-mcp-context-memory.tex:255`

所以这块要这样看：

- “fork/resume 的 worktree 隔离”是已经落地的核心价值
- “spawned agent 也自动隔离”目前更像设计目标或半成品，不该在 merge 时当作已经成熟的硬约束

## 它碰到的边界面

### 不碰 protocol wire

这条线当前没有引入新的 protocol / app-server-protocol wire contract。

它影响的是：

- thread 初始化时的 `cwd`
- fork 时的本地执行环境
- CLI debug

所以它和 `MemoryLink` 不同，不是共享协议冲突源。

### 但会碰共享运行时入口

主要耦合点：

- `codex-rs/tui/src/app.rs`
- `codex-rs/core/src/thread_manager.rs`
- `codex-rs/cli/src/main.rs`

也就是说：

- 核心逻辑本身很孤立
- 真正的 merge 痛点在入口 wiring，不在协议层

## 值得保留的最小语义

必须保留：

- fork session 时可选创建独立 worktree
- thread -> worktree 的 lease 持久化
- resume thread 时自动切回 lease 对应 worktree
- 缺失 worktree 时的自动 restore
- debug CLI 的 list / ensure

建议保留：

- `purpose`、`parent_thread_id`、`pid` 这些 lease 元信息

原因：

- 它们虽然不是最小功能必需，但对排障和后续把 spawned-agent 真正接上很有价值。

## 可以调整的实现细节

- `agent_worktree.rs` 的内部 helper 形状
- `git worktree add/remove` 的封装方式
- TUI fork 时的错误提示和清理顺序
- debug CLI 的输出格式

这些都不是语义核心。

## merge 风险判断

这块比 `MemoryLink` 安全很多，因为它基本不碰共享协议。

主要风险不是“冲突很多”，而是“容易被文档带偏”：

- 误以为 spawned-agent worktree 已经完整落地
- 于是 merge 时去强保一条其实还没真正接通的运行时路径

正确策略应当是：

- 保已经落地的 fork/resume/lease/restore/CLI 语义
- 把 spawned-agent worktree 视为可继续完善的本地扩展，而不是这次 merge 的硬门槛

## 结论

agent worktree 是本地差异化资产，但它当前的真实价值集中在：

- fork 隔离
- resume 恢复正确代码工作区
- lease 驱动的恢复能力

这条线应当保留，而且保留成本相对低。

真正需要降噪的地方，不是删 feature，而是停止把“spawned-agent 也已完全隔离”当成已兑现事实。
