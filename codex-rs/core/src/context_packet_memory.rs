use crate::codex::Session;
use crate::codex::TurnContext;
use crate::context_packet::ContextPacketConfig;
use crate::context_packet::truncate_text_bytes;
use crate::state_db;
use codex_features::Feature;

pub(crate) async fn build_memory_sections(
    sess: &Session,
    turn_context: &TurnContext,
    config: ContextPacketConfig,
) -> Vec<String> {
    if !turn_context.features.enabled(Feature::MemoryTool) {
        return Vec::new();
    }

    let mut sections = Vec::new();

    if let Some(active_memory_source) = turn_context.resolve_memory_read_path_source().await {
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

    let state_db = sess.state_db();
    let Some(state_db) = state_db.as_deref() else {
        return sections;
    };

    if let Some(memory) = state_db::get_thread_memory(
        Some(state_db),
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
            Some(state_db),
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

    sections
}

pub(crate) fn render_active_memory_scope_section(
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
