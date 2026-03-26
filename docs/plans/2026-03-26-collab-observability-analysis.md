# Collab Spawn Observability Merge Analysis

## 目的

给 Block 8 这组 collab spawn 附加字段做一次去噪，回答三个问题：

- 哪些字段已经是当前分支里真的在跑的契约
- 哪些字段只是 schema/UI 预留位
- merge `upstream/main` 时，哪些值得保，哪些不值得为它们扭曲共享协议

## upstream 基线

相对 `upstream/main`，当前分支在 collab spawn 这一层额外扩了：

- `agent_type`
- `model_provider_id`
- `model_source`
- `model_source_detail`

关键差异点：

- upstream 的 `CollabAgentSpawnEndEvent` 没有这组附加字段；
- upstream 的 app-server v2 `CollabAgentState` 只有 `status + message`；
- 当前分支把这组字段往 core protocol、thread history 和部分 TUI 表达上扩了一层。

这意味着它们不是 upstream 已接纳的基础协议，而是本地差异化观测面。

## 当前分支的真实形态

### 1. core protocol 已有完整字段定义

当前定义位置：

- `codex-rs/protocol/src/protocol.rs:3439`
- `codex-rs/protocol/src/protocol.rs:3513`

这里不仅加了 `model_source` / `model_source_detail` 两个 enum，也把
`CollabAgentSpawnEndEvent` 扩成了：

- `agent_type`
- `model`
- `model_provider_id`
- `model_source`
- `model_source_detail`

## 2. 真正有生产者的，只有 `agent_type`

当前两个 spawn handler 都会发：

- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs:140`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs:138`

但它们的实际填充情况是：

- `agent_type`: 用 `role_name` 写入
- `model`: 用 `agent_snapshot.model` 写入
- `model_provider_id`: 明确写成 `None`
- `model_source`: 明确写成 `None`
- `model_source_detail`: 明确写成 `None`

这说明：

- `agent_type` 是真实在线字段
- `model_provider_id` / `model_source*` 现在还不是在线语义，只是接口位

## 3. app-server v2 只部分接住了这组信息

当前 v2 状态结构是：

- `codex-rs/app-server-protocol/src/protocol/v2.rs:4744`

只保留：

- `agent_type`
- `model`
- `model_provider_id`

没有：

- `model_source`
- `model_source_detail`

thread history builder 也只记忆这三项：

- `codex-rs/app-server-protocol/src/protocol/thread_history.rs:83`
- `codex-rs/app-server-protocol/src/protocol/thread_history.rs:656`
- `codex-rs/app-server-protocol/src/protocol/thread_history.rs:880`

所以从 app-server v2 的视角看：

- `agent_type` / `model_provider_id` 至少进入了持久化历史语义
- `model_source*` 根本没有进入 v2 契约

## 4. app-server live 通知并没有把这些字段真正发给前端

`bespoke_event_handling` 在 live `ItemCompletedNotification` 里，只是把
`AgentStatus` 转成 `CollabAgentState`：

- `codex-rs/app-server/src/bespoke_event_handling.rs:1111`

这里没有把：

- `agent_type`
- `model_provider_id`

回填进 live notification 的 `agents_states`。

也就是说，即使 v2 类型和 thread history 能装这些值，app-server 的实时通知路径现在也没有完整暴露它们。

## 5. core TUI 确实有消费者，但强弱分层明显

### `agent_type`

真实消费者明确：

- `codex-rs/tui/src/multi_agents.rs:220`
- `codex-rs/tui/src/multi_agents.rs:662`
- `codex-rs/tui/src/chatwidget.rs:1811`

它会影响：

- spawn 记录中的 role 展示
- 后续 wait / close / resume 时的已知 agent 元数据

这条线是完整的：producer 存在，consumer 也存在。

### `model_provider_id`

消费者也有，但当前 producer 缺失：

