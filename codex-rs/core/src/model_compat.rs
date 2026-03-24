use std::borrow::Cow;

use codex_protocol::openai_models::ReasoningEffort;

/// Returns the canonical Grok model slug when the input refers to a Grok model.
///
/// Accepts both `grok-*` and namespaced `xai/grok-*` forms.
pub(crate) fn normalized_grok_model_slug(slug: &str) -> Option<&str> {
    if slug.starts_with("grok-") {
        Some(slug)
    } else if let Some(stripped) = slug.strip_prefix("xai/")
        && stripped.starts_with("grok-")
    {
        Some(stripped)
    } else {
        None
    }
}

pub(crate) fn is_grok_model_slug(slug: &str) -> bool {
    normalized_grok_model_slug(slug).is_some()
}

/// Returns the canonical Gemma model slug when the input refers to a Gemma model.
///
/// Accepts both `gemma-*` and namespaced `google/gemma-*` forms.
pub(crate) fn normalized_gemma_model_slug(slug: &str) -> Option<&str> {
    if slug.starts_with("gemma-") {
        Some(slug)
    } else if let Some(stripped) = slug.strip_prefix("google/")
        && stripped.starts_with("gemma-")
    {
        Some(stripped)
    } else {
        None
    }
}

pub(crate) fn is_gemma_model_slug(slug: &str) -> bool {
    normalized_gemma_model_slug(slug).is_some()
}

/// Returns the canonical Claude model slug when the input refers to a Claude model.
///
/// Accepts both `claude-*` and namespaced `anthropic/claude-*` forms.
pub(crate) fn normalized_anthropic_model_slug(slug: &str) -> Option<&str> {
    if slug.starts_with("claude-") {
        Some(slug)
    } else if let Some(stripped) = slug.strip_prefix("anthropic/")
        && stripped.starts_with("claude-")
    {
        Some(stripped)
    } else if let Some(stripped) = slug.strip_prefix("antigravity/")
        && stripped.starts_with("claude-")
    {
        Some(stripped)
    } else if let Some(stripped) = slug.strip_prefix("antigravity-anthropic/")
        && stripped.starts_with("claude-")
    {
        Some(stripped)
    } else {
        None
    }
}

pub(crate) fn is_anthropic_model_slug(slug: &str) -> bool {
    normalized_anthropic_model_slug(slug).is_some()
}

pub(crate) fn is_openai_model_slug(slug: &str) -> bool {
    let normalized = slug.strip_prefix("openai/").unwrap_or(slug);
    normalized.starts_with("gpt-")
        || normalized.starts_with("o1-")
        || normalized.starts_with("o3-")
        || normalized.starts_with("o4-")
}

pub(crate) fn normalize_legacy_gemini_model_selection(
    model: &str,
    reasoning_effort: Option<ReasoningEffort>,
) -> (Cow<'_, str>, Option<ReasoningEffort>) {
    match model {
        "gemini-3.1-pro-high" => (
            Cow::Borrowed("gemini-3.1-pro-preview"),
            Some(ReasoningEffort::High),
        ),
        "gemini-3.1-pro-low" => (
            Cow::Borrowed("gemini-3.1-pro-preview"),
            Some(ReasoningEffort::Low),
        ),
        "gemini-3-pro-high" => (
            Cow::Borrowed("gemini-3-pro-preview"),
            Some(ReasoningEffort::High),
        ),
        "gemini-3-pro-low" => (
            Cow::Borrowed("gemini-3-pro-preview"),
            Some(ReasoningEffort::Low),
        ),
        "gemini-3-flash" => (Cow::Borrowed("gemini-3-flash-preview"), reasoning_effort),
        "gemini-3-pro-image" => (
            Cow::Borrowed("gemini-3-pro-image-preview"),
            reasoning_effort,
        ),
        "gemini-3.1-flash-image" => (
            Cow::Borrowed("gemini-3.1-flash-image-preview"),
            reasoning_effort,
        ),
        "antigravity/gemini-3.1-pro-high" | "antigravity-gemini/gemini-3.1-pro-high" => (
            Cow::Borrowed("antigravity/gemini-3.1-pro-preview"),
            Some(ReasoningEffort::High),
        ),
        "antigravity/gemini-3.1-pro-low" | "antigravity-gemini/gemini-3.1-pro-low" => (
            Cow::Borrowed("antigravity/gemini-3.1-pro-preview"),
            Some(ReasoningEffort::Low),
        ),
        "antigravity/gemini-3-pro-high" | "antigravity-gemini/gemini-3-pro-high" => (
            Cow::Borrowed("antigravity/gemini-3-pro-preview"),
            Some(ReasoningEffort::High),
        ),
        "antigravity/gemini-3-pro-low" | "antigravity-gemini/gemini-3-pro-low" => (
            Cow::Borrowed("antigravity/gemini-3-pro-preview"),
            Some(ReasoningEffort::Low),
        ),
        "antigravity/gemini-3-flash" | "antigravity-gemini/gemini-3-flash" => (
            Cow::Borrowed("antigravity/gemini-3-flash-preview"),
            reasoning_effort,
        ),
        "antigravity/gemini-3.1-flash-image" | "antigravity-gemini/gemini-3.1-flash-image" => (
            Cow::Borrowed("antigravity/gemini-3.1-flash-image-preview"),
            reasoning_effort,
        ),
        "antigravity-gemini/gemini-3.1-flash-image-preview" => (
            Cow::Borrowed("antigravity/gemini-3.1-flash-image-preview"),
            reasoning_effort,
        ),
        _ => (Cow::Borrowed(model), reasoning_effort),
    }
}

