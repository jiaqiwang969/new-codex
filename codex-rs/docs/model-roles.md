# Codex 模型角色说明

## 状态面板中的模型类型

从 `/status` 命令可以看到多个模型配置，每个都有特定的角色：

### 1. Model (主模型)

**角色**: 主要的对话和代码生成模型

**配置**:
```toml
model = "claude-sonnet-4-6"
```

**用途**:
- 处理用户的主要请求
- 生成代码和文档
- 进行复杂推理
- 执行工具调用

**运行时切换**: `/model`

---

### 2. Utility/sub-agent (工具/子代理模型)

**角色**: 用于子任务和工具调用的通用模型

**配置**:
```toml
model_sub = "claude-sonnet-4-6"
```

**用途**:
- 子代理（explorer, worker）的默认模型
- 快速工具调用和辅助任务

**运行时切换**: `/model-sub`

---

### 3. Resp. util model (响应工具模型)

**角色**: 用于 Responses API 的工具调用模型

**配置**:
```toml
model_sub_responses = "gpt-5.3-codex-spark|[pro]"
```

**用途**:
- Responses API 的工具执行
- 并行工具调用优化
- 特定于 Responses 协议的任务

**运行时切换**: `/model-sub-responses`

**继承链**:
```
model_sub_responses → model_sub → 内置默认
```

---

### 4. Memory phase-1 (记忆第一阶段)

**角色**: 快速代码库扫描模型

**默认值**: `gpt-5.3-codex-spark` (快速扫描优化)

**配置**:
```toml
[memories]
phase_1_model = "gpt-5.3-codex-spark"
```

**用途**:
- 快速扫描代码库结构
- 提取初始代码片段
- 识别相关文件和符号

**运行时切换**: `/model-memory-phase1`

---

### 5. Memory phase-2 (记忆第二阶段)

**角色**: 深度代码分析和整合模型

**默认值**: `gpt-5.3-codex` (深度分析优化)

**配置**:
```toml
[memories]
phase_2_model = "gpt-5.3-codex"
```

**用途**:
- 深度代码分析
- 整合和精炼记忆
- 生成高质量的代码上下文

**运行时切换**: `/model-memory-phase2`

---

### 6. Entire summary (Entire 摘要)

**角色**: AI 会话历史摘要模型

**默认值**: `claude-sonnet-4-6` (WHY 聚焦)

**配置**:
```toml
[memories]
entire_summary_model = "claude-sonnet-4-6"
```

**用途**:
- 生成 AI 会话的 WHY 聚焦摘要
- 捕获决策理由和上下文
- 支持 Entire 可追溯性系统

**运行时切换**: `/model-entire`

---

## 模型独立性原则

从 2025 年开始，所有后台任务模型（Memory phase-1、phase-2、Entire summary）都是**完全独立**的：

- ✅ 每个模型都有自己的默认值
- ✅ 每个模型都有独立的 `/model-xxx` 命令
- ✅ 不再从 `model_sub` 继承
- ✅ 可以根据任务特性选择最优模型

**设计理由**:
- Memory phase-1 需要快速扫描 → 使用 `gpt-5.3-codex-spark`
- Memory phase-2 需要深度分析 → 使用 `gpt-5.3-codex`
- Entire summary 需要 WHY 聚焦 → 使用 `claude-sonnet-4-6`
- Sub-agent 需要通用能力 → 使用 `model_sub` 配置

这种独立性避免了语义混淆，并允许针对每个任务优化模型选择。

**用途**:
- 扫描代码库结构
- 提取关键文件和模块
- 生成初始上下文摘要
- 快速、广泛的信息收集

**继承链**:
```
memories.phase_1_model → model_sub → 内置默认
```

---

### 5. Memory phase-2 (记忆第二阶段)

**角色**: 深度分析和整合记忆的模型

**配置**:
```toml
[memories]
phase_2_model = "claude-sonnet-4-6"
```

**用途**:
- 深度代码分析
- 架构理解
- 依赖关系推理
- 生成高质量的记忆摘要

**继承链**:
```
memories.phase_2_model → model_sub → DEFAULT_MEMORY_PHASE_TWO_MODEL
```

---

### 6. Entire summary (Entire 总结)

**角色**: 为 Entire checkpoint 生成 WHY-focused 总结的模型

