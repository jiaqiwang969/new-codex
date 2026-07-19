use super::*;

impl ChatWidget {
    pub(super) fn handle_ralph_loop_command(&mut self, cmd: crate::ralph_loop::RalphLoopCommand) {
        if self.ralph_loop_state.is_some() {
            self.add_error_message(
                "A Ralph Loop is already active. Use /cancel-ralph to stop it first.".to_string(),
            );
            return;
        }

        let prompt = cmd.prompt.unwrap_or_else(|| {
            "Continue working on the task. Review your previous output and files. If everything is complete, output <promise>COMPLETE</promise>."
                .to_string()
        });
        let target = self
            .thread_id
            .map(crate::ralph_loop::RalphLoopTarget::Thread)
            .unwrap_or(crate::ralph_loop::RalphLoopTarget::ActiveThread);
        let state = crate::ralph_loop::RalphLoopState::new(
            target,
            cmd.max_iterations,
            cmd.completion_promise.clone(),
            prompt.clone(),
            cmd.delay_seconds,
        );

        if let Some(cwd) = &self.current_cwd {
            crate::ralph_loop::save_ralph_state_file(cwd, &state);
        }

        let max_display = if cmd.max_iterations == 0 {
            "unlimited".to_string()
        } else {
            cmd.max_iterations.to_string()
        };
        let completion_promise = &cmd.completion_promise;
        let delay_seconds = cmd.delay_seconds;
        self.add_info_message(
            format!(
                "Ralph Loop activated: max={max_display}, promise=\"{completion_promise}\", delay={delay_seconds}s"
            ),
            Some(
                "The prompt will be re-submitted after each turn until the promise is detected. Use /ralph-loop -n <N> (or --max <N>) to set the max loop count."
                    .to_string(),
            ),
        );

        self.ralph_loop_state = Some(state);
        self.submit_user_message(prompt.into());
    }

    pub(super) fn handle_cancel_ralph_command(&mut self) {
        let Some(iteration) = self.ralph_loop_state.as_ref().map(|state| state.iteration) else {
            self.add_error_message("No active Ralph Loop to cancel.".to_string());
            return;
        };

        self.finish_ralph_loop(format!(
            "Ralph Loop cancelled after {iteration} iteration(s)."
        ));
        self.request_redraw();
    }

    fn finish_ralph_loop(&mut self, message: String) {
        self.ralph_loop_state = None;
        self.ralph_loop_turn_had_error = false;
        if let Some(cwd) = &self.current_cwd {
            crate::ralph_loop::cleanup_ralph_state_file(cwd);
        }
        self.add_info_message(message, /*hint*/ None);
    }

    fn maybe_finish_ralph_loop(&mut self, last_agent_message: Option<&str>) -> bool {
        let Some(state) = self
            .ralph_loop_state
            .as_ref()
            .filter(|state| state.enabled)
            .cloned()
        else {
            return false;
        };

        if let Some(output) = last_agent_message
            && crate::ralph_loop::check_completion_promise(output, &state.completion_promise)
        {
            let iteration = state.iteration;
            self.finish_ralph_loop(format!(
                "Ralph Loop complete: promise detected after {iteration} iteration(s)."
            ));
            return true;
        }

        if !state.should_continue() {
            let iteration = state.iteration;
            let max = state.max_iterations;
            self.finish_ralph_loop(format!(
                "Ralph Loop stopped: reached max iterations ({iteration}/{max})."
            ));
            return true;
        }

        false
    }

    fn continue_ralph_loop(&mut self) {
        let Some(state) = self
            .ralph_loop_state
            .as_ref()
            .filter(|state| state.enabled)
            .cloned()
        else {
            return;
        };

        let prompt = state.original_prompt.clone();
        let delay = state.delay_seconds;
        let target = state.target().clone();
        let instance_id = state.instance_id().to_string();
        let had_error = self.ralph_loop_turn_had_error;

        let mut pending_generation = None;
        if let Some(loop_state) = self.ralph_loop_state.as_mut() {
            loop_state.next_iteration();
            if had_error && delay > 0 {
                pending_generation = Some(loop_state.schedule_retry());
            } else {
                loop_state.clear_pending_retry();
            }
            if let Some(cwd) = &self.current_cwd {
                crate::ralph_loop::save_ralph_state_file(cwd, loop_state);
            }

            let iteration = loop_state.iteration;
            let max = loop_state.max_iterations;
            let max_display = if max == 0 {
                "unlimited".to_string()
            } else {
                max.to_string()
            };
            self.add_info_message(
                format!("Ralph Loop: starting iteration {iteration}/{max_display}"),
                /*hint*/ None,
            );
        }

        self.ralph_loop_turn_had_error = false;

        if let Some(generation) = pending_generation {
            let tx = self.app_event_tx.clone();
            self.add_info_message(
                format!("Ralph Loop: error detected, waiting {delay}s before retry..."),
                /*hint*/ None,
            );
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                tx.send(AppEvent::RalphLoopDelayedContinue {
                    target,
                    instance_id,
                    generation,
                });
            });
        } else {
            self.enqueue_ralph_loop_follow_up(prompt);
        }
    }

    pub(super) fn on_task_complete_for_ralph_loop(&mut self, last_agent_message: Option<&str>) {
        if self.maybe_finish_ralph_loop(last_agent_message) {
            return;
        }

        self.continue_ralph_loop();
    }

    pub(super) fn on_failed_turn_for_ralph_loop(&mut self) {
        if !self
            .ralph_loop_state
            .as_ref()
            .is_some_and(|state| state.enabled)
        {
            return;
        }

        self.ralph_loop_turn_had_error = true;
        if self.maybe_finish_ralph_loop(/*last_agent_message*/ None) {
            return;
        }

        self.continue_ralph_loop();
    }

    pub(crate) fn handle_ralph_loop_delayed_continue(
        &mut self,
        target: crate::ralph_loop::RalphLoopTarget,
        instance_id: &str,
        generation: u64,
    ) {
        let Some(state) = self.ralph_loop_state.as_ref() else {
            return;
        };
        if !state.enabled
            || state.target() != &target
            || !target.matches_current_thread(self.thread_id)
            || !state.matches_pending_retry(instance_id, generation)
        {
            return;
        }

        let prompt = state.original_prompt.clone();
        if let Some(loop_state) = self.ralph_loop_state.as_mut() {
            loop_state.clear_pending_retry();
            if let Some(cwd) = &self.current_cwd {
                crate::ralph_loop::save_ralph_state_file(cwd, loop_state);
            }
        }
        self.queue_user_message(prompt.into());
    }

    pub(crate) fn maybe_resume_pending_ralph_loop_retry_if_due(&mut self) {
        let Some((target, instance_id, generation)) =
            self.ralph_loop_state.as_ref().and_then(|state| {
                state.pending_retry_due_now().map(|generation| {
                    (
                        state.target().clone(),
                        state.instance_id().to_string(),
                        generation,
                    )
                })
            })
        else {
            return;
        };

        self.handle_ralph_loop_delayed_continue(target, &instance_id, generation);
    }

    fn enqueue_ralph_loop_follow_up(&mut self, prompt: String) {
        self.input_queue
            .queued_user_messages
            .push_back(QueuedUserMessage::from(UserMessage::from(prompt)));
        self.input_queue
            .queued_user_message_history_records
            .push_back(UserMessageHistoryRecord::UserMessageText);
        self.refresh_pending_input_preview();
    }
}
