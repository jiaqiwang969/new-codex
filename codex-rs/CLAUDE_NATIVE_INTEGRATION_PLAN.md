# Claude Native Integration (Final Plan)

目标：让 Claude（Opus/Sonnet）在 codex-rs 里成为与 GPT/Grok/Gemini/Gemma 完全平权的 Provider，并且支持“互换身份”：

- `/model claude-*` 时，Claude 可以作为 leader 调用 GPT 作为小弟（spawn_agent / send_input / wait / close_agent）。
- `/model gpt-*` 时，GPT 也能调用 Claude 作为小弟。
- memory / compact 等 utility 能力不因 leader provider 切换而降级（否则长线程跑不下去）。

本方案以“三平面（Model / Collaboration / Utility）”为核心抽象，避免把 provider 特性、协作协议、基础设施能力耦合在一起。

---

## 0. 设计原则

1. Provider 平权：Claude 不是 “MCP 外挂”，而是 `WireApi` 的一等公民（像 Gemini 一样）。
2. 协作与 provider 解耦：`spawn_agent` 的生命周期/事件流不依赖某个 provider。
3. Utility 平面独立：memory/compact/summary 等内部任务必须可以跨 provider 运行（必要时用 utility provider 回退）。
4. 渐进落地：先 MVP 跑通，再补齐高级特性（thinking/images/caching/error mapping/router）。

---

## 1. Model Plane（Provider 平权）

### 1.1 Anthropic Wire Protocol

- 新增 `WireApi::Anthropic`，并注册内置 provider：`anthropic`。
- 认证：`x-api-key` + `anthropic-version`（默认 `2023-06-01`）。
- 端点：`POST /v1/messages`（SSE stream）。

实现落点（已完成）：

- `core/src/model_provider_info.rs`
- `core/src/client.rs`：新增 `stream_anthropic()`
- `core/src/anthropic_types.rs`
- `core/src/anthropic_content.rs`
- `core/src/anthropic_streaming.rs`

### 1.2 Claude 模型元数据与 Prompt

关键点：

- 1M context：`context_window = 1_048_576`
- 工具调用：以 Function tool 为主（与 Codex 工具契约对齐）
- Claude 专用 system addendum：强调“尽快 tool call、严格 apply_patch 语法、简洁 handoff”

实现落点（已完成）：

- `core/src/models_manager/model_info.rs`：`CLAUDE_INSTRUCTIONS`
- `core/claude_prompt.md`
- `core/src/model_compat.rs`：`normalized_anthropic_model_slug` / `is_anthropic_model_slug`

### 1.3 Provider Auto-Switch（防止 role/config 漏写 provider）

需要覆盖两条链路：

1. config load / role layer merge（`apply_role_to_config()` 走 `Config::load_config_with_layer_stack()`）
2. runtime `/model` 切换（`SessionConfiguration::apply()`）

目标行为：

- `claude-*` 自动切到 `anthropic`
- `gemini-*` / `gemma-*` / `grok-*` 自动切到对应 provider
- 当 leader provider 是非 Responses（例如 Anthropic/Gemini）且目标模型是 OpenAI slug（`gpt-*` / `o1-*` / `o3-*` / `o4-*`），自动切到 Responses provider（优先用户的 responses provider，否则 `openai`）

实现落点（已完成）：

- `core/src/config/mod.rs`：新增 OpenAI slug 的 auto-switch（仅当 current provider 非 Responses）
- `core/src/codex.rs`：`SessionConfiguration::apply()` 增强（仅当目标是 OpenAI slug 且 current provider 非 Responses）
- `core/src/model_compat.rs`：新增 `is_openai_model_slug()`

---

## 2. Collaboration Plane（蜂群协作 / spawn_agent 生命周期）

### 2.1 让“协作拓扑”显式化

leader 必须能看到 swarm 的拓扑关系，否则无法形成真正的蜂群调度。

落实点（已完成）：

- `spawn_agent` tool result 增加：
  - `parent_thread_id`
  - `spawn_depth`
- prompt 注入手工 “Swarm handoff” block：
  - parent thread id / spawn depth
  - memory scope version / binding key
  - worktree 信息（隔离 vs 共享）
  - handoff 要求（touched files / decisions / risks）

实现落点（已完成）：

- `core/src/tools/handlers/multi_agents.rs`
- `core/src/agent/guards.rs`（spawn 深度上限）
- `core/src/tools/spec.rs`（tool 描述同步）

### 2.2 Role Tags（为 Phase Router 埋种子）

给每个 role 增加 `tags: Vec<String>`，把能力标签暴露给 leader，后续不需要额外 “router 组件” 也能让 leader 自己路由任务。

示例 tags：

- `large_context`
- `fast`
- `deep_reasoning`
- `tool_intensive`
- `execution`

