use crate::codex::Session;
use crate::codex::TurnContext;
use crate::compact;
use crate::entire_integration;
use crate::state_db;
use codex_features::Feature;
use codex_protocol::models::ResponseItem;
use codex_utils_string::take_bytes_at_char_boundary;
use codex_utils_string::take_last_bytes_at_char_boundary;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContextPacketConfig {
    pub(crate) max_context_bytes: usize,
    pub(crate) truncation_notice: &'static str,
    pub(crate) max_recent_messages: usize,
    pub(crate) max_recent_bytes: usize,
    pub(crate) max_message_bytes: usize,
    pub(crate) max_trace_summary_bytes: usize,
    pub(crate) max_memory_summary_bytes: usize,
    pub(crate) max_user_instructions_bytes: usize,
    pub(crate) max_session_summary_bytes: usize,
    pub(crate) max_project_memories: usize,
    pub(crate) max_project_memory_summary_bytes: usize,
    pub(crate) include_entire_summary: bool,
    pub(crate) max_entire_checkpoints: usize,
    pub(crate) max_entire_summary_bytes: usize,
}

pub(crate) const CONTEXT_PACKET_TRUNCATION_NOTICE: &str =
    "[Context truncated; showing most recent]\n\n";

pub(crate) const CLAUDE_CODE_CONTEXT_PACKET_CONFIG: ContextPacketConfig = ContextPacketConfig {
    max_context_bytes: 12_000,
    truncation_notice: CONTEXT_PACKET_TRUNCATION_NOTICE,
    max_recent_messages: 16,
    max_recent_bytes: 4_800,
    max_message_bytes: 1_000,
    max_trace_summary_bytes: 1_600,
    max_memory_summary_bytes: 1_600,
    max_user_instructions_bytes: 3_200,
    max_session_summary_bytes: 3_200,
    max_project_memories: 3,
    max_project_memory_summary_bytes: 800,
    include_entire_summary: false,
    max_entire_checkpoints: 0,
    max_entire_summary_bytes: 0,
};

/// Larger packet size intended for MCP "agent" tools where the callee model has a very large
/// context window (e.g. Claude 1M) and does not have access to the parent session history.
pub(crate) const CLAUDE_CODE_LARGE_CONTEXT_PACKET_CONFIG: ContextPacketConfig =
    ContextPacketConfig {
        max_context_bytes: 200_000,
        truncation_notice: CONTEXT_PACKET_TRUNCATION_NOTICE,
        max_recent_messages: 64,
        max_recent_bytes: 120_000,
        max_message_bytes: 8_000,
        max_trace_summary_bytes: 16_000,
        max_memory_summary_bytes: 16_000,
        max_user_instructions_bytes: 16_000,
        max_session_summary_bytes: 32_000,
        max_project_memories: 10,
        max_project_memory_summary_bytes: 4_000,
        include_entire_summary: false,
        max_entire_checkpoints: 0,
        max_entire_summary_bytes: 0,
    };

fn render_active_memory_scope_section(
    scope_kind: &str,
    scope_version: &str,
    summary_sha256: &str,
    binding_key: &str,
    memory_root: &std::path::Path,
    memory_summary: &str,
    max_memory_summary_bytes: usize,
) -> String {
    let memory_root = memory_root.display();
    let memory_summary = truncate_text_bytes(memory_summary.trim(), max_memory_summary_bytes);
    format!(
        "Active memory scope: {scope_kind}\nActive memory scope version: {scope_version}\nActive memory summary sha256: {summary_sha256}\nActive memory binding key: {binding_key}\nActive memory root: {memory_root}\nActive memory summary:\n{memory_summary}"
    )
}

