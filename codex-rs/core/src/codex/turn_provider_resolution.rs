use super::*;

impl Session {
    pub(super) async fn new_turn_from_configuration(
        &self,
        sub_id: String,
        session_configuration: SessionConfiguration,
        final_output_json_schema: Option<Option<Value>>,
        sandbox_policy_changed: bool,
    ) -> Arc<TurnContext> {
        let resolved_provider = {
            let mut state = self.state.lock().await;
            state.resolve_turn_provider(
                &session_configuration.provider_id,
                &session_configuration.provider,
                std::time::Instant::now(),
            )
        };
        let background_message = resolved_provider.background_message.clone();
        // Box this nested async call so startup/resume turn creation does not
        // inline the full future chain onto a small test-thread stack.
        let turn_context = Box::pin(self.new_turn_from_resolved_provider(
            sub_id,
            session_configuration,
            resolved_provider.provider,
            final_output_json_schema,
            sandbox_policy_changed,
        ))
        .await;
        if let Some(message) = background_message {
            self.notify_background_event(&turn_context, message).await;
        }
        turn_context
    }

    pub(super) async fn new_turn_from_resolved_provider(
        &self,
        sub_id: String,
        session_configuration: SessionConfiguration,
        provider: ModelProviderInfo,
        final_output_json_schema: Option<Option<Value>>,
        sandbox_policy_changed: bool,
    ) -> Arc<TurnContext> {
        let per_turn_config = Self::build_per_turn_config(&session_configuration);
        self.services
            .mcp_connection_manager
            .read()
            .await
            .set_approval_policy(&session_configuration.approval_policy);

        if sandbox_policy_changed {
            let sandbox_state = SandboxState {
                sandbox_policy: per_turn_config.permissions.sandbox_policy.get().clone(),
                codex_linux_sandbox_exe: per_turn_config.codex_linux_sandbox_exe.clone(),
                sandbox_cwd: per_turn_config.cwd.to_path_buf(),
                use_legacy_landlock: per_turn_config.features.use_legacy_landlock(),
            };
            if let Err(e) = self
                .services
                .mcp_connection_manager
                .read()
                .await
                .notify_sandbox_state_change(&sandbox_state)
                .await
            {
                warn!("Failed to notify sandbox state change to MCP servers: {e:#}");
            }
        }

        let model_info = self
            .services
            .models_manager
            .get_model_info(
                session_configuration.collaboration_mode.model(),
                &per_turn_config,
            )
            .await;
        let plugin_outcome = self
            .services
            .plugins_manager
            .plugins_for_config(&per_turn_config);
        let effective_skill_roots = plugin_outcome.effective_skill_roots();
        let skills_input = skills_load_input_from_config(&per_turn_config, effective_skill_roots);
        let skills_outcome = Arc::new(
            self.services
                .skills_manager
                .skills_for_config(&skills_input),
        );
        let mut turn_context: TurnContext = Self::make_turn_context(
            self.conversation_id,
            Some(Arc::clone(&self.services.auth_manager)),
            &self.services.session_telemetry,
            provider,
            &session_configuration,
            self.services.user_shell.as_ref(),
            self.services.shell_zsh_path.as_ref(),
            self.services.main_execve_wrapper_exe.as_ref(),
            per_turn_config,
            model_info,
            &self.services.models_manager,
            self.services
                .network_proxy
                .as_ref()
                .map(StartedNetworkProxy::proxy),
            Arc::clone(&self.services.environment),
            sub_id,
            Arc::clone(&self.js_repl),
            skills_outcome,
        );
        turn_context.realtime_active = self.conversation.running_state().await.is_some();

        if let Some(final_schema) = final_output_json_schema {
            turn_context.final_output_json_schema = final_schema;
        }
        let turn_context = Arc::new(turn_context);
        turn_context.turn_metadata_state.spawn_git_enrichment_task();
        turn_context
    }

    pub(crate) async fn provider(&self) -> ModelProviderInfo {
        let mut state = self.state.lock().await;
        let provider_id = state.session_configuration.provider_id.clone();
        let provider = state.session_configuration.provider.clone();
        state
            .resolve_turn_provider(&provider_id, &provider, std::time::Instant::now())
            .provider
    }

    pub(crate) async fn utility_client_and_model_for_slug(
        &self,
        config: &Config,
        model_slug: &str,
    ) -> Option<(ModelClient, ModelInfo, String)> {
        let (provider_id, logical_provider) =
            crate::utility_model::provider_for_model_slug(config, model_slug)?;
        let model_info = self
            .services
            .models_manager
            .get_model_info(model_slug, config)
            .await;
        let resolved_provider = {
            let mut state = self.state.lock().await;
            state.resolve_turn_provider(&provider_id, &logical_provider, std::time::Instant::now())
        };
        let model_client = self
            .services
            .model_client
            .clone_with_provider(resolved_provider.provider);
        Some((model_client, model_info, provider_id))
    }

    pub(super) async fn entire_summary_client_and_model_for_turn(
        &self,
        turn_context: &TurnContext,
    ) -> (ModelClient, ModelInfo, String, Option<String>) {
        let model_slug =
            crate::entire_summary_generator::model_slug(turn_context.config.as_ref()).to_string();
        let (summary_turn_context, background_message) = self
            .turn_context_with_model_resolved_from_pool(turn_context, model_slug.clone())
            .await;
        let model_client = self
            .services
            .model_client
            .clone_with_provider(summary_turn_context.provider.clone());
        (
            model_client,
            summary_turn_context.model_info,
            model_slug,
            background_message,
        )
    }

    pub(super) async fn turn_context_with_model_resolved_from_pool(
        &self,
        turn_context: &TurnContext,
        model: String,
    ) -> (TurnContext, Option<String>) {
        let mut next_turn_context = turn_context
            .with_model(model, &self.services.models_manager)
            .await;
        let resolved_provider = {
            let mut state = self.state.lock().await;
            state.resolve_turn_provider(
                &next_turn_context.config.model_provider_id,
                &next_turn_context.config.model_provider,
                std::time::Instant::now(),
            )
        };
        next_turn_context.provider = resolved_provider.provider;
        (next_turn_context, resolved_provider.background_message)
    }
}
