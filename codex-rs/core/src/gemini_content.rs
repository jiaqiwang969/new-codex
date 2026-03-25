//! Gemini content building: converts internal `ResponseItem` transcripts into
//! the Gemini API content format, builds tool declarations, and manages thought
//! signatures.

use std::borrow::Cow;
use std::collections::HashMap;

use serde_json::Value;
use tracing::debug;

use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;

use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::gemini_types::*;
use crate::model_compat::is_gemma_model_slug;

// ── Constants ────────────────────────────────────────────────────────

const SYNTHETIC_THOUGHT_SIGNATURE: &str = "context_engineering_is_the_way_to_go";
const DEFAULT_GEMINI_THINKING_BUDGET: i32 = 8192;

const GEMINI_READ_ONLY_TOOL_NAMES: [&str; 7] = [
    "grep_files",
    "list_dir",
    "read_file",
    "list_mcp_resources",
    "list_mcp_resource_templates",
    "read_mcp_resource",
    "view_image",
];

// If structured repo tools are unavailable, fall back to shell-style tools so
// Gemini can still gather local context. These tools can access the network, so
// they are intentionally excluded from `GEMINI_READ_ONLY_TOOL_NAMES`.
const GEMINI_FALLBACK_READ_ONLY_TOOL_NAMES: [&str; 3] = ["exec_command", "shell", "shell_command"];

const GEMMA_STABLE_TOOL_NAMES: [&str; 8] = [
    "shell_command",
    "exec_command",
    "write_stdin",
    "grep_files",
    "list_dir",
    "read_file",
    "update_plan",
    "view_image",
];

// ── URL helpers ──────────────────────────────────────────────────────

pub(crate) fn normalize_gemini_base_url(base_url: &str) -> Cow<'_, str> {
    let trimmed = base_url.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/v1") {
        Cow::Owned(format!("{prefix}/v1beta"))
    } else {
        Cow::Borrowed(trimmed)
    }
}

// ── Model helpers ────────────────────────────────────────────────────

pub(crate) fn is_gemini_3_model(api_model: &str) -> bool {
    api_model.starts_with("gemini-3") || api_model.starts_with("gemma-3")
}

fn supports_multiple_inline_images(api_model: &str) -> bool {
    api_model.starts_with("gemini-3")
}

/// Strip Gemini-specific suffixes from the model slug to get the upstream
/// API model name.
pub(crate) fn strip_model_suffix(model: &str) -> &str {
    let m = model.strip_suffix("-codex").unwrap_or(model);
    let m = m.strip_suffix("-germini").unwrap_or(m);
    let m = m.strip_suffix("-gemini").unwrap_or(m);
    let m = m.strip_prefix("google/").unwrap_or(m);
    // Strip antigravity prefix for CLIProxyAPI models
    let m = m.strip_prefix("antigravity/").unwrap_or(m);

    (m.strip_prefix("antigravity-gemini/").unwrap_or(m)) as _
}

// ── Thinking config ──────────────────────────────────────────────────

pub(crate) fn build_gemini_thinking_config(
    api_model: &str,
    reasoning_effort: Option<ReasoningEffortConfig>,
) -> Option<GeminiThinkingConfig> {
    if api_model.contains("image") {
        return None;
    }

    if is_gemini_3_model(api_model) {
        let thinking_level = match reasoning_effort {
            Some(ReasoningEffortConfig::XHigh | ReasoningEffortConfig::High) => "high",
            Some(ReasoningEffortConfig::Medium) => "medium",
            Some(
                ReasoningEffortConfig::Low
                | ReasoningEffortConfig::Minimal
                | ReasoningEffortConfig::None,
            ) => {
                if api_model.contains("flash") {
                    "minimal"
                } else {
                    "low"
                }
            }
            None => "high",
        };
        return Some(GeminiThinkingConfig {
            thinking_level: Some(thinking_level.to_string()),
            include_thoughts: Some(true),
            thinking_budget: None,
        });
    }

    // Gemini 2.5 and other models: use thinkingBudget
    Some(GeminiThinkingConfig {
        thinking_level: None,
        include_thoughts: matches!(
            reasoning_effort,
            Some(ReasoningEffortConfig::High | ReasoningEffortConfig::XHigh)
        )
        .then_some(true),
        thinking_budget: Some(DEFAULT_GEMINI_THINKING_BUDGET),
    })
}

