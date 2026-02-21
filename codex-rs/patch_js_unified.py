import re

with open('core/src/tools/handlers/js_repl.rs', 'r') as f:
    content2 = f.read()

target_js = """        let result = manager
            .execute(Arc::clone(&session), Arc::clone(&turn), tracker, args)
            .await;"""
replacement_js = """        let cwd = turn.cwd.clone();
        let call_id_clone = call_id.clone();
        let session_clone = session.clone();
        let turn_clone = turn.clone();
        let manager_clone = manager.clone();
        
        let result = crate::git_side_effects::track_tool_side_effects(
            &cwd,
            call_id_clone,
            session_clone.as_ref(),
            turn_clone.as_ref(),
            || async {
                manager_clone.execute(Arc::clone(&session_clone), Arc::clone(&turn_clone), tracker, args).await
            }
        ).await;"""
content2 = content2.replace(target_js, replacement_js)

with open('core/src/tools/handlers/js_repl.rs', 'w') as f:
    f.write(content2)


with open('core/src/tools/handlers/unified_exec.rs', 'r') as f:
    content3 = f.read()

target_u1 = """                manager
                    .exec_command(
                        ExecCommandRequest {
                            command,
                            process_id,
                            yield_time_ms,
                            max_output_tokens,
                            tty,
                            workdir: workdir.is_some(),
                            sandbox_permissions,
                            justification,
                            prefix_rule,
                        },
                        &context,
                        cwd,
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
                            ExecCommandRequest {
                                command,
                                process_id,
                                yield_time_ms,
                                max_output_tokens,
                                tty,
                                workdir: workdir.is_some(),
                                sandbox_permissions,
                                justification,
                                prefix_rule,
                            },
                            &context,
                            cwd,
                        ).await
                    }
                ).await"""
content3 = content3.replace(target_u1, replacement_u1)

target_u2 = """                manager
                    .write_stdin(WriteStdinRequest {
                        session_id: args.session_id,
                        chars,
                        yield_time_ms,
                        max_output_tokens,
                        context: &context,
                    })
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
                        manager.write_stdin(WriteStdinRequest {
                            session_id: args.session_id,
                            chars,
                            yield_time_ms,
                            max_output_tokens,
                            context: &context,
                        }).await
                    }
                ).await"""
content3 = content3.replace(target_u2, replacement_u2)

with open('core/src/tools/handlers/unified_exec.rs', 'w') as f:
    f.write(content3)
