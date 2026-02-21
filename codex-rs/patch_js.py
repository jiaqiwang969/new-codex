import re

with open('core/src/tools/handlers/js_repl.rs', 'r') as f:
    content = f.read()

target = """        let result = session
            .services
            .js_repl
            .execute(Arc::clone(&session), Arc::clone(&turn), tracker, args)
            .await;"""
replacement = """        let cwd = turn.cwd.clone();
        let call_id_clone = call_id.clone();
        let session_clone = session.clone();
        let turn_clone = turn.clone();
        
        let result = crate::git_side_effects::track_tool_side_effects(
            &cwd,
            call_id_clone,
            session_clone.as_ref(),
            turn_clone.as_ref(),
            || async {
                session.services.js_repl.execute(Arc::clone(&session_clone), Arc::clone(&turn_clone), tracker, args).await
            }
        ).await;"""
content = content.replace(target, replacement)

with open('core/src/tools/handlers/js_repl.rs', 'w') as f:
    f.write(content)