// ── Tool config ──────────────────────────────────────────────────────

pub(crate) fn build_gemini_tool_config(
    tools: &[ToolSpec],
    formatted_input: &[ResponseItem],
    api_model: &str,
) -> GeminiFunctionCallingConfig {
    let is_gemma_model = is_gemma_model_slug(api_model);
    let last_user_text = last_user_input_text(formatted_input);
    let force_read_first = std::env::var("CODEX_GEMINI_FORCE_READ_TOOLS_FIRST_TURN")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        });

    let is_first_turn_with_user_text = !formatted_input.is_empty()
        && formatted_input.iter().all(|item| {
            matches!(
                item,
                ResponseItem::Message {
                    role, content, .. } if role == "user"
                    && content.iter().any(|c| matches!(c, ContentItem::InputText { .. }))
            )
        });

    let should_force = match force_read_first {
        Some(value) => value,
        None => {
            is_first_turn_with_user_text
                && last_user_text
                    .as_deref()
                    .is_some_and(gemini_request_needs_local_context)
        }
    };

    let stream_fn_args = if is_gemini_3_model(api_model) && !is_gemma_model {
        Some(true)
    } else {
        None
    };

    if is_gemma_model {
        // Local Gemma servers frequently emit "tool intent" text without a real
        // functionCall payload. Keep function-calling in AUTO mode to avoid
        // hard-forcing tool paths that cannot be executed reliably.
        return GeminiFunctionCallingConfig {
            mode: GeminiFunctionCallingMode::Auto,
            allowed_function_names: None,
            stream_function_call_arguments: None,
        };
    }

    if should_force {
        let mut allowed: Vec<String> = tools
            .iter()
            .filter_map(|t| match t {
                ToolSpec::Function(f) if GEMINI_READ_ONLY_TOOL_NAMES.contains(&f.name.as_str()) => {
                    Some(f.name.clone())
                }
                _ => None,
            })
            .collect();

        if allowed.is_empty() {
            allowed = tools
                .iter()
                .filter_map(|t| match t {
                    ToolSpec::Function(f)
                        if GEMINI_FALLBACK_READ_ONLY_TOOL_NAMES.contains(&f.name.as_str()) =>
                    {
                        Some(f.name.clone())
                    }
                    _ => None,
                })
                .collect();
        }

        if !allowed.is_empty() {
            return GeminiFunctionCallingConfig {
                mode: GeminiFunctionCallingMode::Any,
                allowed_function_names: Some(allowed),
                stream_function_call_arguments: stream_fn_args,
            };
        }
    }

    GeminiFunctionCallingConfig {
        mode: GeminiFunctionCallingMode::Auto,
        allowed_function_names: None,
        stream_function_call_arguments: stream_fn_args,
    }
}

