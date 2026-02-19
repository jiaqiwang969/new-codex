# 模型继承问题分析与解决方案

## 问题确认

### 当前继承逻辑

**代码证据**:

1. **Memory phase-1** (core/src/memories/phase1.rs:195-198):
```rust
let model_name = config.memories.phase_1_model.clone()
    .or_else(|| config.model_sub.clone())  // ← 继承自 model_sub
    .unwrap_or(phase_one::MODEL.to_string());
```

2. **Memory phase-2** (core/src/memories/phase2.rs:245-248):
```rust
config.memories.phase_2_model.clone()
    .or_else(|| config.model_sub.clone())  // ← 继承自 model_sub
    .unwrap_or(phase_two::MODEL.to_string())
```

3. **Entire summary** (tui/src/status/card.rs:432-437):
```rust
let entire_summary_model = resolve_memory_model_display(
    config.memories.entire_summary_model.as_deref(),
    config.model_sub.as_deref(),  // ← 继承自 model_sub
    codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL,
    "memories.entire_summary_model",
);
```

4. **resolve_memory_model_display** (tui/src/status/card.rs:880-900):
```rust
fn resolve_memory_model_display(
    explicit_model: Option<&str>,
    model_sub: Option<&str>,
    default_model: &str,
    explicit_source_label: &str,
) -> StatusMemoryModel {
    if let Some(model) = explicit_model {
        return StatusMemoryModel {
            model: model.to_string(),
            source_label: explicit_source_label.to_string(),
        };
    }
    
    if let Some(model) = model_sub {  // ← 继承自 model_sub
        return StatusMemoryModel {
            model: model.to_string(),
            source_label: "config.model_sub".to_string(),
        };
    }
    
    StatusMemoryModel {
        model: default_model.to_string(),
        source_label: "built-in default".to_string(),
    }
}
```

### 问题总结

**你的观察完全正确**：

1. ✅ **Memory phase-1, phase-2, Entire summary 都继承自 model_sub**
2. ✅ **这导致语义混淆**: model_sub 是"子代理模型"，但 Memory/Entire 不是子代理
3. ✅ **用户困惑**: 设置 model_sub 会影响 Memory/Entire，这不符合直觉
4. ✅ **缺乏独立性**: Entire 应该是独立功能，不应该依赖 model_sub

---

## 功能重合分析

### 是否有重合？

**答案: 没有功能重合，但有语义混淆**

| 模型 | 用途 | 是否重合 |
|------|------|---------|
| model_sub | 子代理执行（spawn_agent, explorer, worker） | - |
| Memory phase-1 | 代码库快速扫描 | ❌ 无重合 |
| Memory phase-2 | 代码库深度分析 | ❌ 无重合 |
| Entire summary | AI 会话历史分析 | ❌ 无重合 |

**问题不是功能重合，而是继承关系不合理**：
- model_sub 的语义是"子代理模型"
- 但 Memory/Entire 不是子代理，它们是后台分析任务
- 继承关系导致用户认为它们有关联，实际上没有

---

## 解决方案

### 推荐方案：移除继承，使用独立默认值

**原则**:
1. **每个模型都有明确的用途和独立的默认值**
2. **不再从 model_sub 继承**
3. **保持配置简单，用户可以选择性覆盖**

### 代码改动

#### 1. 定义独立的默认值 (core/src/config/types.rs)

```rust
// 已存在
pub const DEFAULT_MEMORY_PHASE_ONE_MODEL: &str = "gpt-5.1-codex-mini";
pub const DEFAULT_MEMORY_PHASE_TWO_MODEL: &str = "gpt-5.1-codex-mini";

// 需要添加
pub const DEFAULT_ENTIRE_SUMMARY_MODEL: &str = "claude-sonnet-4-6";
```

#### 2. 移除 Memory phase-1 的继承 (core/src/memories/phase1.rs:195-198)

**当前**:
```rust
let model_name = config.memories.phase_1_model.clone()
    .or_else(|| config.model_sub.clone())  // ← 移除这行
    .unwrap_or(phase_one::MODEL.to_string());
```

**改为**:
```rust
let model_name = config.memories.phase_1_model.clone()
    .unwrap_or_else(|| DEFAULT_MEMORY_PHASE_ONE_MODEL.to_string());
```

#### 3. 移除 Memory phase-2 的继承 (core/src/memories/phase2.rs:245-248)

**当前**:
```rust
config.memories.phase_2_model.clone()
    .or_else(|| config.model_sub.clone())  // ← 移除这行
    .unwrap_or(phase_two::MODEL.to_string())
```

