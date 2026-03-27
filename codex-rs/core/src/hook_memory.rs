use codex_hooks::HookEventMemoryContext;
use codex_protocol::protocol::MemoryLink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookMemoryFields {
    pub(crate) memory: Option<MemoryLink>,
    pub(crate) memory_scope_version: Option<String>,
    pub(crate) memory_scope_kind: Option<String>,
    pub(crate) memory_summary_sha256: Option<String>,
    pub(crate) memory_binding_key: Option<String>,
    pub(crate) memory_context: Option<HookEventMemoryContext>,
}

impl HookMemoryFields {
    pub(crate) fn from_context(memory_context: Option<HookEventMemoryContext>) -> Self {
        let memory = memory_link_from_context(memory_context.as_ref());
        let memory_scope_version = memory_context
            .as_ref()
            .and_then(|memory_context| memory_context.active_memory_scope_version.clone());
        let memory_scope_kind = memory_context
            .as_ref()
            .and_then(|memory_context| memory_context.active_scope_kind.clone());
        let memory_summary_sha256 = memory_context
            .as_ref()
            .and_then(|memory_context| memory_context.active_memory_summary_sha256.clone());
        let memory_binding_key = memory_context
            .as_ref()
            .and_then(|memory_context| memory_context.active_memory_binding_key.clone());

        Self {
            memory,
            memory_scope_version,
            memory_scope_kind,
            memory_summary_sha256,
            memory_binding_key,
            memory_context,
        }
    }
}

pub(crate) fn memory_link_from_context(
    memory_context: Option<&HookEventMemoryContext>,
) -> Option<MemoryLink> {
    let scope_version = memory_context
        .and_then(|memory_context| memory_context.active_memory_scope_version.clone());
    let binding_key =
        memory_context.and_then(|memory_context| memory_context.active_memory_binding_key.clone());

    if scope_version.is_none() && binding_key.is_none() {
        return None;
    }

    Some(MemoryLink {
        scope_version,
        binding_key,
    })
}