fn last_user_input_text(input: &[ResponseItem]) -> Option<String> {
    for item in input.iter().rev() {
        let ResponseItem::Message { role, content, .. } = item else {
            continue;
        };
        if role != "user" {
            continue;
        }
        let text: String = content
            .iter()
            .filter_map(|item| match item {
                ContentItem::InputText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        return Some(text.to_string());
    }
    None
}

fn gemini_request_needs_local_context(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let needs_repo_context = [
        "this repo",
        "this repository",
        "this codebase",
        "this project",
        "current repo",
        "current project",
        "repo",
        "repository",
        "codebase",
        "workspace",
        "analyze project",
        "analyse project",
        "analyze the project",
        "analyse the project",
        "debug",
        "fix",
        "bug",
        "stack trace",
        "traceback",
        "cargo",
        "rust",
        ".rs",
        "cargo.toml",
    ]
    .into_iter()
    .any(|needle| lower.contains(needle));

    needs_repo_context || prompt.contains('/') || prompt.contains('\\')
}

// ── Tool declarations ────────────────────────────────────────────────

fn strip_additional_properties(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("additionalProperties");
            for v in map.values_mut() {
                strip_additional_properties(v);
            }
        }
        Value::Array(items) => {
            for v in items {
                strip_additional_properties(v);
            }
        }
        _ => {}
    }
}

pub(crate) fn build_gemini_tools(tools: &[ToolSpec], api_model: &str) -> Option<Vec<GeminiTool>> {
    let filter_for_gemma = is_gemma_model_slug(api_model);
    let enable_google_search = !filter_for_gemma
        && tools
            .iter()
            .any(|tool| matches!(tool, ToolSpec::WebSearch { .. }));
    let mut functions = Vec::new();
    let mut filtered_out = 0usize;
    for tool in tools {
        if let ToolSpec::Function(ResponsesApiTool {
            name,
            description,
            parameters,
            ..
        }) = tool
        {
            if filter_for_gemma && !GEMMA_STABLE_TOOL_NAMES.contains(&name.as_str()) {
                filtered_out += 1;
                continue;
            }
            let params = serde_json::to_value(parameters).ok().map(|mut v| {
                strip_additional_properties(&mut v);
                v
            });
            functions.push(GeminiFunctionDeclaration {
                name: name.clone(),
                description: Some(description.clone()),
                parameters: params,
            });
        }
    }
    if filter_for_gemma && filtered_out > 0 {
        let kept = functions.len();
        debug!("Gemma: filtered {filtered_out} function tools, keeping {kept}");
    }

    let mut out = Vec::new();
    if !functions.is_empty() {
        out.push(GeminiTool {
            function_declarations: Some(functions),
            google_search: None,
        });
    }
    // Gemini API does not allow google_search and functionDeclarations in the
    // same request.  Only add google_search when there are no function tools.
    if enable_google_search && out.is_empty() {
        out.push(GeminiTool {
            function_declarations: None,
            google_search: Some(GeminiGoogleSearchTool::default()),
        });
    }

    if out.is_empty() { None } else { Some(out) }
}
// ── Content building ─────────────────────────────────────────────────

