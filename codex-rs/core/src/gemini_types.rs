//! Gemini API request/response types.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use codex_protocol::protocol::TokenUsage;

// ── Request types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContentRequest>,
    pub contents: Vec<GeminiContentRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GeminiToolConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<GeminiSafetySetting>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiToolConfig {
    pub function_calling_config: GeminiFunctionCallingConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFunctionCallingConfig {
    pub mode: GeminiFunctionCallingMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_function_call_arguments: Option<bool>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GeminiFunctionCallingMode {
    #[allow(dead_code)]
    None,
    Auto,
    Any,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum GeminiMediaResolution {
    #[serde(rename = "media_resolution_low")]
    Low,
    #[serde(rename = "media_resolution_medium")]
    Medium,
    #[serde(rename = "media_resolution_high")]
    High,
    #[serde(rename = "media_resolution_ultra_high")]
    UltraHigh,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GeminiImageSize {
    #[default]
    #[serde(rename = "1K")]
    Size1K,
    #[serde(rename = "2K")]
    Size2K,
    #[serde(rename = "4K")]
    Size4K,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GeminiAspectRatio {
    #[default]
    #[serde(rename = "1:1")]
    Square,
    #[serde(rename = "16:9")]
    Landscape,
    #[serde(rename = "9:16")]
    Portrait,
    #[serde(rename = "4:3")]
    Standard,
    #[serde(rename = "3:4")]
    StandardPortrait,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiImageConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<GeminiImageSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<GeminiAspectRatio>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GeminiResponseModality {
    #[serde(rename = "TEXT")]
    Text,
    #[serde(rename = "IMAGE")]
    Image,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<GeminiMediaResolution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<GeminiResponseModality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<GeminiImageConfig>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
pub(crate) enum GeminiHarmCategory {
    HarmCategoryHarassment,
    HarmCategoryHateSpeech,
    HarmCategorySexuallyExplicit,
    HarmCategoryDangerousContent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code, clippy::enum_variant_names)]
pub(crate) enum GeminiHarmBlockThreshold {
    BlockNone,
    BlockOnlyHigh,
    BlockMediumAndAbove,
    BlockLowAndAbove,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiSafetySetting {
    pub category: GeminiHarmCategory,
    pub threshold: GeminiHarmBlockThreshold,
}

// ── Content types (used in both request and response) ────────────────

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<GeminiPartRequest>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiPartRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GeminiInlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCallPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponsePart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "thought_signature")]
    pub compat_thought_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFunctionCallPart {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFunctionResponsePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<GeminiPartRequest>>,
}

// ── Response types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub response_id: Option<String>,
    pub usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(default)]
    pub error: Option<GeminiErrorResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiErrorResponse {
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<Value>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiCandidate {
    pub content: Option<GeminiContentResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiContentResponse {
    pub parts: Option<Vec<GeminiPartResponse>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiPartResponse {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(rename = "inlineData", default)]
    pub inline_data: Option<GeminiInlineData>,
    #[serde(rename = "functionCall", default, alias = "function_call")]
    pub function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "thoughtSignature", default)]
    pub thought_signature: Option<String>,
    #[serde(default)]
    pub thought: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

// ── Tool definition types ────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiTool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_declarations: Option<Vec<GeminiFunctionDeclaration>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "google_search")]
    pub google_search: Option<GeminiGoogleSearchTool>,
}

#[derive(Debug, Serialize, Default)]
pub(crate) struct GeminiGoogleSearchTool {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

// ── Usage metadata ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiUsageMetadata {
    pub prompt_token_count: Option<i64>,
    pub candidates_token_count: Option<i64>,
    pub total_token_count: Option<i64>,
    pub thoughts_token_count: Option<i64>,
}

impl From<GeminiUsageMetadata> for TokenUsage {
    fn from(meta: GeminiUsageMetadata) -> Self {
        let input = meta.prompt_token_count.unwrap_or_default();
        let output = meta.candidates_token_count.unwrap_or_default();
        let reasoning = meta.thoughts_token_count.unwrap_or_default();
        let total = meta.total_token_count.unwrap_or(input + output + reasoning);
        TokenUsage {
            input_tokens: input,
            cached_input_tokens: 0,
            output_tokens: output,
            reasoning_output_tokens: reasoning,
            total_tokens: total,
        }
    }
}

// ── Safety settings helper ───────────────────────────────────────────

pub(crate) fn default_safety_settings() -> Vec<GeminiSafetySetting> {
    vec![
        GeminiSafetySetting {
            category: GeminiHarmCategory::HarmCategoryHarassment,
            threshold: GeminiHarmBlockThreshold::BlockOnlyHigh,
        },
        GeminiSafetySetting {
            category: GeminiHarmCategory::HarmCategoryHateSpeech,
            threshold: GeminiHarmBlockThreshold::BlockOnlyHigh,
        },
        GeminiSafetySetting {
            category: GeminiHarmCategory::HarmCategorySexuallyExplicit,
            threshold: GeminiHarmBlockThreshold::BlockOnlyHigh,
        },
        GeminiSafetySetting {
            category: GeminiHarmCategory::HarmCategoryDangerousContent,
            threshold: GeminiHarmBlockThreshold::BlockOnlyHigh,
        },
    ]
}
