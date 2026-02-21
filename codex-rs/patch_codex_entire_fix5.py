import re

with open('core/src/codex.rs', 'r') as f:
    content = f.read()

target = """                        sess.notify_background_event(
                            &turn_context,
                            "Generating Entire session summary...".to_string(),
                        )
                        .await;"""

replacement = """                        if !sampling_request_input_messages.is_empty() && 
                           !sampling_request_input_messages.iter().all(|m| m.len() < 5 && (m.to_lowercase().contains("hi") || m.to_lowercase().contains("hello"))) {
                            sess.notify_background_event(
                                &turn_context,
                                "Generating Entire session summary...".to_string(),
                            )
                            .await;
                        }"""
content = content.replace(target, replacement)

with open('core/src/codex.rs', 'w') as f:
    f.write(content)