pub(crate) fn build_gemini_contents(
    items: &[ResponseItem],
    reference_images: &[String],
    api_model: &str,
) -> Vec<GeminiContentRequest> {
    let mut contents = Vec::new();
    let mut function_calls_by_id: HashMap<String, (String, Option<String>)> = HashMap::new();

    for item in items {
        match item {
            ResponseItem::Message {
                role,
                content,
                thought_signature,
                ..
            } => {
                let parts = content_to_gemini_parts(content, thought_signature.as_deref());
                if parts.is_empty() {
                    continue;
                }
                contents.push(GeminiContentRequest {
                    role: Some(map_gemini_role(role)),
                    parts,
                });
            }

            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                thought_signature,
                ..
            } => {
                function_calls_by_id
                    .insert(call_id.clone(), (name.clone(), thought_signature.clone()));
                let args: Value =
                    serde_json::from_str(arguments).unwrap_or(Value::Object(Default::default()));

                // Merge parallel function calls into the same model content block.
                if let Some(last) = contents.last_mut()
                    && last.role.as_deref() == Some("model")
                    && last.parts.iter().all(|p| p.function_call.is_some())
                {
                    last.parts.push(GeminiPartRequest {
                        text: None,
                        inline_data: None,
                        function_call: Some(GeminiFunctionCallPart {
                            name: name.clone(),
                            args,
                        }),
                        function_response: None,
                        thought_signature: None,
                        compat_thought_signature: None,
                    });
                    continue;
                }

                let part_sig = thought_signature.clone();
                contents.push(GeminiContentRequest {
                    role: Some("model".to_string()),
                    parts: vec![GeminiPartRequest {
                        text: None,
                        inline_data: None,
                        function_call: Some(GeminiFunctionCallPart {
                            name: name.clone(),
                            args,
                        }),
                        function_response: None,
                        thought_signature: part_sig.clone(),
                        compat_thought_signature: part_sig,
                    }],
                });
            }

            ResponseItem::FunctionCallOutput { call_id, output } => {
                let (function_name, _) = function_calls_by_id
                    .get(call_id)
                    .map(|(n, s)| (n.clone(), s.clone()))
                    .unwrap_or_else(|| ("unknown_function".to_string(), None));

                let (output_text, mut inline_parts) =
                    build_gemini_function_response_payload(output);
                let response_value = serde_json::json!({
                    "output": output_text,
                    "success": output.success.unwrap_or(true)
                });
                let supports_multimodal = supports_multiple_inline_images(api_model);
                let nested_parts = if supports_multimodal && !inline_parts.is_empty() {
                    Some(std::mem::take(&mut inline_parts))
                } else {
                    None
                };

                let response_part = GeminiPartRequest {
                    text: None,
                    inline_data: None,
                    function_call: None,
                    function_response: Some(GeminiFunctionResponsePart {
                        id: Some(call_id.clone()),
                        name: function_name,
                        response: response_value,
                        parts: nested_parts,
                    }),
                    thought_signature: None,
                    compat_thought_signature: None,
                };

                // Merge parallel function responses into the same user content block.
                if let Some(last) = contents.last_mut()
                    && last.role.as_deref() == Some("user")
                    && last
                        .parts
                        .iter()
                        .all(|p| p.function_response.is_some() || p.inline_data.is_some())
                {
                    last.parts.push(response_part);
                    if !supports_multimodal {
                        last.parts.append(&mut inline_parts);
                    }
                    continue;
                }

                let mut parts = vec![response_part];
                if !supports_multimodal {
                    parts.append(&mut inline_parts);
                }
                contents.push(GeminiContentRequest {
                    role: Some("user".to_string()),
                    parts,
                });
            }

            _ => {
                // Skip other item types (e.g., Reasoning, LocalShellCall, etc.)
            }
        }
    }

    limit_inline_images_for_model(&mut contents, api_model);
    append_reference_images_to_contents(&mut contents, reference_images, api_model);

    if tracing::enabled!(tracing::Level::DEBUG) {
        let (fc, fr) = contents.iter().fold((0, 0), |(fc, fr), c| {
            c.parts.iter().fold((fc, fr), |(fc, fr), p| {
                (
                    fc + usize::from(p.function_call.is_some()),
                    fr + usize::from(p.function_response.is_some()),
                )
            })
        });
        debug!(
            "Gemini: built {} contents with {} function calls and {} function responses",
            contents.len(),
            fc,
            fr
        );
    }

    contents
}

// ── Content helpers ──────────────────────────────────────────────────

fn content_to_gemini_parts(
    content: &[ContentItem],
    message_thought_signature: Option<&str>,
) -> Vec<GeminiPartRequest> {
    let mut parts = Vec::new();
    for entry in content {
        match entry {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                if text.trim().is_empty() {
                    continue;
                }
                parts.push(GeminiPartRequest {
                    text: Some(text.clone()),
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                    thought_signature: None,
                    compat_thought_signature: None,
                });
            }
            ContentItem::InputImage { image_url } => {
                if let Some((mime, data)) = parse_data_url(image_url)
                    && !mime.is_empty()
                    && !data.trim().is_empty()
                {
                    parts.push(gemini_inline_data_part(mime, data));
                }
            }
        }
    }
    if let Some(sig) = message_thought_signature
        && let Some(last) = parts.last_mut()
        && last.thought_signature.is_none()
    {
        last.thought_signature = Some(sig.to_string());
        last.compat_thought_signature = Some(sig.to_string());
    }
    parts
}