pub(crate) async fn build_context_packet(
    sess: &Session,
    turn_context: &TurnContext,
    config: ContextPacketConfig,
) -> String {
    if config.max_context_bytes == 0 {
        return String::new();
    }

    let mut sections = Vec::new();
    sections.push(format!("Working directory: {}", turn_context.cwd.display()));

    if config.max_user_instructions_bytes > 0
        && let Some(user_instructions) = turn_context.user_instructions.as_deref()
    {
        let user_instructions =
            truncate_text_bytes(user_instructions.trim(), config.max_user_instructions_bytes);
        let user_instructions = user_instructions.trim();
        if !user_instructions.is_empty() {
            sections.push(format!("User instructions:\n{user_instructions}"));
        }
    }

    if turn_context.features.enabled(Feature::MemoryTool)
        && let Some(active_memory_source) = turn_context.resolve_memory_read_path_source().await
    {
        sections.push(render_active_memory_scope_section(
            active_memory_source.scope_kind,
            &active_memory_source.memory_scope_version,
            &active_memory_source.memory_summary_sha256,
            &active_memory_source.memory_binding_key,
            active_memory_source.memory_root.as_path(),
            &active_memory_source.memory_summary,
            config.max_memory_summary_bytes,
        ));
    }

    let include_memory_sections =
        sess.state_db().is_some() && turn_context.features.enabled(Feature::MemoryTool);

    if include_memory_sections {
        if let Some(memory) = state_db::get_thread_memory(
            sess.state_db().as_deref(),
            sess.conversation_id,
            "context_packet_thread_memory",
        )
        .await
        {
            let trace_summary =
                truncate_text_bytes(memory.raw_memory.trim(), config.max_trace_summary_bytes);
            let memory_summary = truncate_text_bytes(
                memory.memory_summary.trim(),
                config.max_memory_summary_bytes,
            );
            sections.push(format!(
                "Saved thread memory:\nTrace summary:\n{trace_summary}\n\nMemory summary:\n{memory_summary}"
            ));
        }

        if config.max_project_memories > 0
            && let Some(memories) = state_db::get_last_n_thread_memories_for_cwd(
                sess.state_db().as_deref(),
                turn_context.cwd.as_path(),
                config.max_project_memories.saturating_add(1),
                "context_packet_project_memory",
            )
            .await
        {
            let mut selected = Vec::new();
            for memory in memories {
                if memory.thread_id == sess.conversation_id {
                    continue;
                }
                selected.push(memory);
                if selected.len() >= config.max_project_memories {
                    break;
                }
            }

            if !selected.is_empty() {
                let mut out = String::new();
                for (idx, memory) in selected.into_iter().enumerate() {
                    if idx > 0 {
                        out.push('\n');
                        out.push('\n');
                    }
                    let thread_id = memory.thread_id;
                    let summary = truncate_text_bytes(
                        memory.memory_summary.trim(),
                        config.max_project_memory_summary_bytes,
                    );
                    out.push_str(&format!("- Thread {thread_id}:\n{summary}"));
                }
                sections.push(format!("Recent project memories (same cwd):\n{out}"));
            }
        }
    }

    // Add Entire summary section
    if config.include_entire_summary && config.max_entire_checkpoints > 0 {
        // Try to get checkpoints with AI summaries if enabled
        let checkpoints_result = if turn_context.config.memories.entire_summary_enabled {
            entire_integration::get_recent_entire_checkpoints_with_summaries(
                turn_context.cwd.as_path(),
                config.max_entire_checkpoints,
                Some(&sess.services.model_client),
                Some(&sess.services.models_manager),
                Some(&turn_context.config),
            )
            .await
        } else {
            entire_integration::get_recent_entire_checkpoints(
                turn_context.cwd.as_path(),
                config.max_entire_checkpoints,
            )
            .await
        };

        if let Ok(checkpoints) = checkpoints_result
            && !checkpoints.is_empty()
        {
            let summary = entire_integration::format_checkpoints_summary(&checkpoints);
            let summary = truncate_text_bytes(&summary, config.max_entire_summary_bytes);
            let summary = summary.trim();
            if !summary.is_empty() {
                sections.push(format!("Recent AI Sessions (via Entire):\n{summary}"));
            }
        }
    }

    let history = sess.clone_history().await;
    let collected = collect_history_context_from_items(
        history.raw_items(),
        config.max_message_bytes,
        config.max_recent_messages,
        config.max_recent_bytes,
    );
    if let Some(summary) = collected.summary {
        let summary = truncate_text_bytes(summary.trim(), config.max_session_summary_bytes)
            .trim()
            .to_string();
        if !summary.is_empty() {
            sections.push(format!("Session summary:\n{summary}"));
        }
    }
    if !collected.recent.is_empty() {
        sections.push(format!(
            "Recent chat excerpt:\n{}",
            format_role_prefixed_messages(&collected.recent).trim()
        ));
    }

    truncate_context(sections.join("\n\n"), config)
}

struct HistoryContext {
    summary: Option<String>,
    recent: Vec<(String, String)>,
}