实现落点（已完成）：

- `core/src/config/mod.rs`：`AgentRoleConfig.tags` / `AgentRoleToml.tags`
- `core/src/agent/role.rs`：spawn tool spec 渲染 tags

### 2.3 Claude 角色（内置小弟）

内置两个一等公民角色（通过 role config 走原生 provider，而不是 MCP）：

- `claude-opus`：深推理
- `claude-sonnet`：快执行

实现落点（已完成）：

- `core/src/agent/builtins/claude-opus.toml`
- `core/src/agent/builtins/claude-sonnet.toml`
- `core/src/agent/role.rs`

---

## 3. Utility Plane（memory / compact / summarize 的跨 provider 可用性）

这是 Claude 当 leader 的硬前置：否则长线程无法持续。

### 3.1 Utility Model Router（小工具：按任务选 provider）

引入轻量 `utility_model` 路由层，用于 **内部任务**（memory pipeline / trace summarize / previous-model compact 等），避免 “用 Anthropic provider 去跑 gpt-*” 这种硬错误。

实现落点（已完成）：

- `core/src/utility_model.rs`
- `core/src/client.rs`：新增 `ModelClient::clone_with_provider()`

### 3.2 Memory Phase 1 / Phase 2 与 leader provider 解耦

问题：

- Phase 1/2 的 model 由 `[memories].phase_1_model/phase_2_model` 决定，但之前实现绑死 `session.services.model_client`（也就是 leader provider）。

目标：

- Phase 1 extraction 使用 phase_1_model 对应的 provider/model（必要时走 utility provider）
- Phase 2 consolidation agent 的 Config 必须同步更新 `model_provider_id/model_provider`

实现落点（已完成）：

- `core/src/memories/phase1.rs`
- `core/src/memories/phase2.rs`

### 3.3 Memory Trace Summarize（Responses-only endpoint）的 fallback

问题：

- `/v1/memories/trace_summarize` 是 Responses-only unary endpoint。
- Claude/Gemini leader 需要把这类任务外包给 utility Responses provider（默认 `openai` + `gpt-5.1-codex-mini`）。

实现落点（已完成）：

- `core/src/thread_memory.rs`：遇到 `UnsupportedOperation` 时 fallback 到 utility client/model

### 3.4 Previous-model inline compact 的 provider 正确性

问题：

- 历史上 `TurnContext::with_model()` 只换 model，不换 provider；跨 provider 切换后会导致 compact 用错 endpoint。

实现落点（已完成）：

- `core/src/codex.rs`：`TurnContext::with_model()` 使用 `utility_model::provider_for_model_slug()` 同步更新 provider/config

---

## 4. Phase Plan（建议顺序）

### Phase 1（MVP：Claude 原生 provider + 角色）

验收：

- `WireApi::Anthropic` 可用
- `claude-opus` / `claude-sonnet` 可 spawn，支持工具调用
- `/model` 在 Claude/GPT/Gemini/Grok/Gemma 间切换不会路由到错误 endpoint

状态：已实现（以当前分支为准）。

### Phase 2（Utility Plane：memory/compact 不降级）

验收：

- Claude leader 场景下，memory phase1/phase2 能正常跑（不依赖 leader provider）
- thread_memory trace summarize 在非 Responses leader 下能 fallback
- previous-model compact 不会跨 provider 打错 endpoint

状态：已实现核心路径；建议补充更多端到端覆盖（见下）。

### Phase 3（质量与能力补齐）

1. Claude thinking（extended thinking）更系统的配置桥接（`ReasoningEffort` -> budget mapping）
2. Claude 图片输入（Messages API image blocks）与 Codex `InputImage` 映射
3. prompt caching（Anthropic cache tokens）与 TokenUsage 统计对齐
4. 错误映射与重试策略细化（429/5xx/invalid_request/context_length）

### Phase 4（真正的蜂群：路由/调度）

不引入“硬编码 router”，而是把能力暴露给 leader：

- 基于 role tags 的自路由（leader 自己选择 subagent 类型）
- 再进一步：提供一个可选 “Swarm Planner” 工具（只输出调度 plan，不直接改代码）

---

## 5. Testing Strategy（建议）

在 sandbox 环境下，wiremock 绑定端口可能 PermissionDenied（环境限制）；建议优先：

- `cargo test -p codex-core <filter>`（跑 unit tests）
- `cargo test -p codex-app-server --test all -- --nocapture model_list`（模型列表契约）

对于需要真实 API 的路径，建议在非 sandbox 环境验证：

- `/model claude-opus-4-6` + 工具调用（shell/apply_patch）
- Claude leader spawn GPT explorer（验证 provider auto-switch）
- memory phase1/phase2 的落盘与 phase2 agent spawn

