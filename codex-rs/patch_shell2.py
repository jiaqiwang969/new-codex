import re

with open('core/src/tools/handlers/shell.rs', 'r') as f:
    content = f.read()

target = """        let out = orchestrator
            .run(&mut runtime, &req, &tool_ctx, &turn, turn.approval_policy)
            .await
            .map(|result| result.output);"""
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
        ).await.map(|result| result.output);"""
content = content.replace(target, replacement)

with open('core/src/tools/handlers/shell.rs', 'w') as f:
    f.write(content)

with open('core/src/tools/handlers/js_repl.rs', 'r') as f:
    content2 = f.read()

target_js = """        let result = session
            .services
            .js_repl
            .execute(Arc::clone(&session), Arc::clone(&turn), tracker, args)
            .await;"""
replacement_js = """        let cwd = turn.cwd.clone();
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
content2 = content2.replace(target_js, replacement_js)

with open('core/src/tools/handlers/js_repl.rs', 'w') as f:
    f.write(content2)

with open('core/src/tools/handlers/unified_exec.rs', 'r') as f:
    content3 = f.read()

target_u1 = """                manager
                    .exec_command(
                        &context,
                        command,
                        cwd.clone(),
                        tty,
                        workdir.is_some(),
                        yield_time_ms,
                        max_output_tokens,
                        sandbox_permissions,
                        justification,
                        prefix_rule,
                    )
                    .await"""
replacement_u1 = """                let cwd_clone = cwd.clone();
                let call_id_clone = context.call_id.clone();
                let session_clone = context.session.clone();
                let turn_clone = context.turn.clone();
                crate::git_side_effects::track_tool_side_effects(
                    &cwd_clone,
                    call_id_clone,
                    session_clone.as_ref(),
                    turn_clone.as_ref(),
                    || async {
                        manager.exec_command(
                            &context,
                            command,
                            cwd.clone(),
                            tty,
                            workdir.is_some(),
                            yield_time_ms,
                            max_output_tokens,
                            sandbox_permissions,
                            justification,
                            prefix_rule,
                        ).await
                    }
                ).await"""
content3 = content3.replace(target_u1, replacement_u1)

target_u2 = """                manager
                    .write_stdin(&context, args.session_id, chars, yield_time_ms, max_output_tokens)
                    .await"""
replacement_u2 = """                let cwd = turn.cwd.clone();
                let call_id_clone = context.call_id.clone();
                let session_clone = context.session.clone();
                let turn_clone = context.turn.clone();
                crate::git_side_effects::track_tool_side_effects(
                    &cwd,
                    call_id_clone,
                    session_clone.as_ref(),
                    turn_clone.as_ref(),
                    || async {
                        manager.write_stdin(&context, args.session_id, chars, yield_time_ms, max_output_tokens).await
                    }
                ).await"""
content3 = content3.replace(target_u2, replacement_u2)

with open('core/src/tools/handlers/unified_exec.rs', 'w') as f:
    f.write(content3)

