# State DB + Memory + App-Server Automation 协作说明

## 1. 目标与结论

本文把 `rollout`、`state.sqlite`、`memories`、`hooks`、`app-server v2` 的职责边界与协作链路讲清楚，目标是：

- 保持“**rollout 是唯一事实源**”不动摇。
- 让 `state.sqlite` 成为“**加速索引 + 并发协调**”，而不是权威数据源。
- 让 `memory` 与 `hooks` 成为多 agent 协作的稳定基础设施。
- 让 `app-server v2` 成为外部 automation 的标准控制面。

---

## 2. 图示索引（先看图，再看文）

- 组件关系图：`docs/uml/state_memory_automation_component.svg`
- 执行时序图：`docs/uml/state_memory_automation_sequence.svg`
- 关键对象与关联键：`docs/uml/state_memory_automation_data.svg`

对应源文件：

- `docs/uml/state_memory_automation_component.puml`
- `docs/uml/state_memory_automation_sequence.puml`
- `docs/uml/state_memory_automation_data.puml`

---

## 3. 各组件职责（人话版）

### 3.1 codex-core（执行引擎）

- 负责真正执行 turn：组装上下文、调用模型、执行本地/MCP 工具、收集结果。
- 同步做三件事：
  1) 追加写入 rollout 事件；
  2) 更新 state.sqlite 投影；
  3) 发送 hooks 事件给外部自动化。

### 3.2 rollout.jsonl（事实源）

- 记录“发生过什么”的完整事件序列（append-only）。
- 审计、追溯、重放都依赖 rollout。
- 设计原则：**任何状态都应可从 rollout 重建**。

### 3.3 state.sqlite（投影与协调）

- 作用一：提供线程列表/搜索/分页等快速读能力。
- 作用二：提供 jobs/locks，支持 memory 后台任务并发协作（claim、heartbeat、retry）。
- 非目标：不能取代 rollout 成为权威事实源。

### 3.4 memories（可注入记忆）

- 记忆文件位于 `~/.codex/memories/...`，核心是 `memory_summary.md`。
- turn 执行前按优先级选择活跃记忆：`cwd > user > global`。
- 记忆生成由后台 pipeline（stage1/stage2）完成并持续更新。

### 3.5 hooks（外部协作总线）

- 把关键事件（AfterAgent / AfterToolUse / AfterMcpToolCall）发给外部系统。
- 外部系统可做通知、审计、二次编排、跨 agent 触发。
- hooks 是“副作用与集成出口”，不是主存储。

### 3.6 app-server v2（automation 控制面）

- 对外提供 JSON-RPC（如 `thread/*`、`turn/*`、`model/*`）。
- 外部 orchestrator 通过它驱动 turn，并订阅通知。
- app-server 负责控制面协议，不替代 core 的执行职责。

---

## 4. 一次 turn 的端到端协作流程

1. 外部客户端调用 `turn/start`（可带 model override）。
2. app-server 把请求转给 core。
3. core 选择活跃 memory summary（`cwd > user > global`），并生成/携带 `MemoryLink`。
4. core 调用 provider（OpenAI/Gemini/Grok...），得到文本或 tool call。
5. 如需工具：
   - 本地工具：core 本地执行；
   - MCP 工具：core 通过 MCP client 调对应 server。
6. core 写 rollout（tool call/result、assistant message 等）。
7. core 发 hooks 事件（附带 thread/turn/call 信息与 memory 关联字段）。
8. core 更新 state.sqlite（线程投影、必要索引、memory stage1 输出、jobs 状态等）。
9. app-server 将过程通知和完成状态回传给 automation 客户端。

并行后台：memory pipeline 周期性跑 stage1/stage2，更新 `memory_summary.md`，供下一轮 turn 读取。

---

## 5. 关键关联键（多 agent 协作核心）

- 业务链路键：`threadId`、`turnId`、`callId`
- 记忆链路键：`MemoryLink`（`scope_kind`、`scope_version`、`summary_sha256`、`binding_key`）

