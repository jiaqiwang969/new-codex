# 模型配置设计分析：重合问题与改进建议

## 当前问题

从状态面板可以看到：

```
Model:                claude-sonnet-4-6
Utility/sub-agent:    claude-sonnet-4-6
Resp. util model:     gpt-5.3-codex-spark|[pro]
Memory phase-1:       claude-sonnet-4-6 (config.model_sub)
Memory phase-2:       claude-sonnet-4-6 (config.model_sub)
Entire summary:       claude-sonnet-4-6 (config.model_sub)
```

**问题 1**: Memory phase-1, phase-2, Entire summary 都继承自 model_sub，显示相同的模型
**问题 2**: 用户无法清晰区分这些模型的用途
**问题 3**: 继承关系导致混淆，用户不知道哪些是独立配置，哪些是继承的

---

## 用途分析

让我们从实际使用场景来分析这些模型：

### 1. Model (主模型)
- **用途**: 用户对话、主要代码生成
- **独立性**: ✅ 完全独立
- **配置**: `model = "claude-sonnet-4-6"`
- **命令**: `/model`

### 2. Utility/sub-agent (工具/子代理)
- **用途**: spawn_agent 创建的子代理（explorer, worker）
- **独立性**: ✅ 完全独立
- **配置**: `model_sub = "claude-sonnet-4-6"`
- **命令**: `/model-sub`

### 3. Resp. util model (Responses 工具)
- **用途**: Responses API 内部工具调用（memory trace summarization）
- **独立性**: ⚠️ 技术性继承（必须是 OpenAI 模型）
- **配置**: `model_sub_responses = "gpt-5.3-codex-spark"`
- **命令**: `/model-sub-responses`
- **特殊性**: 协议限制，必须是 OpenAI 兼容模型

### 4. Memory phase-1
- **用途**: 快速扫描代码库，提取结构信息
- **独立性**: ❓ 当前继承自 model_sub
- **配置**: `[memories] phase_1_model = "..."`
- **命令**: ❌ 无（只能通过配置文件）

### 5. Memory phase-2
- **用途**: 深度代码分析，生成高质量摘要
- **独立性**: ❓ 当前继承自 model_sub
- **配置**: `[memories] phase_2_model = "..."`
- **命令**: ❌ 无（只能通过配置文件）

### 6. Entire summary
- **用途**: 为 Entire checkpoint 生成 WHY-focused 总结
- **独立性**: ❓ 当前继承自 model_sub
- **配置**: `[memories] entire_summary_model = "..."`
- **命令**: `/model-entire`

---

## 重合分析

### 是否有功能重合？

**Memory phase-1 vs Memory phase-2**:
- ❌ **无重合** - 两阶段处理，phase-1 快速扫描，phase-2 深度分析
- 但都是 Memory 系统的一部分

**Memory phase-1/2 vs Entire summary**:
- ❌ **无重合** - Memory 是代码库分析，Entire 是会话历史分析
- 完全不同的数据源和目的

**Utility/sub-agent vs Memory/Entire**:
- ❌ **无重合** - sub-agent 是执行任务，Memory/Entire 是生成摘要
- 不同的使用场景

### 继承关系是否合理？

**当前继承链**:
```
model_sub (工具模型)
    ├─ Memory phase-1 (代码扫描)
    ├─ Memory phase-2 (代码分析)
    └─ Entire summary (会话分析)
```

**问题**:
1. **语义不匹配**: model_sub 是"子代理模型"，但 Memory/Entire 不是子代理
2. **用户困惑**: 用户设置 model_sub 是为了子代理，不是为了 Memory/Entire
3. **缺乏独立性**: Entire summary 应该是独立的功能，不应该依赖 model_sub

---

## 改进建议

### 方案 1: 完全独立（推荐）

每个模型都有独立的配置，不继承：

```toml
# 主模型
model = "claude-sonnet-4-6"

# 子代理模型
model_sub = "claude-sonnet-4-6"

# Responses API 工具模型（技术性继承，保持现状）
model_sub_responses = "gpt-5.3-codex-spark"

[memories]
# Memory 系统模型（独立配置）
phase_1_model = "claude-sonnet-4-6"
phase_2_model = "claude-opus-4-6"

# Entire 总结模型（独立配置）
entire_summary_model = "claude-opus-4-6"
```

**优点**:
- ✅ 清晰明确，每个模型的用途一目了然
- ✅ 用户可以为每个场景选择最合适的模型
- ✅ 没有隐式继承，避免混淆

**缺点**:
- ❌ 配置项较多
- ❌ 用户需要为每个模型做选择

### 方案 2: 分组继承

按功能分组，每组有自己的默认模型：