- `codex-rs/tui/src/multi_agents.rs:226`
- `codex-rs/tui/src/multi_agents.rs:668`
- `codex-rs/tui/src/chatwidget.rs:1818`

它的价值不在 UI 装饰本身，而在本仓明确保留了：

- account-pool
- 自定义 provider endpoint

在这种分支语义下，provider attribution 不是纯噪音，后续排障很有用。

但就“现在是否已经在线”而言，它还不是。

### `model_source` / `model_source_detail`

消费者存在，但全部在 core TUI 本地：

- `codex-rs/tui/src/multi_agents.rs:229`
- `codex-rs/tui/src/multi_agents.rs:671`
- `codex-rs/tui/src/chatwidget.rs:7507`

它们主要服务于：

- spawn 详情里的 route 标签
- `model_sub` / `model_sub_auto` 的 utility routing hint

问题在于：当前仓里找不到任何非测试生产者会把这两个字段填成非 `None`。

也就是说：

- 这些字段不是“活跃 contract”
- 它们更像一套尚未真正接通的 UI 观测设计

## 6. app-server TUI 这边没有形成第二套强消费者

`tui_app_server` 在把 app-server item 转回 legacy core 事件时，直接把这组字段全部写成 `None`：

- `codex-rs/tui_app_server/src/chatwidget.rs:3560`

所以即便 core TUI 有 `model_source*` 的渲染逻辑：

- app-server TUI 路径并没有跟上
- 它不是一个跨前端都成立的 contract

## merge 决策

### 必须保留

- `agent_type`

理由：

- 它是当前唯一真实 producer + consumer 都成立的附加字段
- 它还能进入 app-server thread history，具备最小跨层意义

### 建议保留，但不要为它扭曲 merge

- `model_provider_id`

理由：

- 对这个分支来说，provider attribution 不是空洞观测值，和 account-pool / 自定义 endpoint 有真实关系
- `agent_snapshot` 本身已经能拿到 `model_provider_id`
  - `codex-rs/core/src/agent/control.rs:628`
  - `codex-rs/core/src/codex_thread.rs:33`
- 但当前 handler 还没真正把它发出来，所以它不该成为 merge 阻塞项

换句话说：

- 值得保留为“可完成的观测点”
- 不值得为了它破坏 upstream 的 collab/app-server 主体结构

### 不应当作为 shared-wire 硬约束来保

- `model_source`
- `model_source_detail`

理由：

- 当前没有真实 producer
- app-server v2 没有这两个字段
- app-server TUI 也没有消费链
- 它们现在主要是 core TUI 的本地观测预留位

正确做法应当是：

- 不把它们当这次 merge 的硬门槛
- 如果以后真的把 `model_sub` provenance 从 spawn 选择链一路打通，再决定是否重新扩 shared wire

## 冲突处理建议

如果 merge 冲突落在这组字段附近，优先级应当是：

1. 先保 upstream 的 collab lifecycle 结构
2. 再保 `agent_type`
3. 再看 `model_provider_id` 能否低成本接到 `agent_snapshot.model_provider_id`
4. 不要为了 `model_source*` 去扩 app-server v2 或重写大量 shared protocol 代码

更直白地说：

- `agent_type` 值得 defend
- `model_provider_id` 值得 opportunistic keep
- `model_source*` 不值得当下硬扛

## 结论

Block 8 不应该按“四个字段等权保留”来处理。

更准确的结论是：

- `agent_type` 是已落地的真实附加元数据，应该保
- `model_provider_id` 对本分支有价值，但当前仍属半接线状态，可以保留方向，不要保留负担
- `model_source` / `model_source_detail` 现在还不是成熟契约，不应该成为 upstream merge 的阻塞点

因此，Block 8 的正确策略不是“尽量都留下”，而是：

- 只为已经证明有价值且已真正在线的字段花 merge 预算
- 把未接通的 observability 预留位从 shared-wire 决策里降级
