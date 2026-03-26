# Codex-RS 当前工程路线图（基于现有代码盘点）

> 版本：2026-02-10 盘点版
> 范围：`core/`、`tui/`、`app-server/`、`exec/`、`state/`，以及本机 `~/.codex/config.toml` 运行配置。
> 目标：按“现状 → 可复用资产 → 短板 → 路线图”逐项梳理，优先沉淀可复用能力。

---

## 0) 总览：当前 6+2 工程主线

1. 多 Agent 协作与共享记忆（长期主线）
2. Ralph Loop 向 Automation 演进
3. 多模型协作底座（Grok / Gemini / Gemma）
4. Infra 稳定性（Agent API Pool / Provider Failover）
5. Git Graph 资产已 parked，当前主线不再激活
6. 运行治理与可观测中台（建议提升为独立主线）
7. （补充）App-Server 协议化能力沉淀
8. （补充）质量与回归自动化（防回退体系）

---

## 1) 多 Agent 协作与共享记忆

### 1.1 现状（代码已落地）

- 协作工具链已经完整接入：`spawn_agent` / `send_input` / `resume_agent` / `wait` / `close_agent`（`core/src/tools/spec.rs`、`core/src/tools/handlers/multi_agents.rs`）。
- Agent 控制面支持 spawn/resume/send/interrupt/shutdown 与状态订阅（`core/src/agent/control.rs`）。
- 已有并发与深度保护：线程数上限与深度限制（`core/src/agent/guards.rs`，`MAX_THREAD_SPAWN_DEPTH = 2`）。
- Context Packet 已抽象为通用构建器（`core/src/context_packet.rs`），并已复用于：
  - `claude_code` MCP 调用自动注入 `workFolder/context`（`core/src/mcp_tool_call.rs`）
  - `spawn_agent` 初始 prompt 注入（`core/src/tools/handlers/multi_agents.rs`）
- 线程记忆链路已落地：`thread_memory` SQLite + `get_memory` 工具 + turn/compaction 后异步更新（`state/migrations/0006_thread_memory.sql`、`core/src/thread_memory.rs`、`core/src/tools/handlers/get_memory.rs`）。
- App-Server 和 Exec 已可结构化消费协作事件（`app-server/src/bespoke_event_handling.rs`、`exec/src/event_processor_with_jsonl_output.rs`）。

### 1.2 可复用资产

- `ContextPacketConfig + build_context_packet()`：可作为所有“子 Agent 启动上下文”的统一注入器。
- `thread_memory` 数据模型：可跨 TUI / App-Server / Automation 复用为“长期记忆层”。
- 协作事件协议（collab begin/end）：可复用于可视化、审计、指标统计。
- Agent 守卫（线程上限/深度）：可复用为任何自动化场景的安全闸门。

### 1.3 短板与风险

- 深度上限目前较保守（仅 1 层），对复杂分治任务不够。
- `memory_tool`、`sqlite` 仍为特性开关驱动，默认场景未完全“强制一致”。
- 已有 thread memory 回填工具（debug CLI：`codex debug thread-memory backfill ...`），但尚未产品化（文档/安全阀/限流策略）。

### 1.4 路线图

- **P0（1-2 周）**：统一 Context Packet 模板与字段契约，沉淀成可跨模型可扩展协议。
- **P1（2-4 周）**：将 thread memory 回填能力产品化（把 debug CLI 补齐为可发现/可控/可观测的工具）。
- **P2（4-8 周）**：放开可配置深度策略（按任务/模式动态限制），并增加协作质量评分。

---

## 2) Ralph Loop → Automation 演进

### 2.1 现状（代码已落地）

- TUI 已支持 `/ralph-loop` 与 `/cancel-ralph`（`tui/src/slash_command.rs`、`tui/src/chatwidget.rs`）。
- Ralph 状态机已独立模块化（`tui/src/ralph_loop.rs`）：
  - 迭代次数控制
  - `<promise>...</promise>` 完成判定
  - 出错延迟重试
  - 状态文件落盘：`.codex/ralph-loop.local.md`