推荐将 `binding_key` 作为跨系统 join key：

- 可以把“本次工具调用”与“当时生效的 memory 版本”稳定关联；
- 便于把不同 provider / 不同 agent 的行为归拢到同一上下文版本；
- 便于外部 orchestrator 做因果链路追踪与回归诊断。

---

## 6. 与 Session / 历史记录的关系

- Session 内存态（in-memory history）是当前对话执行态，强调“快”和“当前上下文”。
- rollout 是持久事实日志，强调“完整可追溯”。
- state.sqlite 是 rollout 的读优化与调度投影，强调“可检索”和“并发协调”。
- memories 是从历史中抽取的长期知识压缩，强调“跨 turn/跨 session 连续性”。

四者不是替代关系，而是分层关系：

- 执行态（Session）
- 事实层（rollout）
- 投影层（state.sqlite）
- 记忆层（memories）

---

## 7. 面向多 agent 的优化建议（不破坏现有体系）

### P0：一致性与自愈

- 保证“缺 state 数据时可回退 rollout 重建”。
- 对 `state db missing rollout path` 类问题增加自动补齐与降噪策略。

### P1：hooks 可靠投递

- 明确至少一次投递语义（at-least-once）。
- hook 目标实现幂等（建议按 `callId` 去重）。
- 增加失败重试/观测（避免多 agent 链路出现“丢一步”）。

### P1：MemoryLink 全链路透传

- 在 tool call、collab、hooks、app-server 通知中尽量完整携带 `MemoryLink`。
- 将 `binding_key` 固化为外部编排系统的标准上下文键。

### P2：控制面解耦

- 外部 automation 尽量只依赖 app-server v2 + hooks，不直连内部表结构。
- 减少对 `state.sqlite` 内部 schema 的耦合，降低演进成本。

### P3：性能与成本

- memory pipeline 继续做增量提取与节流策略。
- 对高频 list/search 使用 state 索引，但保持可回放可重建。

---

## 8. 建议的验证清单（每次迭代可复用）

- 功能验证：
  - turn 正常走通（含本地工具、MCP 工具）；
  - hooks 能收到事件且字段完整（含 memory 字段）；
  - app-server 通知顺序与状态正确。
- 一致性验证：
  - 清空或损坏 state 后，能否通过 rollout 回放恢复关键能力；
  - memory summary 更新后，下一 turn 能否携带新的 `binding_key`。
- 稳定性验证：
  - 并发 turn + 并发 memory pipeline 下无重复 claim/死锁；
  - hooks 目标异常时主流程不被破坏，失败可观测可重试。

---

## 9. 最小闭环示例：hooks → 编排器（entireio/cli adapter）→ app-server v2

这一节给一个“能跑起来的最小闭环”思路：Codex 负责执行与产出事件，编排器负责把事件变成后续动作（开新 turn / 叫另一个 agent / 写任务元数据）。

### 9.1 hook 触发的运行时契约（adapter 能依赖什么）

当 turn 完成或 MCP 工具调用完成时，Codex 会以 fire-and-forget 方式启动你配置的 `notify` 命令（见 `config.toml` 的 `notify = [...]`），并提供两类输入：

- **环境变量**（建议优先读，字段更直接）：`CODEX_HOOK_EVENT`、`CODEX_HOOK_THREAD_ID`、`CODEX_HOOK_TURN_ID`、`CODEX_HOOK_CWD`、`CODEX_HOOK_PROVIDER_NAME`、`CODEX_HOOK_MODEL_SLUG`、`CODEX_HOOK_MEMORY_*`、以及 MCP 场景下的 `CODEX_HOOK_MCP_*`。
- **最后一个 argv 参数**：一个 JSON 字符串（为了兼容旧 notify 行为），同样描述事件内容。

注意：

- `notify` 进程的 stdout/stderr 会被丢弃（为了避免污染主交互）；如果要调试，请在 adapter 内部写文件/打日志到你自己的系统。
- 当前 legacy notify hook 主要覆盖：
  - `agent-turn-complete`
  - `mcp-tool-call-complete`
  其他事件类型如果需要，建议通过新的 hooks 配置面扩展（避免继续塞进 notify 兼容层）。

