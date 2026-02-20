# Anthropic Memory Citation Enhancement

## Problem

GPT-5.3-codex 系列模型能够输出 "Memory used: ..." 引用，但 Claude 模型不能。

## Root Cause

1. **OpenAI (GPT-5.3-codex)**:
   - 支持原生的 `developer` 角色
   - GPT-5.3-codex 系列模型经过专门训练，能够识别并严格遵循 developer instructions 中的格式要求
   - Developer 角色在模型中可能有更高的权重或特殊处理

2. **Anthropic (Claude)**:
   - **不支持** `developer` 角色，将其映射为普通的 `user` 角色
   - Claude 模型将 memory citation 要求当作普通用户消息处理，而不是系统级指令
   - Claude 没有被专门训练来识别和遵循 "Memory used:" 这种特定的引用格式

## Solution

将 memory citation 要求从 developer instructions 中提取出来，添加到 Anthropic 的 system prompt 中。

### Implementation

1. **新增函数** (`core/src/anthropic_content.rs`):
   - `extract_memory_citation_requirements()`: 从 developer instructions 中提取 memory citation 要求
   - `build_anthropic_messages_and_extract_memory_requirements()`: 构建 Anthropic 消息并提取 memory citation 要求

2. **修改请求构建** (`core/src/client.rs`):
   - 使用新函数提取 memory citation 要求
   - 将提取的要求追加到 Anthropic 的 system prompt 中

### Code Changes

#### core/src/anthropic_content.rs

```rust
/// Extracts memory citation requirements from developer instructions.
fn extract_memory_citation_requirements(text: &str) -> Option<String> {
    if let Some(start_idx) = text.find("Memory citation requirements:") {
        let section_text = &text[start_idx..];
        let end_idx = section_text
            .find("\n\n=")
            .or_else(|| section_text.find("\n\nIf memory"))
            .unwrap_or(section_text.len());
        
        let citation_section = section_text[..end_idx].trim();
        if !citation_section.is_empty() {
            return Some(citation_section.to_string());
        }
    }
    None
}

/// Builds Anthropic messages and extracts memory citation requirements.
pub(crate) fn build_anthropic_messages_and_extract_memory_requirements(
    input: &[ResponseItem],
) -> (Vec<AnthropicMessage>, Option<String>) {
    // ... builds messages and extracts memory requirements from developer messages
}
```

#### core/src/client.rs

```rust
let (messages, memory_citation_requirements) = 
    build_anthropic_messages_and_extract_memory_requirements(&formatted_input);

// Build system prompt: base instructions + memory citation requirements
let mut instructions = prompt.base_instructions.text.trim().to_string();
if let Some(memory_requirements) = memory_citation_requirements {
    if !instructions.is_empty() {
        instructions.push_str("\n\n");
    }
    instructions.push_str("## Memory Citation Requirements\n\n");
    instructions.push_str(&memory_requirements);
}
```

## Testing

Added unit tests in `core/src/anthropic_content.rs`:
- `test_extract_memory_citation_requirements()`: 验证提取逻辑
- `test_extract_memory_citation_requirements_not_found()`: 验证未找到时的行为
- `test_build_anthropic_messages_extracts_memory_requirements()`: 验证完整流程

## Expected Behavior

使用 Claude 模型时，memory citation 要求会被添加到 system prompt 中，Claude 应该能够：
1. 识别这些要求
2. 在使用 memory 文件后输出 "Memory used: ..." 引用
3. 将引用放在回复的最后一行

## Notes

- 这个修改不影响 OpenAI 模型的行为
- Memory citation 要求仍然保留在 developer instructions 中（用于 OpenAI）
- 对于 Anthropic，这些要求会被额外添加到 system prompt 中以提高遵循率