```toml
# 主模型
model = "claude-sonnet-4-6"

# 子代理模型
model_sub = "claude-sonnet-4-6"

# Responses API 工具模型
model_sub_responses = "gpt-5.3-codex-spark"

[memories]
# Memory 系统默认模型
default_model = "claude-sonnet-4-6"
phase_1_model = "..."  # 可选，继承自 memories.default_model
phase_2_model = "..."  # 可选，继承自 memories.default_model

[entire]
# Entire 系统默认模型
default_model = "claude-opus-4-6"
summary_model = "..."  # 可选，继承自 entire.default_model
```

**优点**:
- ✅ 分组清晰，语义明确
- ✅ 可以为每个功能组设置默认值
- ✅ 减少配置项，同时保持灵活性

**缺点**:
- ❌ 配置结构更复杂
- ❌ 需要重构现有代码

### 方案 3: 智能默认值（最简单）

保持当前结构，但改变默认值逻辑：

```toml
# 主模型
model = "claude-sonnet-4-6"

# 子代理模型
model_sub = "claude-sonnet-4-6"

# Responses API 工具模型
model_sub_responses = "gpt-5.3-codex-spark"

[memories]
# 如果不配置，使用内置的智能默认值（不继承 model_sub）
phase_1_model = "..."  # 默认: claude-sonnet-4-6 (内置)
phase_2_model = "..."  # 默认: claude-sonnet-4-6 (内置)
entire_summary_model = "..."  # 默认: claude-sonnet-4-6 (内置)
```

**继承逻辑**:
```rust
// 当前（有问题）
entire_summary_model → model_sub → 内置默认

// 改进后
entire_summary_model → 内置默认（不继承 model_sub）
```

**优点**:
- ✅ 最小改动
- ✅ 避免 model_sub 的语义混淆
- ✅ 用户可以选择性配置

**缺点**:
- ❌ 失去了统一配置的便利性

---

## 推荐方案

### 短期（最小改动）: 方案 3

**改动**:
1. 移除 Memory phase-1/2/Entire 对 model_sub 的继承
2. 使用内置默认值（如 `DEFAULT_MEMORY_MODEL = "claude-sonnet-4-6"`）
3. 更新文档，明确每个模型的独立性

**代码改动**:
```rust
// core/src/config/types.rs
pub const DEFAULT_MEMORY_PHASE_1_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_MEMORY_PHASE_2_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_ENTIRE_SUMMARY_MODEL: &str = "claude-sonnet-4-6";

impl MemoriesConfig {
    pub fn effective_phase_1_model(&self) -> &str {
        self.phase_1_model.as_deref()
            .unwrap_or(DEFAULT_MEMORY_PHASE_1_MODEL)  // 不再继承 model_sub
    }
    
    pub fn effective_entire_summary_model(&self) -> &str {
        self.entire_summary_model.as_deref()
            .unwrap_or(DEFAULT_ENTIRE_SUMMARY_MODEL)  // 不再继承 model_sub
    }
}
```

### 长期（最佳设计）: 方案 1

完全独立的配置，每个模型都有明确的用途和默认值。

---

## Slash 命令设计

### 当前状态

| 模型 | Slash 命令 | 状态 |
|------|-----------|------|
| Model | `/model` | ✅ 存在 |
| Utility/sub-agent | `/model-sub` | ✅ 存在 |
| Resp. util model | `/model-sub-responses` | ✅ 存在 |
| Memory phase-1 | ❌ 无 | ❌ 缺失 |
| Memory phase-2 | ❌ 无 | ❌ 缺失 |
| Entire summary | `/model-entire` | ✅ 存在 |

### 建议

**选项 A: 添加所有命令**
```
/model
/model-sub
/model-sub-responses
/model-memory-phase1
/model-memory-phase2
/model-entire
```

**选项 B: 简化命令（推荐）**
```
/model              # 主模型
/model-sub          # 子代理
/model-responses    # Responses API（重命名，去掉 sub）
/model-memory       # Memory 系统（统一配置 phase-1 和 phase-2）
/model-entire       # Entire 总结
```

---

## 总结

**核心问题**: 
- Memory phase-1/2/Entire 继承自 model_sub 导致语义混淆
- model_sub 应该只用于子代理，不应该是其他模型的继承源

**推荐改进**:
1. **短期**: 移除继承关系，使用独立的内置默认值
2. **长期**: 完全独立的配置，每个模型都有明确的用途
3. **命令**: 简化 slash 命令，使用更清晰的命名

**下一步**:
1. 确认改进方案
2. 实现代码改动
3. 更新文档和测试
4. 更新状态面板显示逻辑