fn map_gemini_role(role: &str) -> String {
    if role.eq_ignore_ascii_case("assistant") {
        "model".to_string()
    } else {
        "user".to_string()
    }
}

fn gemini_inline_data_part(mime_type: String, data: String) -> GeminiPartRequest {
    GeminiPartRequest {
        text: None,
        inline_data: Some(GeminiInlineData { mime_type, data }),
        function_call: None,
        function_response: None,
        thought_signature: None,
        compat_thought_signature: None,
    }
}

fn split_function_output_content(
    items: &[FunctionCallOutputContentItem],
) -> (Vec<String>, Vec<GeminiPartRequest>) {
    let mut text_parts = Vec::new();
    let mut inline_parts = Vec::new();
    for item in items {
        match item {
            FunctionCallOutputContentItem::InputText { text } => {
                if !text.trim().is_empty() {
                    text_parts.push(text.clone());
                }
            }
            FunctionCallOutputContentItem::InputImage { image_url, .. } => {
                if let Some((mime, data)) = parse_data_url(image_url) {
                    inline_parts.push(gemini_inline_data_part(mime, data));
                } else if !image_url.trim().is_empty() {
                    text_parts.push(format!("Image reference: {image_url}"));
                }
            }
        }
    }
    (text_parts, inline_parts)
}

fn build_gemini_function_response_payload(
    output: &FunctionCallOutputPayload,
) -> (String, Vec<GeminiPartRequest>) {
    let (text_parts, inline_parts) =
        if let Some(items) = output.content_items().filter(|items| !items.is_empty()) {
            split_function_output_content(items)
        } else {
            let mut tp = Vec::new();
            let text = output.to_string();
            if !text.trim().is_empty() {
                tp.push(text);
            }
            (tp, Vec::new())
        };

    let mut output_text = if text_parts.is_empty() {
        String::new()
    } else {
        text_parts.join("\n")
    };
    if output_text.is_empty() && !inline_parts.is_empty() {
        output_text = format!("Binary content provided ({} item(s)).", inline_parts.len());
    }
    (output_text, inline_parts)
}

pub(crate) fn parse_data_url(url: &str) -> Option<(String, String)> {
    let without_prefix = url.strip_prefix("data:")?;
    let (meta, data) = without_prefix.split_once(',')?;
    let (mime, encoding) = meta.split_once(';')?;
    if !encoding.eq_ignore_ascii_case("base64") {
        return None;
    }
    Some((mime.to_string(), data.to_string()))
}

// ── Thought signatures ───────────────────────────────────────────────

pub(crate) fn ensure_active_loop_has_thought_signatures(
    contents: &[GeminiContentRequest],
) -> Vec<GeminiContentRequest> {
    let mut new_contents = contents.to_vec();

    // Find the start of the "active loop" as the last user turn with text.
    let mut last_user_with_text: Option<usize> = None;
    for (idx, content) in new_contents.iter().enumerate() {
        if !content
            .role
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("user"))
        {
            continue;
        }
        if content
            .parts
            .iter()
            .any(|p| p.text.as_deref().is_some_and(|t| !t.trim().is_empty()))
        {
            last_user_with_text = Some(idx);
        }
    }

    let Some(start) = last_user_with_text.and_then(|i| i.checked_add(1)) else {
        return new_contents;
    };
    if start >= new_contents.len() {
        return new_contents;
    }

    for content in &mut new_contents[start..] {
        if !content
            .role
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("model"))
        {
            continue;
        }

        let mut patched_first_call = false;
        for part in &mut content.parts {
            if part.function_call.is_some() && !patched_first_call {
                patched_first_call = true;
                if part.thought_signature.is_none() {
                    let sig = part
                        .compat_thought_signature
                        .clone()
                        .unwrap_or_else(|| SYNTHETIC_THOUGHT_SIGNATURE.to_string());
                    part.thought_signature = Some(sig.clone());
                    if part.compat_thought_signature.is_none() {
                        part.compat_thought_signature = Some(sig);
                    }
                } else if part.compat_thought_signature.is_none() {
                    part.compat_thought_signature = part.thought_signature.clone();
                }
            }
            if part.inline_data.is_some() && part.thought_signature.is_none() {
                let sig = part
                    .compat_thought_signature
                    .clone()
                    .unwrap_or_else(|| SYNTHETIC_THOUGHT_SIGNATURE.to_string());
                part.thought_signature = Some(sig.clone());
                if part.compat_thought_signature.is_none() {
                    part.compat_thought_signature = Some(sig);
                }
            }
        }
    }

    new_contents
}

