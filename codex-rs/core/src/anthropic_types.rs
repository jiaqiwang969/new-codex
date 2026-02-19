//! Anthropic Messages API request/response types.

use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

// ── Request types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicRequest {
    pub model: String,
    pub max_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinking>,
    pub stream: bool,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub(crate) struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicRequestContentBlock>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicRequestContentBlock {
    Text {
        text: String,
    },
    Image {
        source: AnthropicImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicThinking {
    #[serde(rename = "type")]
    pub thinking_type: String,
    pub budget_tokens: i64,
}

// ── Streaming event payload types ────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessageStartEvent {
    pub message: AnthropicMessageStartData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessageStartData {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicContentBlockStartEvent {
    pub index: usize,
    pub content_block: AnthropicContentBlockStart,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicContentBlockStart {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    Thinking {
        thinking: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicContentBlockDeltaEvent {
    pub index: usize,
    pub delta: AnthropicContentBlockDelta,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicContentBlockDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicContentBlockStopEvent {
    pub index: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicMessageDeltaEvent {
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
    #[serde(default)]
    pub cache_read_input_tokens: i64,
}

impl From<AnthropicUsage> for TokenUsage {
    fn from(usage: AnthropicUsage) -> Self {
        let cached_input_tokens = usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
        let total_tokens = usage.input_tokens + usage.output_tokens;
        TokenUsage {
            input_tokens: usage.input_tokens,
            cached_input_tokens,
            output_tokens: usage.output_tokens,
            reasoning_output_tokens: 0,
            total_tokens,
        }
    }
}
