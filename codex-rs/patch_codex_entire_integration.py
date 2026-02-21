import re

with open('core/src/entire_integration.rs', 'r') as f:
    content = f.read()

target = """                spawn_summary_generation(
                    &checkpoint,
                    cwd.to_path_buf(),
                    client.clone(),
                    Arc::clone(manager),
                    cfg.clone(),
                );"""

replacement = """                let is_trivial_prompt = checkpoint.prompt_summary.len() < 10 
                    && (checkpoint.prompt_summary.to_lowercase().contains("hi") || checkpoint.prompt_summary.to_lowercase().contains("hello")) 
                    && checkpoint.files_changed.is_empty();
                
                if !is_trivial_prompt {
                    // Generate summary asynchronously in background
                    spawn_summary_generation(
                        &checkpoint,
                        cwd.to_path_buf(),
                        client.clone(),
                        Arc::clone(manager),
                        cfg.clone(),
                    );
                }"""

content = content.replace(target, replacement)

with open('core/src/entire_integration.rs', 'w') as f:
    f.write(content)
