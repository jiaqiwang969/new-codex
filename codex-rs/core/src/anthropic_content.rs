//! Anthropic content building: converts internal `ResponseItem` transcripts into
//! the Anthropic Messages API request format and builds tool declarations.

use serde_json::Value;
use tracing::debug;

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

use crate::anthropic_types::AnthropicImageSource;
use crate::anthropic_types::AnthropicMessage;
use crate::anthropic_types::AnthropicRequestContentBlock;
use crate::anthropic_types::AnthropicTool;
use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;

pub(crate) fn normalize_anthropic_base_url(base_url: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        std::borrow::Cow::Borrowed(trimmed)
    } else {
        std::borrow::Cow::Owned(format!("{trimmed}/v1"))
    }
}

/// Extracts memory citation requirements from developer instructions.
/// Returns the extracted text if found, otherwise None.
fn extract_memory_citation_requirements(text: &str) -> Option<String> {
    // Look for the memory citation section in developer instructions
    if let Some(start_idx) = text.find("Memory citation requirements:") {
        // Find the end of this section (next major section or end of text)
        let section_text = &text[start_idx..];

        // Extract until we hit a blank line followed by a header or end of text
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

/// Builds Anthropic messages from ResponseItems and extracts memory citation requirements
/// from developer instructions to be added to the system prompt.
pub(crate) fn build_anthropic_messages_and_extract_memory_requirements(
    input: &[ResponseItem],
) -> (Vec<AnthropicMessage>, Option<String>) {
    let mut out: Vec<AnthropicMessage> = Vec::new();
    let mut memory_citation_requirements: Option<String> = None;

    let mut push_blocks = |role: &str, blocks: Vec<AnthropicRequestContentBlock>| {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut()
            && last.role == role
        {
            last.content.extend(blocks);
        } else {
            out.push(AnthropicMessage {
                role: role.to_string(),
                content: blocks,
            });
        }
    };

    for item in input {
        match item {
            ResponseItem::Message { role, content, .. } => match role.as_str() {
                "user" | "developer" => {
                    // Anthropic has no "developer" role; map it to "user" so that
                    // developer instructions (memory prompts, permissions, etc.)
                    // are delivered to the model.
                    let blocks = content
                        .iter()
                        .filter_map(|item| match item {
                            ContentItem::InputText { text } => {
                                Some(AnthropicRequestContentBlock::Text { text: text.clone() })
                            }
                            ContentItem::InputImage { image_url } => {
                                parse_image_url_to_anthropic_block(image_url)
                            }
                            ContentItem::OutputText { .. } => None,
                        })
                        .collect::<Vec<_>>();
                    push_blocks("user", blocks);
                }
                "assistant" => {
                    let blocks = content
                        .iter()
                        .filter_map(|item| match item {
                            ContentItem::OutputText { text } => {
                                Some(AnthropicRequestContentBlock::Text { text: text.clone() })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    push_blocks("assistant", blocks);
                }
                other => {
                    debug!("dropping message role {other} for Anthropic conversion");
                }
            },
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                let input = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| {
                    serde_json::json!({
                        "_raw": arguments,
                    })
                });
                push_blocks(
                    "assistant",
                    vec![AnthropicRequestContentBlock::ToolUse {
                        id: call_id.clone(),
                        name: name.clone(),
                        input,
                    }],
                );
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                push_blocks(
                    "user",
                    vec![AnthropicRequestContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: output.to_string(),
                        is_error: output.success.map(|success| !success),
                    }],
                );
            }
            _ => {}
        }
    }

    // Extract memory citation requirements from developer messages
    for item in input {
        if let ResponseItem::Message { role, content, .. } = item
            && role == "developer"
        {
            for content_item in content {
                if let ContentItem::InputText { text } = content_item
                    && let Some(requirements) = extract_memory_citation_requirements(text)
                {
                    memory_citation_requirements = Some(requirements);
                    break;
                }
            }
        }
    }

    (out, memory_citation_requirements)
}

#[allow(dead_code)]
fn old_build_anthropic_messages(input: &[ResponseItem]) -> Vec<AnthropicMessage> {
    let mut out: Vec<AnthropicMessage> = Vec::new();

    let mut push_blocks = |role: &str, blocks: Vec<AnthropicRequestContentBlock>| {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut()
            && last.role == role
        {
            last.content.extend(blocks);
        } else {
            out.push(AnthropicMessage {
                role: role.to_string(),
                content: blocks,
            });
        }
    };

    for item in input {
        match item {
            ResponseItem::Message { role, content, .. } => match role.as_str() {
                "user" | "developer" => {
                    // Anthropic has no "developer" role; map it to "user" so that
                    // developer instructions (memory prompts, permissions, etc.)
                    // are delivered to the model.
                    let blocks = content
                        .iter()
                        .filter_map(|item| match item {
                            ContentItem::InputText { text } => {
                                Some(AnthropicRequestContentBlock::Text { text: text.clone() })
                            }
                            ContentItem::InputImage { image_url } => {
                                parse_image_url_to_anthropic_block(image_url)
                            }
                            ContentItem::OutputText { .. } => None,
                        })
                        .collect::<Vec<_>>();
                    push_blocks("user", blocks);
                }
                "assistant" => {
                    let blocks = content
                        .iter()
                        .filter_map(|item| match item {
                            ContentItem::OutputText { text } => {
                                Some(AnthropicRequestContentBlock::Text { text: text.clone() })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    push_blocks("assistant", blocks);
                }
                other => {
                    debug!("dropping message role {other} for Anthropic conversion");
                }
            },
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                let input = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| {
                    serde_json::json!({
                        "_raw": arguments,
                    })
                });
                push_blocks(
                    "assistant",
                    vec![AnthropicRequestContentBlock::ToolUse {
                        id: call_id.clone(),
                        name: name.clone(),
                        input,
                    }],
                );
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                push_blocks(
                    "user",
                    vec![AnthropicRequestContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: output.to_string(),
                        is_error: output.success.map(|success| !success),
                    }],
                );
            }
            _ => {}
        }
    }

    out
}

pub(crate) fn build_anthropic_tools(tools: &[ToolSpec]) -> Option<Vec<AnthropicTool>> {
    let mut out = Vec::new();
    for tool in tools {
        let ToolSpec::Function(ResponsesApiTool {
            name,
            description,
            parameters,
            ..
        }) = tool
        else {
            continue;
        };
        let input_schema = match serde_json::to_value(parameters) {
            Ok(value) => value,
            Err(_) => continue,
        };
        out.push(AnthropicTool {
            name: name.clone(),
            description: description.clone(),
            input_schema,
        });
    }

    (!out.is_empty()).then_some(out)
}

/// Parses a `data:` URL or HTTPS URL into an Anthropic image content block.
fn parse_image_url_to_anthropic_block(image_url: &str) -> Option<AnthropicRequestContentBlock> {
    let trimmed = image_url.trim();
    if trimmed.starts_with("data:") {
        // Parse data:image/<type>;base64,<data>
        let without_prefix = trimmed.strip_prefix("data:")?;
        let (meta, data) = without_prefix.split_once(',')?;
        let (mime, encoding) = meta.split_once(';')?;
        if !encoding.eq_ignore_ascii_case("base64") || mime.is_empty() || data.trim().is_empty() {
            return None;
        }
        Some(AnthropicRequestContentBlock::Image {
            source: AnthropicImageSource::Base64 {
                media_type: mime.to_string(),
                data: data.to_string(),
            },
        })
    } else if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        Some(AnthropicRequestContentBlock::Image {
            source: AnthropicImageSource::Url {
                url: trimmed.to_string(),
            },
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_memory_citation_requirements() {
        let text = r#"
## Memory

You have access to memory.

Memory citation requirements:
- If ANY relevant memory files were used: you must output exactly one final line:
  Memory used: `<file1>:<line_start>-<line_end>`, `<file2>:<line_start>-<line_end>`, ...
  - Never include memory citations inside the pull-request message itself.
  - Never cite blank lines; double-check ranges.
  - Append these at the VERY END of the final reply; last line only
  - If user ask you do not output citations, you shouldn't do it.

========= MEMORY_SUMMARY BEGINS =========
Some memory content here.
========= MEMORY_SUMMARY ENDS =========
"#;

        let result = extract_memory_citation_requirements(text);
        assert!(result.is_some());
        let requirements = result.unwrap();
        assert!(requirements.contains("Memory citation requirements:"));
        assert!(requirements.contains("Memory used:"));
        assert!(requirements.contains("VERY END of the final reply"));
    }

    #[test]
    fn test_extract_memory_citation_requirements_not_found() {
        let text = "Some text without memory citation requirements";
        let result = extract_memory_citation_requirements(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_anthropic_messages_extracts_memory_requirements() {
        let input = vec![
            ResponseItem::Message {
                id: None,
                role: "developer".to_string(),
                content: vec![ContentItem::InputText {
                    text: r#"
Memory citation requirements:
- If ANY relevant memory files were used: you must output exactly one final line:
  Memory used: `<file1>:<line_start>-<line_end>`
"#
                    .to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "Hello".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];

        let (messages, memory_requirements) =
            build_anthropic_messages_and_extract_memory_requirements(&input);

        // Both developer and user messages should be mapped to "user" role
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content.len(), 2);

        // Memory requirements should be extracted
        assert!(memory_requirements.is_some());
        let requirements = memory_requirements.unwrap();
        assert!(requirements.contains("Memory citation requirements:"));
        assert!(requirements.contains("Memory used:"));
    }
}
