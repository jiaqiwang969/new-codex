import re

with open('core/src/codex.rs', 'r') as f:
    content = f.read()

target = """                        sess.notify_background_event(
                            &turn_context,
                            "Generating Entire session summary...".to_string(),
                        )
                        .await;

                        let user_prompt = sampling_request_input_messages.join("\\n");
                        let ai_response = last_agent_message.clone().unwrap_or_default();

                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> = side_effects_guard.iter().cloned().collect();
                        drop(side_effects_guard);"""

replacement = """                        let user_prompt = sampling_request_input_messages.join("\\n");
                        let ai_response = last_agent_message.clone().unwrap_or_default();

                        let side_effects_guard = turn_context.side_effects_files.lock().await;
                        let files_changed: Vec<String> = side_effects_guard.iter().cloned().collect();
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

content = content.replace(target, replacement)

with open('core/src/codex.rs', 'w') as f:
    f.write(content)