**配置**:
```toml
[memories]
entire_summary_model = "claude-sonnet-4-6"
```

**用途**:
- 分析 AI 会话历史
- 生成决策理由说明
- 捕获 MOTIVATION, APPROACH, TRADEOFFS
- 为未来会话提供上下文

**运行时切换**: `/model-entire`

**继承链**:
```
memories.entire_summary_model → model_sub → DEFAULT_MEMORY_PHASE_TWO_MODEL
```

---

## 配置策略

### 最小配置（推荐）

只配置核心模型，其他自动继承：

```toml
model = "claude-sonnet-4-6"
model_sub = "claude-sonnet-4-6"

[memories]
# 所有 memory 相关模型继承自 model_sub
```

### 差异化配置

根据任务特点选择不同模型：

```toml
# 主模型：使用最强模型处理复杂任务
model = "claude-opus-4-6"

# 工具模型：使用快速模型处理简单任务
model_sub = "claude-sonnet-4-6"

# Responses 工具：使用专门优化的模型
model_sub_responses = "gpt-5.3-codex-spark|[pro]"

[memories]
# Phase-1：快速扫描，使用 Sonnet
phase_1_model = "claude-sonnet-4-6"

# Phase-2：深度分析，使用 Opus
phase_2_model = "claude-opus-4-6"

# Entire：决策分析，使用 Opus
entire_summary_model = "claude-opus-4-6"
```

### 成本优化配置

平衡性能和成本：

```toml
# 主模型：标准模型
model = "claude-sonnet-4-6"

# 工具模型：使用相同模型减少切换开销
model_sub = "claude-sonnet-4-6"

[memories]
# 所有继承自 model_sub，减少配置复杂度
```

---

## 运行时切换命令

| 模型类型 | Slash 命令 | 配置路径 |
|---------|-----------|---------|
| 主模型 | `/model` | `model` |
| 工具模型 | `/model-sub` | `model_sub` |
| Responses 工具 | `/model-sub-responses` | `model_sub_responses` |
| Entire 总结 | `/model-entire` | `memories.entire_summary_model` |

Memory phase-1/phase-2 目前只能通过配置文件修改。

---

## 继承关系图

```
┌─────────────────────────────────────────────────────────────┐
│                         model (主模型)                        │
│                   独立配置，不继承其他模型                      │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                      model_sub (工具模型)                     │
│                   作为其他专用模型的继承源                      │
└──────────────────────────┬──────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┬─────────────────┐
           │               │               │                 │
           ▼               ▼               ▼                 ▼
    ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
    │model_sub_   │ │memories.    │ │memories.    │ │memories.    │
    │responses    │ │phase_1_model│ │phase_2_model│ │entire_      │
    │             │ │             │ │             │ │summary_model│
    └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘
```

---

## 选择建议

### 何时使用 Opus？

- **主模型**: 复杂架构设计、难以调试的问题
- **Phase-2**: 大型代码库的深度分析
- **Entire summary**: 重要决策的详细记录

### 何时使用 Sonnet？

- **主模型**: 日常开发任务、快速迭代
- **工具模型**: 所有子任务和工具调用
- **Phase-1**: 快速代码库扫描
- **Entire summary**: 常规会话记录

### 何时使用专用模型（如 gpt-5.3-codex-spark）？

- **Responses 工具**: 需要特定协议优化时
- **特殊场景**: 模型有特定优势的任务

---

## 查看当前配置

```bash
# 查看所有模型配置
codex /status

# 查看配置文件
cat ~/.codex/config.toml
```

---

## 常见问题

**Q: 为什么有这么多模型配置？**

A: 不同任务有不同的性能/成本权衡。专用模型让你可以精细控制每个场景。

**Q: 我需要配置所有模型吗？**

A: 不需要。只配置 `model` 和 `model_sub`，其他会自动继承。

**Q: 如何知道哪个模型在执行任务？**

A: 查看 `/status` 面板，或在日志中查看模型调用信息。

**Q: 可以为不同项目使用不同模型吗？**

A: 可以。使用 `[projects]` 配置或 profile 功能。

**Q: 继承链是如何工作的？**

A: 如果专用模型未配置，会依次查找 `model_sub` → 内置默认值。

---

**相关文档**:
- `docs/entire-integration.md` - Entire 集成详细说明
- `config-examples/config.toml` - 配置示例
