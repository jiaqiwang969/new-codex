# 模型类型详细说明：区别与使用场景

## 核心问题：为什么需要这么多模型配置？

从代码分析来看，这些模型配置**并不重合**，而是针对不同的技术场景和 API 协议。

---

## 关键区别：model_sub vs model_sub_responses

### model_sub (工具/子代理模型)

**用途**: 子代理（explorer, worker）的默认模型

**使用场景**:
- `spawn_agent` 创建的子代理
- Explorer 角色：快速代码探索
- Worker 角色：独立任务执行
- 作为其他专用模型的继承源

**协议**: 任何协议（Anthropic / OpenAI / Gemini 等）

**代码证据**:
```rust
// core/src/tools/handlers/multi_agents.rs:315
let mut selected_model_sub = turn.config.model_sub.clone()
```

---

### model_sub_responses (Responses 工具模型)

**用途**: **仅用于 Responses API 的内部工具调用**

**关键区别**: 这是一个技术性的区分，不是功能性的区分！

**使用场景**:
- Memory trace summarization（记忆追踪总结）
- 当主模型不支持 Responses API 时的降级
- **必须是 OpenAI 兼容的模型**

**协议**: **仅 OpenAI Responses API**

**代码证据**:
```rust
// core/src/thread_memory.rs:315-319
// Memory trace summarization is a Responses-only endpoint.
let fallback_model_slug = utility_model::responses_utility_model_slug(config);
```

**继承逻辑**:
```rust
// core/src/utility_model.rs:99-111
pub(crate) fn responses_utility_model_slug(config: &Config) -> &str {
    config
        .model_sub_responses
        .as_deref()
        .filter(|model| is_openai_model_slug(model))  // 必须是 OpenAI 模型！
        .or_else(|| {
            config.model_sub.as_deref()
                .filter(|model| is_openai_model_slug(model))
        })
        .unwrap_or(DEFAULT_UTILITY_MODEL)  // "gpt-5.1-codex-mini"
}
```

---

## 对比表格

| 维度 | model_sub | model_sub_responses |
|------|-----------|---------------------|
| **用途** | 子代理的默认模型 | Responses API 内部工具调用 |
| **协议限制** | 任何协议 | **仅 OpenAI Responses API** |
| **使用场景** | spawn_agent, explorer, worker | Memory trace summarization |
| **模型限制** | 任何模型 | **必须是 OpenAI 兼容模型** |
| **继承关系** | 作为其他模型的继承源 | 继承自 model_sub（如果是 OpenAI 模型） |

---

## 为什么需要 model_sub_responses？

**技术原因**:

1. **协议不兼容**: Claude 主模型使用 Anthropic Messages API，不支持 Responses API 的某些端点（如 memory trace summarization）

2. **降级机制**: 当主模型不支持某个功能时，需要一个 Responses API 兼容的模型来处理

3. **性能优化**: Responses API 有特定的优化（如并行工具调用），某些任务用专门的模型更高效

**实际例子**:
```
场景: 你使用 Claude Sonnet 作为主模型

1. 用户发起对话 → 使用 model (claude-sonnet-4-6, Anthropic API)
2. 需要生成 memory trace summary → Claude 不支持这个 Responses API 端点
3. 系统降级到 model_sub_responses (gpt-5.3-codex-spark, Responses API)
4. 完成 memory summarization
```

---

## 其他模型类型

### Memory phase-1 & phase-2

**用途**: 记忆系统的两阶段处理
- Phase-1: 快速扫描代码库
- Phase-2: 深度分析

**继承**: 都继承自 `model_sub`

### Entire summary

**用途**: 为 Entire checkpoint 生成 WHY-focused 总结

**继承**: 继承自 `model_sub`

---

## 配置建议

### 场景 1: 纯 Claude 工作流

```toml
model = "claude-sonnet-4-6"
model_sub = "claude-sonnet-4-6"
model_sub_responses = "gpt-5.1-codex-mini"  # 仅用于 Responses API 降级

[memories]
# 所有继承自 model_sub
```

**说明**: 
- 主要工作用 Claude
- 子代理也用 Claude
- 只有需要 Responses API 特定功能时才用 OpenAI 模型

### 场景 2: 纯 OpenAI 工作流

```toml
model = "gpt-5.3-codex-spark|[pro]"
model_sub = "gpt-5.1-codex-mini"
# model_sub_responses 不需要配置，会继承 model_sub

[memories]
# 所有继承自 model_sub
```

**说明**:
- 全部使用 OpenAI 模型
- 不需要单独配置 model_sub_responses
- 所有功能都原生支持

### 场景 3: 混合工作流（推荐）

```toml
model = "claude-opus-4-6"                    # 主任务用最强模型
model_sub = "claude-sonnet-4-6"              # 子任务用快速模型
model_sub_responses = "gpt-5.3-codex-spark|[pro]"  # Responses API 专用

[memories]
phase_1_model = "claude-sonnet-4-6"          # 快速扫描
phase_2_model = "claude-opus-4-6"            # 深度分析
entire_summary_model = "claude-opus-4-6"     # 重要决策记录
```

---

## 常见问题

**Q: 为什么我的 model_sub 是 Claude，但 Resp. util model 显示 gpt-5.3-codex-spark？**

A: 因为 `model_sub_responses` 的继承逻辑会过滤掉非 OpenAI 模型，所以会降级到内置默认值。

**Q: 我需要配置 model_sub_responses 吗？**

A: 取决于你的主模型：
- 如果主模型是 Claude/Gemini → **建议配置**，用于 Responses API 降级
- 如果主模型是 OpenAI → **不需要**，会自动继承 model_sub

**Q: model_sub 和 model_sub_responses 会同时使用吗？**

A: 不会冲突，它们用于不同场景：
- `model_sub`: 子代理（spawn_agent）
- `model_sub_responses`: Responses API 内部工具调用

**Q: 为什么不统一用一个模型？**

A: 因为：
1. **协议限制**: 不同 API 协议不兼容
2. **性能优化**: 不同任务适合不同模型
3. **成本控制**: 简单任务用便宜模型，复杂任务用强大模型

---

## 技术细节：Responses API vs Messages API

### Anthropic Messages API
- Claude 模型的原生协议
- 支持工具调用、流式输出
- **不支持某些 Responses API 特定端点**

### OpenAI Responses API
- OpenAI 模型的协议
- 支持并行工具调用
- 有特定的优化端点（如 memory trace summarization）

### 为什么需要两者？

Codex 支持多种模型提供商，需要在不同协议之间无缝切换。当主模型不支持某个功能时，自动降级到兼容的模型。

---

**相关文档**:
- `docs/model-roles.md` - 模型角色概览
- `docs/entire-integration.md` - Entire 集成详细说明