- 事件循环已与 AppEvent 联动（`tui/src/app_event.rs`、`tui/src/app.rs`）。

### 2.2 可复用资产

- `RalphLoopState` 与参数解析器可抽到 `core`，做“无 UI 自动循环引擎”。
- 当前“回合结束触发下一轮”的机制可复用于任务编排器。

### 2.3 短板与风险

- 目前仅 TUI 内可用，尚未形成 App-Server API。
- 无定时/调度框架（cron/queue）与任务持久化队列。
- 缺少结构化运行指标（成功率、平均迭代次数、异常原因分布）。

### 2.4 路线图

- **P0（1-2 周）**：抽象 Ralph Loop Core（脱离 TUI）。
- **P1（2-4 周）**：App-Server 新增 loop start/status/stop RPC。
- **P2（4-8 周）**：接入定时调度（定时任务 + 幂等去重 + 超时熔断）。

---

## 3) 多模型协作底座（Grok / Gemini / Gemma）

### 3.1 现状（代码已落地）

- Provider 注册体系已支持 OpenAI / Gemini / Gemma / Grok（`core/src/model_provider_info.rs`）。
- 模型预设与 API 列表已纳入 Gemini/Gemma/Grok（`core/src/models_manager/model_presets.rs`、`app-server/tests/suite/v2/model_list.rs`）。
- 运行时支持按模型家族自动切换 provider（`core/src/config/mod.rs`、`core/src/codex.rs`）。
- 兼容性规则已显式编码（`core/src/model_compat.rs`）：
  - Grok 的 web_search/reasoning/memory_trace_summarize 限制
  - Gemma/Grok namespaced slug 归一化

### 3.2 可复用资产

- `model_compat` 能力矩阵函数可直接复用于“任务到模型路由器”。
- provider family auto-switch 逻辑可复用于 App-Server 与自动化任务入口。
- 统一的 model preset + reasoning effort 元数据可支撑前端选择器和策略引擎。

### 3.3 短板与风险

- Grok 当前不支持 `memory_trace_summarize`，跨会话记忆链路能力不对齐。
- Gemma 本地链路虽可跑，但缺少“健康检查 + 容量自检 + 首 token 监控”闭环。
- 当前本机 `~/.codex/config.toml` 存在 profile/provider id 不一致风险：
  - `profiles.grok.model_provider = "grok-main"`，但 provider 实际定义为 `[model_providers.grok]`
  - `profiles.gemini.model_provider = "gemini-main"`，但 provider 实际定义为 `[model_providers.gemini]`

### 3.4 路线图

- **P0（本周）**：修正 profile provider id，新增启动期配置自检。
- **P1（2-4 周）**：构建“能力感知路由”（按任务类型自动选模型）。
- **P2（4-8 周）**：引入多模型协同评审（Explorer/Worker 交叉校验）。

---

## 4) Infra 稳定性：Agent API Pool

### 4.1 现状（代码已落地）

- provider 层支持 `account_pool`（`core/src/model_provider_info.rs`、`../docs/config.md`）。
- 失败后自动切换同 provider 账号（`core/src/codex.rs` 中 `maybe_switch_provider_account()`）。
- 切换结果会回写 `config.toml` 当前生效账号（`persist_provider_account_selection()`）。
- 轮换逻辑具备去重、顺序遍历、单 turn 内不重复尝试（`normalize_account_pool()` / `next_account_from_pool()`）。

### 4.2 可复用资产

- 账号池逻辑可复用到任何 OpenAI-compatible/Gemini-compatible proxy。
- 按错误类型触发切换（401/403/429、重试耗尽）可复用于统一重试中间件。

### 4.3 短板与风险

