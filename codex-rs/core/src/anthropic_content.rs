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

pub(crate) fn build_anthropic_messages(input: &[ResponseItem]) -> Vec<AnthropicMessage> {
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