**改为**:
```rust
config.memories.phase_2_model.clone()
    .unwrap_or_else(|| DEFAULT_MEMORY_PHASE_TWO_MODEL.to_string())
```

#### 4. 移除 Entire summary 的继承 (tui/src/status/card.rs)

**当前**:
```rust
let entire_summary_model = resolve_memory_model_display(
    config.memories.entire_summary_model.as_deref(),
    config.model_sub.as_deref(),  // ← 移除这个参数
    codex_core::DEFAULT_MEMORY_PHASE_TWO_MODEL,
    "memories.entire_summary_model",
);
```

**改为**:
```rust
let entire_summary_model = resolve_memory_model_display(
    config.memories.entire_summary_model.as_deref(),
    None,  // 不再继承
    codex_core::DEFAULT_ENTIRE_SUMMARY_MODEL,
    "memories.entire_summary_model",
);
```

#### 5. 更新 resolve_memory_model_display 函数签名

保持函数不变，但调用时传 `None` 作为 `model_sub` 参数。

或者创建新函数：
```rust
fn resolve_independent_model_display(
    explicit_model: Option<&str>,
    default_model: &str,
    explicit_source_label: &str,
) -> StatusMemoryModel {
    if let Some(model) = explicit_model {
        return StatusMemoryModel {
            model: model.to_string(),
            source_label: explicit_source_label.to_string(),
        };
    }
    
    StatusMemoryModel {
        model: default_model.to_string(),
        source_label: "built-in default".to_string(),
    }
}
```

---

## 改动后的效果

### 配置示例

```toml
model = "claude-sonnet-4-6"
model_sub = "claude-sonnet-4-6"
model_sub_responses = "gpt-5.3-codex-spark|[pro]"

[memories]
# 可选配置，不配置则使用独立的默认值
# phase_1_model = "..."  # 默认: gpt-5.1-codex-mini
# phase_2_model = "..."  # 默认: gpt-5.1-codex-mini
# entire_summary_model = "..."  # 默认: claude-sonnet-4-6
```

### 状态面板显示

**改动前**:
```
Model:                claude-sonnet-4-6
Utility/sub-agent:    claude-sonnet-4-6
Memory phase-1:       claude-sonnet-4-6 (config.model_sub)  ← 混淆
Memory phase-2:       claude-sonnet-4-6 (config.model_sub)  ← 混淆
Entire summary:       claude-sonnet-4-6 (config.model_sub)  ← 混淆
```

**改动后**:
```
Model:                claude-sonnet-4-6
Utility/sub-agent:    claude-sonnet-4-6
Memory phase-1:       gpt-5.1-codex-mini (built-in default)  ← 清晰
Memory phase-2:       gpt-5.1-codex-mini (built-in default)  ← 清晰
Entire summary:       claude-sonnet-4-6 (built-in default)   ← 清晰
```

或者用户显式配置：
```
Memory phase-1:       claude-sonnet-4-6 (memories.phase_1_model)
Memory phase-2:       claude-opus-4-6 (memories.phase_2_model)
Entire summary:       claude-opus-4-6 (memories.entire_summary_model)
```

---

## 优点

1. ✅ **语义清晰**: 每个模型都有独立的用途和默认值
2. ✅ **避免混淆**: model_sub 只用于子代理，不影响其他功能
3. ✅ **用户友好**: 用户可以清楚地看到每个模型的来源
4. ✅ **灵活性**: 用户可以为每个功能选择最合适的模型
5. ✅ **向后兼容**: 现有配置仍然有效，只是默认行为改变

---

## 需要改动的文件

1. `core/src/config/types.rs` - 添加 DEFAULT_ENTIRE_SUMMARY_MODEL
2. `core/src/memories/phase1.rs` - 移除 model_sub 继承
3. `core/src/memories/phase2.rs` - 移除 model_sub 继承
4. `tui/src/status/card.rs` - 更新显示逻辑
5. `tui/src/app.rs` - 更新 /model-entire 命令的继承逻辑
6. `docs/model-roles.md` - 更新文档
7. `config-examples/config.toml` - 更新示例

---

## 测试需要更新

1. `core/tests/entire_config_test.rs` - 更新继承测试
2. `tui/src/status/tests.rs` - 更新状态显示测试

---

## 下一步

1. 确认这个方案是否符合你的预期
2. 实现代码改动
3. 更新测试
4. 更新文档
5. 提交 commit