fn append_reference_images_to_contents(
    contents: &mut Vec<GeminiContentRequest>,
    reference_images: &[String],
    api_model: &str,
) {
    if reference_images.is_empty() {
        return;
    }
    let max_inline_images = if supports_multiple_inline_images(api_model) {
        14
    } else {
        1
    };
    let limit = reference_images.len().min(max_inline_images);

    let user_index = contents.iter().rposition(|c| {
        c.role
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("user"))
    });

    let index = if let Some(i) = user_index {
        i
    } else {
        contents.push(GeminiContentRequest {
            role: Some("user".to_string()),
            parts: Vec::new(),
        });
        contents.len().saturating_sub(1)
    };

    for image_url in reference_images.iter().take(limit) {
        if let Some((mime, data)) = parse_data_url(image_url) {
            if mime.is_empty() || data.trim().is_empty() {
                continue;
            }
            contents[index]
                .parts
                .push(gemini_inline_data_part(mime, data));
        } else if !image_url.trim().is_empty() {
            contents[index].parts.push(GeminiPartRequest {
                text: Some(format!("Image reference: {image_url}")),
                inline_data: None,
                function_call: None,
                function_response: None,
                thought_signature: None,
                compat_thought_signature: None,
            });
        }
    }
}

fn limit_inline_images_for_model(contents: &mut [GeminiContentRequest], api_model: &str) {
    if supports_multiple_inline_images(api_model) {
        return;
    }

    let mut seen_inline_image = false;
    let mut dropped_images = 0usize;

    for content in contents {
        content.parts.retain(|part| {
            if part.inline_data.is_none() {
                return true;
            }
            if seen_inline_image {
                dropped_images += 1;
                return false;
            }
            seen_inline_image = true;
            true
        });
    }

    if dropped_images > 0 {
        debug!("Gemma: dropped {dropped_images} extra inline image part(s)");
    }
}

// ── Thought signature stripping for non-Gemini providers ─────────────

