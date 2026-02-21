import re

with open('core/src/tools/handlers/shell.rs', 'r') as f:
    content = f.read()

target = """        let out = orchestrator
            .run(&mut runtime, &req, &tool_ctx, &turn, turn.approval_policy)
            .await;"""
replacement = """        let cwd = exec_params.cwd.clone();
        let call_id_clone = call_id.clone();
        let session_clone = session.clone();
        let turn_clone = turn.clone();
        let out = crate::git_side_effects::track_tool_side_effects(
            &cwd,
            call_id_clone,
            session_clone.as_ref(),
            turn_clone.as_ref(),
            || async {
                orchestrator
                    .run(&mut runtime, &req, &tool_ctx, &turn, turn.approval_policy)
                    .await
            }
        ).await;"""
content = content.replace(target, replacement)

with open('core/src/tools/handlers/shell.rs', 'w') as f:
    f.write(content)