pub(crate) fn model_supports_web_search_tool(slug: &str) -> bool {
    match normalized_grok_model_slug(slug) {
        Some(grok_slug) => grok_slug.starts_with("grok-4"),
        None => true,
    }
}

pub(crate) fn model_supports_web_search_external_web_access(slug: &str) -> bool {
    normalized_grok_model_slug(slug).is_none()
}

pub(crate) fn model_supports_reasoning_effort(slug: &str) -> bool {
    match normalized_grok_model_slug(slug) {
        Some(grok_slug) => grok_slug.starts_with("grok-3-mini"),
        None => true,
    }
}

pub(crate) fn model_supports_memory_trace_summarize(slug: &str) -> bool {
    normalized_grok_model_slug(slug).is_none()
}

pub(crate) fn model_supports_input_images(slug: &str) -> bool {
    // Codex-Spark is text-only.
    if slug.starts_with("gpt-5.3-codex-spark") {
        return false;
    }

    match normalized_grok_model_slug(slug) {
        Some(grok_slug) => {
            grok_slug.starts_with("grok-4") || grok_slug.starts_with("grok-2-vision")
        }
        None => true,
    }
}

pub(crate) fn model_supports_data_url_input_images(slug: &str) -> bool {
    if !model_supports_input_images(slug) {
        return false;
    }

    match normalized_grok_model_slug(slug) {
        Some(grok_slug) => model_supports_input_images(grok_slug),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn grok_slug_normalization_handles_namespaced_and_bare_slugs() {
        assert_eq!(
            normalized_grok_model_slug("grok-4-latest"),
            Some("grok-4-latest")
        );
        assert_eq!(
            normalized_grok_model_slug("xai/grok-4-latest"),
            Some("grok-4-latest")
        );
        assert_eq!(normalized_grok_model_slug("grok-4.1"), Some("grok-4.1"));
        assert_eq!(normalized_grok_model_slug("xai/gpt-5"), None);
        assert_eq!(normalized_grok_model_slug("gpt-5"), None);
    }

    #[test]
    fn gemma_slug_normalization_handles_namespaced_and_bare_slugs() {
        assert_eq!(normalized_gemma_model_slug("gemma-3n"), Some("gemma-3n"));
        assert_eq!(
            normalized_gemma_model_slug("google/gemma-3n-e4b-it"),
            Some("gemma-3n-e4b-it")
        );
        assert_eq!(normalized_gemma_model_slug("gemini-3-pro-preview"), None);
    }

    #[test]
    fn web_search_capabilities_match_current_xai_constraints() {
        assert!(model_supports_web_search_tool("gpt-5-codex"));
        assert!(model_supports_web_search_tool("grok-4-1-fast-reasoning"));
        assert!(model_supports_web_search_tool("xai/grok-4-latest"));
        assert!(!model_supports_web_search_tool("grok-3"));

        assert!(model_supports_web_search_external_web_access("gpt-5-codex"));
        assert!(!model_supports_web_search_external_web_access(
            "grok-4-latest"
        ));
        assert!(!model_supports_web_search_external_web_access(
            "xai/grok-4-latest"
        ));
    }

    #[test]
    fn data_url_image_support_matches_current_xai_constraints() {
        assert!(model_supports_data_url_input_images("gpt-5-codex"));
        assert!(model_supports_data_url_input_images("grok-4-0709"));
        assert!(model_supports_data_url_input_images("xai/grok-4-latest"));
        assert!(model_supports_data_url_input_images("grok-2-vision-1212"));
        assert!(!model_supports_data_url_input_images("grok-3"));
    }

    #[test]
    fn reasoning_effort_support_matches_current_xai_constraints() {
        assert!(model_supports_reasoning_effort("gpt-5-codex"));
        assert!(!model_supports_reasoning_effort("grok-4-latest"));
        assert!(!model_supports_reasoning_effort("xai/grok-4-latest"));
        assert!(!model_supports_reasoning_effort("grok-3"));
        assert!(model_supports_reasoning_effort("grok-3-mini"));
        assert!(model_supports_reasoning_effort("xai/grok-3-mini"));
    }

    #[test]
    fn memory_trace_summarization_is_disabled_for_grok_models() {
        assert!(model_supports_memory_trace_summarize("gpt-5-codex"));
        assert!(!model_supports_memory_trace_summarize("grok-4-latest"));
        assert!(!model_supports_memory_trace_summarize("xai/grok-4-latest"));
    }

    #[test]
    fn grok_image_support_matches_current_xai_constraints() {
        assert!(model_supports_input_images("gpt-5-codex"));
        assert!(model_supports_input_images("grok-4-latest"));
        assert!(model_supports_input_images("xai/grok-4-0709"));
        assert!(model_supports_input_images("grok-2-vision-1212"));
        assert!(!model_supports_input_images("grok-3"));
        assert!(!model_supports_input_images("grok-3-mini"));
    }

    #[test]
    fn spark_model_is_text_only() {
        assert!(!model_supports_input_images("gpt-5.3-codex-spark|[pro]"));
        assert!(!model_supports_data_url_input_images(
            "gpt-5.3-codex-spark|[pro]"
        ));
    }

    #[test]
    fn anthropic_slug_normalization_handles_namespaced_and_bare_slugs() {
        assert_eq!(
            normalized_anthropic_model_slug("claude-opus-4-6"),
            Some("claude-opus-4-6")
        );
        assert_eq!(
            normalized_anthropic_model_slug("anthropic/claude-sonnet-4-6"),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(normalized_anthropic_model_slug("anthropic/gpt-5"), None);
        assert_eq!(normalized_anthropic_model_slug("gpt-5"), None);
    }

    #[test]
    fn is_openai_model_slug_detects_namespaced_and_bare_slugs() {
        assert!(is_openai_model_slug("gpt-5.3-codex"));
        assert!(is_openai_model_slug("openai/gpt-5.3-codex"));
        assert!(is_openai_model_slug("o1-mini"));
        assert!(is_openai_model_slug("o3-mini"));
        assert!(is_openai_model_slug("o4-mini"));
        assert!(!is_openai_model_slug("claude-opus-4-6"));
        assert!(!is_openai_model_slug("gemini-3-pro-preview"));
        assert!(!is_openai_model_slug("grok-4-latest"));
        assert!(!is_openai_model_slug("llama3"));
    }

    #[test]
    fn legacy_gemini_model_selection_normalizes_old_slugs_and_effort() {
        for (model, reasoning_effort, expected_model, expected_effort) in [
            (
                "gemini-3.1-pro-high",
                Some(ReasoningEffort::Minimal),
                "gemini-3.1-pro-preview",
                Some(ReasoningEffort::High),
            ),
            (
                "gemini-3.1-pro-low",
                Some(ReasoningEffort::High),
                "gemini-3.1-pro-preview",
                Some(ReasoningEffort::Low),
            ),
            (
                "antigravity/gemini-3-pro-high",
                Some(ReasoningEffort::Minimal),
                "antigravity/gemini-3-pro-preview",
                Some(ReasoningEffort::High),
            ),
            (
                "antigravity-gemini/gemini-3-flash",
                Some(ReasoningEffort::Medium),
                "antigravity/gemini-3-flash-preview",
                Some(ReasoningEffort::Medium),
            ),
            (
                "gemini-3-pro-image",
                None,
                "gemini-3-pro-image-preview",
                None,
            ),
            (
                "gemini-3-pro-image-preview",
                Some(ReasoningEffort::Medium),
                "gemini-3-pro-image-preview",
                Some(ReasoningEffort::Medium),
            ),
            (
                "gemini-3.1-flash-image",
                None,
                "gemini-3.1-flash-image-preview",
                None,
            ),
            (
                "gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
                "gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
            ),
            (
                "antigravity/gemini-3.1-flash-image",
                None,
                "antigravity/gemini-3.1-flash-image-preview",
                None,
            ),
            (
                "antigravity/gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
                "antigravity/gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
            ),
            (
                "antigravity-gemini/gemini-3.1-flash-image",
                None,
                "antigravity/gemini-3.1-flash-image-preview",
                None,
            ),
            (
                "antigravity-gemini/gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
                "antigravity/gemini-3.1-flash-image-preview",
                Some(ReasoningEffort::Medium),
            ),
            (
                "antigravity/gemini-3.1-pro-preview",
                Some(ReasoningEffort::High),
                "antigravity/gemini-3.1-pro-preview",
                Some(ReasoningEffort::High),
            ),
        ] {
            let (normalized_model, normalized_effort) =
                normalize_legacy_gemini_model_selection(model, reasoning_effort);
            assert_eq!(normalized_model.as_ref(), expected_model);
            assert_eq!(normalized_effort, expected_effort);
        }
    }
}
