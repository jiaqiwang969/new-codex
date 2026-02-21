import re

with open('core/src/tools/handlers/unified_exec.rs', 'r') as f:
    content3 = f.read()

target_u1 = """                manager
                    .exec_command(
                        ExecCommandRequest {
                            command,
                            process_id,
                            yield_time_ms,
                            max_output_tokens,
                            workdir,
                            network: context.turn.network.clone(),
                            tty,
                            sandbox_permissions,
                            justification,
                            prefix_rule,
                        },
                        &context,
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
                                workdir,
                                network: context.turn.network.clone(),
                                tty,
                                sandbox_permissions,
                                justification,
                                prefix_rule,
                            },
                            &context,
                        ).await
                    }
                ).await"""
content3 = content3.replace(target_u1, replacement_u1)

target_u2 = """                manager
                    .write_stdin(WriteStdinRequest {
                        process_id: &args.session_id.to_string(),
                        input: &args.chars,
                        yield_time_ms: args.yield_time_ms,
                        max_output_tokens: args.max_output_tokens,
                    })
                    .await"""
replacement_u2 = """                let cwd_clone = context.turn.cwd.clone();
                let call_id_clone = context.call_id.clone();
                let session_clone = context.session.clone();
                let turn_clone = context.turn.clone();
                crate::git_side_effects::track_tool_side_effects(
                    &cwd_clone,
                    call_id_clone,
                    session_clone.as_ref(),
                    turn_clone.as_ref(),
                    || async {
                        manager.write_stdin(WriteStdinRequest {
                            process_id: &args.session_id.to_string(),
                            input: &args.chars,
                            yield_time_ms: args.yield_time_ms,
                            max_output_tokens: args.max_output_tokens,
                        }).await
                    }
                ).await"""
content3 = content3.replace(target_u2, replacement_u2)

with open('core/src/tools/handlers/unified_exec.rs', 'w') as f:
    f.write(content3)