#[allow(dead_code)]
pub(crate) fn strip_thought_signatures_from_input(input: &[ResponseItem]) -> Vec<ResponseItem> {
    input
        .iter()
        .cloned()
        .map(|mut item| {
            match &mut item {
                ResponseItem::FunctionCall {
                    thought_signature, ..
                } => {
                    *thought_signature = None;
                }
                ResponseItem::Message {
                    thought_signature, ..
                } => {
                    *thought_signature = None;
                }
                _ => {}
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_common::tools::ResponsesApiTool;
    use crate::tools::spec::JsonSchema;
    use pretty_assertions::assert_eq;

    fn function_tool(name: &str) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: name.to_string(),
            description: format!("tool {name}"),
            defer_loading: None,
            parameters: JsonSchema::Object {
                properties: Default::default(),
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            strict: false,
        })
    }

    fn first_turn_input() -> Vec<ResponseItem> {
        first_turn_input_with_text("hi")
    }

    fn first_turn_input_with_text(text: &str) -> Vec<ResponseItem> {
        vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }]
    }

    #[test]
    fn gemini_3_detection_includes_gemma_3() {
        assert!(is_gemini_3_model("gemini-3-pro-preview"));
        assert!(is_gemini_3_model("gemma-3n"));
        assert!(!is_gemini_3_model("gemini-2.5-pro"));
    }

    #[test]
    fn strip_model_suffix_preserves_antigravity_preview_image_api_model() {
        assert_eq!(
            strip_model_suffix("antigravity/gemini-3.1-flash-image-preview"),
            "gemini-3.1-flash-image-preview"
        );
        assert_eq!(
            strip_model_suffix("antigravity-gemini/gemini-3.1-flash-image-preview"),
            "gemini-3.1-flash-image-preview"
        );
        assert_eq!(
            strip_model_suffix("antigravity/gemini-3-pro-image-preview"),
            "gemini-3-pro-image-preview"
        );
        assert_eq!(
            strip_model_suffix("antigravity-gemini/gemini-3-pro-image-preview"),
            "gemini-3-pro-image-preview"
        );
    }

    #[test]
    fn gemma_reference_images_are_limited_to_one() {
        let images = vec![
            "data:image/png;base64,AAAA".to_string(),
            "data:image/png;base64,BBBB".to_string(),
        ];
        let contents = build_gemini_contents(&[], &images, "gemma-3n");

        assert_eq!(contents.len(), 1);
        let inline_count = contents[0]
            .parts
            .iter()
            .filter(|part| part.inline_data.is_some())
            .count();
        assert_eq!(inline_count, 1);
    }

    #[test]
    fn gemini_3_reference_images_keep_multiple_items() {
        let images = vec![
            "data:image/png;base64,AAAA".to_string(),
            "data:image/png;base64,BBBB".to_string(),
        ];
        let contents = build_gemini_contents(&[], &images, "gemini-3-pro-preview");

        assert_eq!(contents.len(), 1);
        let inline_count = contents[0]
            .parts
            .iter()
            .filter(|part| part.inline_data.is_some())
            .count();
        assert_eq!(inline_count, 2);
    }

    #[test]
    fn gemma_current_turn_images_are_limited_to_one() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![
                ContentItem::InputText {
                    text: "compare".to_string(),
                },
                ContentItem::InputImage {
                    image_url: "data:image/png;base64,AAAA".to_string(),
                },
                ContentItem::InputImage {
                    image_url: "data:image/png;base64,BBBB".to_string(),
                },
            ],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }];

        let contents = build_gemini_contents(&items, &[], "gemma-3n");
        let inline_count: usize = contents
            .iter()
            .map(|content| {
                content
                    .parts
                    .iter()
                    .filter(|part| part.inline_data.is_some())
                    .count()
            })
            .sum();

        assert_eq!(inline_count, 1);
    }

    #[test]
    fn gemini_3_current_turn_images_keep_multiple_items() {
        let items = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![
                ContentItem::InputText {
                    text: "compare".to_string(),
                },
                ContentItem::InputImage {
                    image_url: "data:image/png;base64,AAAA".to_string(),
                },
                ContentItem::InputImage {
                    image_url: "data:image/png;base64,BBBB".to_string(),
                },
            ],
            end_turn: None,
            phase: None,
            thought_signature: None,
        }];

        let contents = build_gemini_contents(&items, &[], "gemini-3-pro-preview");
        let inline_count: usize = contents
            .iter()
            .map(|content| {
                content
                    .parts
                    .iter()
                    .filter(|part| part.inline_data.is_some())
                    .count()
            })
            .sum();

        assert_eq!(inline_count, 2);
    }

    #[test]
    fn gemma_tools_filter_to_stable_subset() {
        let tools = vec![
            function_tool("shell_command"),
            function_tool("read_file"),
            function_tool("apply_patch"),
            function_tool("list_mcp_resources"),
        ];

        let gemini_tools = build_gemini_tools(&tools, "gemma-3n")
            .expect("gemma should keep at least the stable function tools");
        let names: Vec<String> = gemini_tools[0]
            .function_declarations
            .as_ref()
            .expect("function declarations should be present")
            .iter()
            .map(|f| f.name.clone())
            .collect();

        assert_eq!(
            names,
            vec!["shell_command".to_string(), "read_file".to_string()]
        );
    }

    #[test]
    fn non_gemma_tools_keep_full_declaration_set() {
        let tools = vec![
            function_tool("shell_command"),
            function_tool("read_file"),
            function_tool("apply_patch"),
            function_tool("list_mcp_resources"),
        ];

        let gemini_tools = build_gemini_tools(&tools, "gemini-2.5-pro")
            .expect("gemini should include all function tools");
        let names: Vec<String> = gemini_tools[0]
            .function_declarations
            .as_ref()
            .expect("function declarations should be present")
            .iter()
            .map(|f| f.name.clone())
            .collect();

        assert_eq!(
            names,
            vec![
                "shell_command".to_string(),
                "read_file".to_string(),
                "apply_patch".to_string(),
                "list_mcp_resources".to_string()
            ]
        );
    }

    #[test]
    fn gemini_tools_do_not_include_google_search_when_function_tools_present() {
        // With the gemini_web_search function-call approach, ToolSpec::WebSearch
        // is replaced by a function tool in build_specs() for Gemini providers.
        // build_gemini_tools() should NOT add google_search alongside functions.
        let tools = vec![
            function_tool("shell_command"),
            ToolSpec::WebSearch {
                external_web_access: Some(true),
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        ];

        let gemini_tools = build_gemini_tools(&tools, "gemini-3-pro-preview")
            .expect("gemini tools should be present");

        assert!(
            gemini_tools.iter().all(|tool| tool.google_search.is_none()),
            "google_search must not coexist with functionDeclarations"
        );
    }

    #[test]
    fn gemma_tools_do_not_include_google_search() {
        let tools = vec![
            function_tool("shell_command"),
            ToolSpec::WebSearch {
                external_web_access: Some(true),
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        ];

        let gemini_tools =
            build_gemini_tools(&tools, "gemma-3n").expect("gemma tools should be present");

        assert!(
            gemini_tools.iter().all(|tool| tool.google_search.is_none()),
            "gemma should not enable google_search"
        );
    }

    #[test]
    fn gemma_tool_config_does_not_force_any_mode() {
        let tools = vec![function_tool("shell_command"), function_tool("read_file")];
        let config = build_gemini_tool_config(&tools, &first_turn_input(), "gemma-3n");

        assert_eq!(config.mode, GeminiFunctionCallingMode::Auto);
        assert_eq!(config.allowed_function_names, None);
        assert_eq!(config.stream_function_call_arguments, None);
    }

    #[test]
    fn gemini_3_tool_config_does_not_force_for_generic_first_turn() {
        let tools = vec![function_tool("shell_command"), function_tool("read_file")];
        let config = build_gemini_tool_config(&tools, &first_turn_input(), "gemini-3-pro-preview");

        assert_eq!(config.mode, GeminiFunctionCallingMode::Auto);
        assert_eq!(config.allowed_function_names, None);
        assert_eq!(config.stream_function_call_arguments, Some(true));
    }

    #[test]
    fn gemini_3_tool_config_forces_read_first_turn_when_prompt_needs_local_context() {
        let tools = vec![function_tool("shell_command"), function_tool("read_file")];
        let input = first_turn_input_with_text("analyze current project");
        let config = build_gemini_tool_config(&tools, &input, "gemini-3-pro-preview");

        assert_eq!(config.mode, GeminiFunctionCallingMode::Any);
        assert_eq!(
            config.allowed_function_names,
            Some(vec!["read_file".to_string()])
        );
        assert_eq!(config.stream_function_call_arguments, Some(true));
    }
}
