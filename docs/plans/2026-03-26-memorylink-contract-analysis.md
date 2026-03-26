# MemoryLink 最小契约分析

## 目的

在继续 `upstream/main` 合并时，给 `MemoryLink` 这条本地 continuity 线划出最小保留边界，避免后续为了减冲突把真正有价值的 contract 一起删掉。

## 现状结论

- 这不是 TUI 侧需求，当前真实消费面主要是：
  - hooks 输出
  - MCP tool call 参数注入与回传
  - app-server v2 对外通知/历史项
- `MemoryLink` 当前完整字段为：
  - `scope_version`
  - `scope_kind`
  - `summary_sha256`
  - `binding_key`
- `binding_key` 在现有设计里已经被当成首选跨系统 join key。
- `scope_version` 是更短、更可读的版本标识。
- `scope_kind` 与 `summary_sha256` 本质上更偏诊断/冗余信息。

## 最小可保留契约

### 1. 身份标识

必须保留：

- `binding_key`

建议保留：

- `scope_version`

理由：

- `binding_key` 才是稳定关联“这次 tool/collab 行为对应哪一版 memory”的主键。
- 只保留 `binding_key` 也能工作，但外部系统需要解析字符串才能拿到更多语义，契约会变脆。
- `scope_version` 作为短 ID 很适合日志、排障和人读，不应该强迫外部系统从 `binding_key` 反解。

兼容保留、但不是最小必需：

- `scope_kind`
- `summary_sha256`

原因：

- `scope_kind` 可从 `scope_version` 前缀推断。
- `summary_sha256` 可从 `binding_key` 后半段推断。
- 这两个字段现在更多是在降低排障成本，而不是唯一性本身所必需。

## 必须保留的暴露面

### 1. app-server v2

必须保留：

- `Turn.memory`
- `ThreadItem::McpToolCall.memory`
- `ThreadItem::CollabAgentToolCall.memory`

理由：

- `Turn.memory` 负责声明 turn 开始/结束时的活跃 memory。
- 仅有 `Turn.memory` 不够，因为外部自动化经常要把某个具体 MCP/collab 行为和 memory 版本对齐。
- item 级 `memory` 让外部系统不必靠“猜它属于哪个 turn 状态”来回填。

### 2. hooks

必须保留：

- `HookEventAfterAgent.memory`
- `HookEventAfterMcpToolCall.memory`
- `HookEventAfterToolUse.memory`

建议保留：

- `memory_context`

理由：

- `memory` 是面向外部编排的 continuity 元数据。
- `memory_context` 不是主键，但它提供了 active memory root / summary path / exists 状态，实际很适合诊断和自动化旁路检查。

## hooks 平铺字段是否还能删

当前不建议直接删：

- `memory_scope_version`
- `memory_scope_kind`
- `memory_summary_sha256`
- `memory_binding_key`

原因不是它们更“正确”，而是它们已经承担了兼容输出职责：

- `hooks/src/user_notification.rs` 现在就是从这些平铺字段导出环境变量。
- 外部 hook 脚本也很可能已经直接依赖这些 JSON 顶层字段，而不是嵌套 `memory`。

结论：

- 嵌套 `memory` 应视为 canonical shape。
- 平铺字段应视为兼容层，而不是继续扩散的新标准。
- 真要裁剪，应该先改成“内部从 `memory` 计算 env，再评估是否做版本化移除”，不能在这次 upstream merge 里顺手砍。

## 最容易继续冲突的文件

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/bespoke_event_handling.rs`
- `codex-rs/hooks/src/types.rs`
- `codex-rs/core/src/mcp_tool_call.rs`

原因：

- upstream 近期也在持续改 collab/app-server/protocol 形状；
- 我们的 `MemoryLink` 正好插在这些高频变动边界上；
- `hooks/src/types.rs` 这块几乎是纯本地扩展，未来每次 merge 都会重新碰撞。

## 后续合并建议

优先策略：

- 保 core 内部 memory/Entire 能力不动；
- 把对外 contract 压在少数边界层；
- 不再把 `MemoryLink` 扩散到更多 UI 或附属协议面。

如果以后必须继续缩面，建议顺序：

1. 先把 hooks 的平铺字段降级为兼容层认知，不再新增更多重复字段。
2. 如需继续瘦身，优先考虑让外部 contract 只承诺 `binding_key + scope_version`。
3. `scope_kind` / `summary_sha256` 作为次级兼容信息，放到最后再评估是否去掉。

## 结论

`MemoryLink` 不该被视为一整块都可删的本地噪音。真正必须保的是：

- `binding_key`
- `scope_version`
- `Turn.memory`
- `McpToolCall.memory`
- `CollabAgentToolCall.memory`
- hooks 的嵌套 `memory`

而最应该被当成“未来可压缩冲突面”的，是 hooks 的平铺冗余字段，不是 continuity 本身。
