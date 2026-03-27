//! Gemini web search handler: converts a `gemini_web_search` function call into
//! a lightweight Gemini API request that uses only the `google_search` tool
//! (no function declarations), then returns grounding results as text.

use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;

use crate::function_tool::FunctionCallError;
use crate::gemini_content::normalize_gemini_base_url;
use crate::gemini_types::*;
use crate::provider_auth::resolve_gemini_api_key;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

use super::parse_arguments;

pub struct GeminiWebSearchHandler;

#[derive(Deserialize)]
struct Args {
    query: String,
}

#[async_trait]
impl ToolHandler for GeminiWebSearchHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolPayload::Function { arguments } = &invocation.payload else {
            return Err(FunctionCallError::RespondToModel(
                "expected function payload".into(),
            ));
        };
        let args: Args = parse_arguments(arguments)?;
        if args.query.trim().is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "query must not be empty".into(),
            ));
        }

        let provider = &invocation.turn.provider;
        let base_url = provider.base_url.as_deref().ok_or_else(|| {
            FunctionCallError::RespondToModel("Gemini provider must define a base_url".into())
        })?;
        let base_url = normalize_gemini_base_url(base_url);

        // Use a lightweight model for the search-only call.
        let search_model = "gemini-2.5-flash";
        let url = format!(
            "{}/models/{}:generateContent",
            base_url.as_ref().trim_end_matches('/'),
            search_model,
        );

        let auth = match invocation.turn.auth_manager.as_ref() {
            Some(manager) => manager.auth().await,
            None => None,
        };
        let api_key = resolve_gemini_api_key(provider, auth.as_ref());

        // Build a minimal request with only google_search tool.
        let request = GeminiRequest {
            system_instruction: None,
            contents: vec![GeminiContentRequest {
                role: Some("user".into()),
                parts: vec![GeminiPartRequest {
                    text: Some(args.query.clone()),
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                    thought_signature: None,
                    compat_thought_signature: None,
                }],
            }],
            tools: Some(vec![GeminiTool {
                function_declarations: None,
                google_search: Some(GeminiGoogleSearchTool::default()),
            }]),
            tool_config: None,
            generation_config: None,
            safety_settings: None,
        };

        let client = crate::default_client::build_reqwest_client();
        let mut req_builder = client.post(&url);
        req_builder = provider.apply_http_headers(req_builder);
        if let Some(key) = api_key.as_deref() {
            req_builder = req_builder.header("x-goog-api-key", key);
        }

        debug!(query = %args.query, url = %url, "gemini_web_search: sending request");

        let response =
            req_builder.json(&request).send().await.map_err(|e| {
                FunctionCallError::RespondToModel(format!("HTTP request failed: {e}"))
            })?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|e| format!("failed to read response body: {e}"));

        if !status.is_success() {
            return Err(FunctionCallError::RespondToModel(format!(
                "Gemini web search API returned {status}: {body_text}"
            )));
        }

        let gemini_resp: GeminiResponse = serde_json::from_str(&body_text).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to parse Gemini response: {e}"))
        })?;

        let result_text = format_grounding_response(&gemini_resp);

        Ok(FunctionToolOutput::from_text(result_text, Some(true)))
    }
}

/// Extract text content and grounding metadata from a Gemini response into a
/// human-readable string that the model can consume.
fn format_grounding_response(resp: &GeminiResponse) -> String {
    let mut out = String::new();

    // Extract text from candidates.
    if let Some(candidates) = &resp.candidates {
        for candidate in candidates {
            if let Some(content) = &candidate.content
                && let Some(parts) = &content.parts
            {
                for part in parts {
                    if let Some(text) = &part.text {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
            }

            // Append grounding metadata (search queries + source links).
            if let Some(meta) = &candidate.grounding_metadata {
                if let Some(queries) = &meta.web_search_queries
                    && !queries.is_empty()
                {
                    out.push_str("\n--- Search Queries ---\n");
                    for q in queries {
                        out.push_str("- ");
                        out.push_str(q);
                        out.push('\n');
                    }
                }
                if let Some(chunks) = &meta.grounding_chunks {
                    let sources: Vec<_> = chunks
                        .iter()
                        .filter_map(|c| c.web.as_ref())
                        .filter_map(|w| {
                            let uri = w.uri.as_deref()?;
                            let title = w.title.as_deref().unwrap_or(uri);
                            Some(format!("- [{title}]({uri})"))
                        })
                        .collect();
                    if !sources.is_empty() {
                        out.push_str("\n--- Sources ---\n");
                        for s in &sources {
                            out.push_str(s);
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }

    if out.trim().is_empty() {
        "No web search results found.".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_grounding_with_text_and_sources() {
        let resp = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContentResponse {
                    parts: Some(vec![GeminiPartResponse {
                        text: Some("Rust 1.84 is the latest stable release.".into()),
                        inline_data: None,
                        function_call: None,
                        thought_signature: None,
                        thought: None,
                    }]),
                }),
                grounding_metadata: Some(GeminiGroundingMetadata {
                    web_search_queries: Some(vec!["latest rust version".into()]),
                    grounding_chunks: Some(vec![GeminiGroundingChunk {
                        web: Some(GeminiGroundingChunkWeb {
                            uri: Some("https://blog.rust-lang.org/".into()),
                            title: Some("Rust Blog".into()),
                        }),
                    }]),
                }),
            }]),
            response_id: None,
            usage_metadata: None,
            error: None,
        };

        let text = format_grounding_response(&resp);
        assert!(text.contains("Rust 1.84"));
        assert!(text.contains("latest rust version"));
        assert!(text.contains("[Rust Blog](https://blog.rust-lang.org/)"));
    }

    #[test]
    fn format_grounding_empty_response() {
        let resp = GeminiResponse {
            candidates: None,
            response_id: None,
            usage_metadata: None,
            error: None,
        };
        assert_eq!(
            format_grounding_response(&resp),
            "No web search results found."
        );
    }
}