### 9.2 配置示例（npx + GitHub repo）

你希望用 `npx` 直接运行 GitHub 上的 adapter（不用 npm publish）。可参考：

```toml
# ~/.codex/config.toml
notify = ["npx", "-y", "github:jiaqiwang969/cli#main", "hooks", "codex", "notify"]
```

说明：

- `notify` 只需要写“固定 argv”（不含 JSON）；Codex 会自动把事件 JSON 作为最后一个参数追加。
- 在当前 entireio/cli adapter 的实现下，Codex hooks 的“日志”与“会话元数据”分开存（避免日志跟随仓库移动，方便跨项目聚合）：
  - `$CODEX_HOME/hooks/entire/logs/entire.log`（默认 `~/.codex/...`）：结构化 JSONL 日志（包含 hooks 处理记录）。
  - `.entire/metadata/<session-id>/`（repo 根目录）：按 session 归档的 context/prompt/summary（便于项目级追溯与回放）。
- 你的 adapter 要做到：
  - 读取 `CODEX_HOOK_*` 环境变量（推荐）；或解析最后一个 argv 的 JSON；
  - 生成一个稳定的 idempotency key（例如 `event_type + thread_id + turn_id (+ call_id)`）去重；
  - 将事件写入自己的元数据存储/任务系统（entireio/cli 的强项）。

### 9.3 编排器最小逻辑（3 步）

1) **接收并落库（必做）**

- 把事件按 `thread_id/turn_id/call_id` 落到你自己的存储里（SQLite/JSONL/你们的任务系统皆可）。
- 把 `memory_binding_key` 一并存下来，作为跨 agent “同一记忆版本”的 join key。

2) **决策与派发（可配置规则）**

- 根据 `CODEX_HOOK_EVENT` 分支：
  - `agent-turn-complete`：适合触发“复盘/验收/补测/同步记忆”等后续动作。
  - `mcp-tool-call-complete`：适合做“失败兜底/重试/改用其他 agent 或 provider”的编排。
- 规则尽量从简单开始（白名单 + 显式条件），不要一上来做复杂 NLP 判定，避免误触发。

3) **通过 app-server v2 触发下一步**

- 编排器调用 app-server v2（JSON-RPC）发起新的 `turn/start`，把“下一步任务”作为输入。
- 必要时带上 model/provider override（例如失败后从 gemini 切到 gpt 做深度 debug，或叫 claude 做 review）。
- 由于 `MemoryLink` 会随 turn 注入与传播，后续 turn 将天然在同一工作目录记忆策略下连续推进。

### 9.4 三个可落地的场景（建议先做 smoke）

1) **MCP 失败兜底**

- 触发条件：`CODEX_HOOK_EVENT=mcp-tool-call-complete` 且 `CODEX_HOOK_MCP_STATUS != ok`。
- 编排动作：
  - 在同一个 `thread_id` 上发起新 `turn/start`，让 gpt 模型根据错误做兜底（改用本地命令 / 换工具 / 给出下一步手动指令）。

2) **“写完就跑测试”自动化**

- 触发条件：`agent-turn-complete`。
- 编排动作：
  - 如果本 turn 产生了 patch/文件变更（可通过 rollout/state 投影或你自己的变更检测），自动开一个 turn 让 agent 运行项目测试并回填结果。
  - 这一步要加“repo/cwd 锁”，避免并发执行造成工作区污染。

3) **多 agent 分工链路（研究 → 实现 → 复审）**

- 触发条件：`agent-turn-complete` 且用户请求涉及调研/比较/外部资料。
- 编排动作：
  - 先开一个 “grok/web-search” turn 拉资料；
  - 再开一个 “gemini/前端” turn 落实现；
  - 最后开一个 “claude/review” turn 做 PR 级别复审。
- 关键点：全过程用 `memory_binding_key` 关联，便于把三段输出串成一条可追溯链路。