- 目前偏“故障后切换”，缺少“事前健康检查 + 权重负载”。
- 无统一可观测面板查看账号池命中率/切换率。
- 配置校验仍偏运行时暴露问题，缺少 preflight lint。

### 4.4 路线图

- **P0（1 周）**：新增 `codex config lint`（provider id、env key、account_pool 完整性）。
- **P1（2-4 周）**：账号健康探针 + 冷却时间 + 权重轮换。
- **P2（4-8 周）**：SLO 化（provider 成功率、切换率、p95 首包时延）。

---

## 5) Git Graph 资产已 parked，当前主线不再激活

### 5.1 现状（代码已落地）

- 当前 merge 主线已经把 `Ctrl+G` 恢复为官方 external editor 快捷键（`tui/src/app.rs`）。
- `git-graph` 不再是活跃 workspace 成员，也不再接入当前 TUI 编译图。
- 仓库里仍保留 parked 的 `codex-rs/git-graph/` 资产，便于后续单独评估是否重启。

### 5.2 可复用资产

- vendored `git-graph` 树本身仍可作为独立能力候选，必要时再单独接回。
- 旧 overlay 的经验说明：如果未来要重启这类视图，应该走独立 feature block，
  而不是混进主 TUI merge 面。

### 5.3 当前判断

- 继续把它当活跃功能会引入额外依赖、锁文件和文档维护成本。
- 当前主线没有对应快捷键、测试和用户文档闭环，强行保留只会制造误导。

### 5.4 路线图

- **P0**：维持 parked 状态，不再占用当前 upstream merge 收口精力。
- **P1**：如果后续确认仍有价值，再单独决定是重启 vendored crate，还是只保留轻量
  `git log --graph` 视图。
- **P2**：只有在重新立项后，才考虑任务/代理/提交联动这类增强。

---

## 6) 运行治理与可观测中台（建议提升优先级）

### 6.1 现状（已有基础）

- 协作/MCP/命令事件均已结构化（`core` 事件 → `app-server` 通知 → `exec` JSONL）。
- App-Server 已有线程与会话管理接口（`thread/*`, `turn/*`, `command/exec`, `collaborationMode/list`）。

### 6.2 可复用资产

- `exec/src/exec_events.rs` 的统一事件模型可作为观测上报标准。
- `app-server` 双向 JSON-RPC 可作为编排平面。

### 6.3 短板

- 缺少统一 SLO、告警分级与 Runbook。
- 缺少跨子系统的 trace_id 级链路追踪。

### 6.4 路线图

- **P0（1 周）**：定义 5 个核心指标：成功率、失败率、p95 延迟、fallback 触发率、平均协作深度。
- **P1（2-4 周）**：接入告警与故障复盘模板。
- **P2（4-8 周）**：灰度发布 + 自动回滚策略。

---

## 7) App-Server 协议化建设（与自动化深度耦合）

### 7.1 现状

- 已支持 thread/turn 粒度控制与 streaming notifications（`app-server/README.md`）。
- 已支持 `collaborationMode/list` 与 collab tool item 映射。

### 7.2 可复用资产

- 作为统一“远程编排入口”，可以驱动 TUI 外自动化客户端。

### 7.3 路线图

- **P0**：将 Ralph Loop core 接入 app-server（先不做调度，仅做可远程触发）。
- **P1**：补充任务状态查询、取消、幂等 key。
- **P2**：加入定时触发器与外部 webhook 触发器。

---

## 8) 质量与回归自动化（稳定演进护栏）

### 8.1 现状

- 已有大量单测/集成测试基础，覆盖 collab/model/provider 等核心路径。

### 8.2 建议复用

- 利用 `core/tests/suite/*`、`app-server/tests/suite/*` 模式，沉淀“多 agent 场景回放集”。

### 8.3 路线图

