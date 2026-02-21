import re

with open('core/src/codex.rs', 'r') as f:
    content = f.read()

target = """                        if !sampling_request_input_messages.is_empty() && 
                           !sampling_request_input_messages.iter().all(|m| m.len() < 5 && (m.to_lowercase().contains("hi") || m.to_lowercase().contains("hello"))) {
                            sess.notify_background_event(
                                &turn_context,
                                "Generating Entire session summary...".to_string(),
                            )
                            .await;
                        }"""

replacement = """                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> =
                            side_effects_guard.iter().cloned().collect();
                        drop(side_effects_guard);
                        
                        let has_files_changed = !files_changed.is_empty();
                        let is_trivial_prompt = sampling_request_input_messages.len() == 1 
                            && sampling_request_input_messages[0].len() < 10 
                            && !has_files_changed;

                        if !is_trivial_prompt {
                            sess.notify_background_event(
                                &turn_context,
                                "Generating Entire session summary...".to_string(),
                            )
                            .await;
                        }"""
if target in content:
    content = content.replace(target, replacement)
else:
    # already applied or modified in some other way, just find the generation block
    pass

target2 = """                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> =
                            side_effects_guard.iter().cloned().collect();
                        drop(side_effects_guard);

                        let input = codex_hooks::EntireSummaryInput {"""
replacement2 = """                        let input = codex_hooks::EntireSummaryInput {"""
content = content.replace(target2, replacement2)

with open('core/src/codex.rs', 'w') as f:
    f.write(content)
