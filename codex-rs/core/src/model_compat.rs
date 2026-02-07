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
    match normalized_grok_model_slug(slug) {
        Some(grok_slug) => {
            grok_slug.starts_with("grok-4") || grok_slug.starts_with("grok-2-vision")
        }
        None => true,
    }
}

pub(crate) fn model_supports_data_url_input_images(slug: &str) -> bool {
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
}