- **P0**：为 6 条主链路建立 golden case（spawn→wait→close、memory update、provider failover 等）。
- **P1**：新增自动回归矩阵（模型家族 × 协作模式 × sandbox）。
- **P2**：引入稳定性压测（长会话、并发 sub-agent、故障注入）。

---

## 9) 可复用资产总表（优先沉淀）

| 资产 | 代码位置 | 当前使用方 | 下一步复用方向 |
|---|---|---|---|
| Context Packet Builder | `core/src/context_packet.rs` | `claude_code`、`spawn_agent` | 扩展到 review/app-server/automation |
| Thread Memory 存储与读取 | `core/src/thread_memory.rs` + `state/src/runtime.rs` | turn/compaction、`get_memory` | 历史回填 + 多项目记忆融合 |
| Collab Tool 协议与处理器 | `core/src/tools/spec.rs` + `core/src/tools/handlers/collab.rs` | core/tui/app-server/exec | 自动编排 DSL |
| Agent 守卫机制 | `core/src/agent/guards.rs` | sub-agent 生命周期 | 动态深度策略/资源配额 |
| Provider Account Pool | `core/src/model_provider_info.rs` + `core/src/codex.rs` | 模型请求容灾 | 健康探针 + 权重路由 |
| Exec JSONL 事件模型 | `exec/src/exec_events.rs` | 自动化消费端 | 观测平台统一上报格式 |
| Parked git-graph assets | `git-graph/` | 当前主线未启用 | 独立重评是否需要轻量重接 |

---

## 10) 90 天里程碑建议（按可落地优先级）

### M1（第 1-2 周）：稳定性清障

- 修复 profile/provider id 不一致（本机配置）
- 增加 config lint（provider/account_pool/env_key）
- 梳理并冻结 Context Packet 字段契约 v1

### M2（第 3-6 周）：能力产品化

- Ralph Loop Core 下沉到 `core`
- App-Server 暴露 loop start/status/stop
- Thread memory backfill 命令产品化（当前已存在 debug CLI）

### M3（第 7-12 周）：规模化与治理

- 定时调度 + 幂等机制
- 可观测指标面板 + 告警分级 + Runbook
- Git Graph 与 thread/turn 关联展示

---

## 11) 本周可直接执行的 12 条任务（建议）

1. 修正 `~/.codex/config.toml` 中 `profiles.grok` / `profiles.gemini` 的 `model_provider` 名称。  
2. 新增 provider preflight 校验命令（至少覆盖 provider existence + account_pool 完整性）。  
3. 将 `RalphLoopState` 从 `tui` 迁移至 `core`（先不改 UI 行为）。  
4. 为 Ralph Loop 增加结构化运行事件（start/iteration/error/complete/cancel）。  
5. 提供 thread_memory backfill CLI（按 rollout 目录批处理）。  
   - 现状：`codex debug thread-memory backfill --all --archived --force`
   - 下一步：补齐 dry-run、并发/限流、敏感信息保护与可观测性
6. 扩展 Context Packet 到 review 子流程。  
7. 把 collab 关键事件统一打上 trace_id。  
8. 在 `exec --json` 输出增加协作统计摘要。  
9. 为 provider failover 增加 cooldown 逻辑。  
10. 为 Git Graph overlay 增加 branch 过滤与刷新参数。  
11. 建立多模型协作 smoke tests（Grok/Gemini/Gemma）。  
12. 建立首版 SLO 面板：成功率、p95、fallback 率。

---

## 12) 结论

当前代码已经具备“多 agent + 共享记忆 + 多模型 + provider 容灾 + 可视化”的核心骨架。下一阶段关键不是再加散点功能，而是：

- 把 **Ralph Loop 产品化为可编排能力**（脱离单一 TUI）
- 把 **可复用组件升级为平台能力**（context/memory/collab/provider/event）
- 把 **稳定性与治理** 与功能并行推进（SLO、告警、回滚、回归）

这样可以保证你前面 5 条主线真正合流为一个可持续演进的工程体系。