fn collect_history_context_from_items(
    items: &[ResponseItem],
    max_message_bytes: usize,
    max_recent_messages: usize,
    max_recent_bytes: usize,
) -> HistoryContext {
    let mut summary: Option<String> = None;
    let mut boundary = 0usize;
    let mut messages: Vec<(String, String)> = Vec::new();

    let summary_prefix = format!("{}\n", compact::SUMMARY_PREFIX);

    for item in items {
        let ResponseItem::Message { role, content, .. } = item else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(text) = compact::content_items_to_text(content) else {
            continue;
        };

        if role == "user" && compact::is_summary_message(&text) {
            let suffix = text
                .strip_prefix(&summary_prefix)
                .unwrap_or(text.as_str())
                .to_string();
            summary = Some(suffix);
            boundary = messages.len();
            continue;
        }

        messages.push((role.clone(), truncate_text_bytes(&text, max_message_bytes)));
    }

    let after_summary = &messages[boundary..];
    let recent =
        take_last_messages_with_byte_budget(after_summary, max_recent_messages, max_recent_bytes);

    HistoryContext { summary, recent }
}

fn take_last_messages_with_byte_budget(
    messages: &[(String, String)],
    max_messages: usize,
    max_bytes: usize,
) -> Vec<(String, String)> {
    if max_messages == 0 || max_bytes == 0 {
        return Vec::new();
    }

    let mut used = 0usize;
    let mut selected_rev = Vec::new();
    for (role, text) in messages.iter().rev().take(max_messages) {
        // Roughly account for formatting overhead ("User: " + "\n\n", etc.).
        let cost = role.len().saturating_add(text.len()).saturating_add(16);
        if !selected_rev.is_empty() && used.saturating_add(cost) > max_bytes {
            break;
        }
        selected_rev.push((role.clone(), text.clone()));
        used = used.saturating_add(cost);
    }
    selected_rev.reverse();
    selected_rev
}

fn truncate_text_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let truncated = take_bytes_at_char_boundary(text, max_bytes);
    format!("{truncated}...")
}

fn format_role_prefixed_messages(messages: &[(String, String)]) -> String {
    let mut out = String::new();
    for (idx, (role, text)) in messages.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
            out.push('\n');
        }
        let label = match role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            other => other,
        };
        out.push_str(&format!("{label}: {}\n", text.trim()));
    }
    out
}

fn truncate_context(mut context: String, config: ContextPacketConfig) -> String {
    if context.len() <= config.max_context_bytes {
        return context;
    }

    let notice_len = config.truncation_notice.len();
    let budget = config.max_context_bytes.saturating_sub(notice_len);
    let truncated = take_last_bytes_at_char_boundary(&context, budget);
    context = format!("{}{truncated}", config.truncation_notice);
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::ContentItem;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    #[test]
    fn collect_history_context_prefers_last_summary_and_only_keeps_messages_after_it() {
        let summary_prefix = format!("{}\n", compact::SUMMARY_PREFIX);
        let first_summary = format!("{summary_prefix}summary one");
        let second_summary = format!("{summary_prefix}summary two");

        let items = vec![
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "before summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: first_summary,
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "after first summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: second_summary,
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "after second summary".to_string(),
                }],
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];

        let ctx = collect_history_context_from_items(&items, 1_000, 10, 10_000);
        assert_eq!(ctx.summary.as_deref(), Some("summary two"));
        assert_eq!(ctx.recent.len(), 1);
        assert_eq!(ctx.recent[0].0, "user".to_string());
        assert_eq!(ctx.recent[0].1, "after second summary".to_string());
    }

    #[test]
    fn render_active_memory_scope_section_includes_scope_version_and_root() {
        let summary_sha256 = "a".repeat(64);
        let section = render_active_memory_scope_section(
            "user",
            "user:123456789abc",
            &summary_sha256,
            "user:123456789abc:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Path::new("/tmp/.codex/memories/user/memory"),
            "memory summary text",
            1_000,
        );

        let expected = "Active memory scope: user\n\
Active memory scope version: user:123456789abc\n\
Active memory summary sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
Active memory binding key: user:123456789abc:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
Active memory root: /tmp/.codex/memories/user/memory\n\
Active memory summary:\n\
memory summary text";
        assert_eq!(section, expected);
    }

    #[test]
    fn render_active_memory_scope_section_truncates_summary() {
        let summary_sha256 = "a".repeat(64);
        let section = render_active_memory_scope_section(
            "cwd",
            "cwd:aaaaaaaaaaaa",
            &summary_sha256,
            "cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Path::new("/tmp/project/.codex/memory"),
            "abcdef",
            4,
        );

        assert_eq!(
            section,
            "Active memory scope: cwd\n\
Active memory scope version: cwd:aaaaaaaaaaaa\n\
Active memory summary sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
Active memory binding key: cwd:aaaaaaaaaaaa:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
Active memory root: /tmp/project/.codex/memory\n\
Active memory summary:\n\
abcd..."
        );
    }
}
