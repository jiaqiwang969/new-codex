import re

with open('core/src/codex.rs', 'r') as f:
    content = f.read()

# 1. Add to struct
content = re.sub(
    r"pub\(crate\) turn_metadata_state: Arc<TurnMetadataState>,\n\}",
    "pub(crate) turn_metadata_state: Arc<TurnMetadataState>,\n    pub(crate) side_effects_files: std::sync::Arc<tokio::sync::Mutex<std::collections::BTreeSet<String>>>,\n}",
    content
)

# 2. Add to TurnContext::new
content = re.sub(
    r"turn_metadata_state: Arc::new\(TurnMetadataState::default\(\)\),\n\s*\}",
    "turn_metadata_state: Arc::new(TurnMetadataState::default()),\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),\n        }",
    content
)

# 3. Add to TurnContext for review
content = re.sub(
    r"let review_turn_context = TurnContext \{\n\s*sub_id: uuid::Uuid::new_v4\(\)\.to_string\(\),",
    "let review_turn_context = TurnContext {\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),\n            sub_id: uuid::Uuid::new_v4().to_string(),",
    content
)

# 4. Add to OverrideTurnContext
content = re.sub(
    r"Self \{\n\s*sub_id,\n\s*config: config\.clone\(\),",
    "Self {\n            sub_id,\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),\n            config: config.clone(),",
    content
)

# 5. Inject files_changed to EntireSummaryInput
target = """                        let input = codex_hooks::EntireSummaryInput {
                            thread_id: sess.conversation_id.to_string(),
                            turn_id: turn_context.sub_id.clone(),
                            user_prompt,
                            ai_response,
                            files_changed: vec![],
                        };"""
replacement = """                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> = side_effects_guard.iter().cloned().collect();
                        drop(side_effects_guard);

                        let input = codex_hooks::EntireSummaryInput {
                            thread_id: sess.conversation_id.to_string(),
                            turn_id: turn_context.sub_id.clone(),
                            user_prompt,
                            ai_response,
                            files_changed,
                        };"""
content = content.replace(target, replacement)

with open('core/src/codex.rs', 'w') as f:
    f.write(content)
