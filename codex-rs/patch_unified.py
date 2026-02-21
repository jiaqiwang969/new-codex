import re

with open('core/src/tools/handlers/unified_exec.rs', 'r') as f:
    content = f.read()

target = """                manager
                    .exec_command(
                        &context,
                        command,
                        cwd,
                        tty,
                        workdir.is_some(),
                        yield_time_ms,
                        max_output_tokens,
                        sandbox_permissions,
                        justification,
                        prefix_rule,
                    )
                    .await"""
replacement = """                let cwd_clone = cwd.clone();
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
                            cwd,
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
content = content.replace(target, replacement)

target2 = """                manager
                    .write_stdin(&context, args.session_id, chars, yield_time_ms, max_output_tokens)
                    .await"""
replacement2 = """                let cwd = turn.cwd.clone();
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
content = content.replace(target2, replacement2)

with open('core/src/tools/handlers/unified_exec.rs', 'w') as f:
    f.write(content)
