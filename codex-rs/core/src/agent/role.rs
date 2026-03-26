//! Applies agent-role configuration layers on top of an existing session config.
//!
//! Roles are selected at spawn time and are loaded with the same config machinery as
//! `config.toml`. This module resolves built-in and user-defined role files, inserts the role as a
//! high-precedence layer, and preserves the caller's current profile/provider unless the role
//! explicitly takes ownership of model selection. It does not decide when to spawn a sub-agent or
//! which role to use; the multi-agent tool handler owns that orchestration.

use crate::config::AgentRoleConfig;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::config::agent_roles::parse_agent_role_file_contents;
use crate::config::deserialize_config_toml_with_base;
use crate::config_loader::ConfigLayerEntry;
use crate::config_loader::ConfigLayerStack;
use crate::config_loader::ConfigLayerStackOrdering;
use crate::config_loader::resolve_relative_paths_in_config_toml;
use anyhow::anyhow;
use codex_app_server_protocol::ConfigLayerSource;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;
use toml::Value as TomlValue;

const BUILT_IN_CLAUDE_OPUS_CONFIG: &str = include_str!("builtins/claude-opus.toml");
const BUILT_IN_CLAUDE_SONNET_CONFIG: &str = include_str!("builtins/claude-sonnet.toml");
const BUILT_IN_CLAUDE_HAIKU_CONFIG: &str = include_str!("builtins/claude-haiku.toml");
/// The role name used when a caller omits `agent_type`.
pub const DEFAULT_ROLE_NAME: &str = "default";
const AGENT_TYPE_UNAVAILABLE_ERROR: &str = "agent type is currently not available";

/// Applies a named role layer to `config` while preserving caller-owned model selection.
///
/// The role layer is inserted at session-flag precedence so it can override persisted config, but
/// the caller's current `profile` and `model_provider` remain sticky runtime choices unless the
/// role explicitly sets `profile`, explicitly sets `model_provider`, or rewrites the active
/// profile's `model_provider` in place. Roles that only change the selected model still reroute
/// the provider through the usual model-family-aware helper so cross-family roles keep honoring
/// user-configured providers. Rebuilding the config without those overrides would make a spawned
/// agent silently fall back to the default provider, which is the bug this preservation logic
/// avoids.
pub(crate) async fn apply_role_to_config(
    config: &mut Config,
    role_name: Option<&str>,
) -> Result<(), String> {
    let role_name = role_name.unwrap_or(DEFAULT_ROLE_NAME);

    let role = resolve_role_config(config, role_name)
        .cloned()
        .ok_or_else(|| format!("unknown agent_type '{role_name}'"))?;

    apply_role_to_config_inner(config, role_name, &role)
        .await
        .map_err(|err| {
            tracing::warn!("failed to apply role to config: {err}");
            AGENT_TYPE_UNAVAILABLE_ERROR.to_string()
        })
}

async fn apply_role_to_config_inner(
    config: &mut Config,
    role_name: &str,
    role: &AgentRoleConfig,
) -> anyhow::Result<()> {
    let is_built_in = !config.agent_roles.contains_key(role_name);
    let Some(config_file) = role.config_file.as_ref() else {
        return Ok(());
    };
    let role_layer_toml = load_role_layer_toml(config, config_file, is_built_in, role_name).await?;
    let (preserve_current_profile, preserve_current_provider, reroute_provider_for_selected_model) =
        preservation_policy(config, &role_layer_toml);

    *config = reload::build_next_config(
        config,
        role_layer_toml,
        preserve_current_profile,
        preserve_current_provider,
        reroute_provider_for_selected_model,
    )?;
    Ok(())
}

async fn load_role_layer_toml(
    config: &Config,
    config_file: &Path,
    is_built_in: bool,
    role_name: &str,
) -> anyhow::Result<TomlValue> {
    let (role_config_toml, role_config_base) = if is_built_in {
        let role_config_contents = built_in::config_file_contents(config_file)
            .map(str::to_owned)
            .ok_or(anyhow!("No corresponding config content"))?;
        let role_config_toml: TomlValue = toml::from_str(&role_config_contents)?;
        (role_config_toml, config.codex_home.as_path())
    } else {
        let role_config_contents = tokio::fs::read_to_string(config_file).await?;
        let role_config_base = config_file
            .parent()
            .ok_or(anyhow!("No corresponding config content"))?;
        let role_config_toml = parse_agent_role_file_contents(
            &role_config_contents,
            config_file,
            role_config_base,
            Some(role_name),
        )?
        .config;
        (role_config_toml, role_config_base)
    };

    deserialize_config_toml_with_base(role_config_toml.clone(), role_config_base)?;
    Ok(resolve_relative_paths_in_config_toml(
        role_config_toml,
        role_config_base,
    )?)
}

pub(crate) fn resolve_role_config<'a>(
    config: &'a Config,
    role_name: &str,
) -> Option<&'a AgentRoleConfig> {
    config
        .agent_roles
        .get(role_name)
        .or_else(|| built_in::configs().get(role_name))
}

fn preservation_policy(config: &Config, role_layer_toml: &TomlValue) -> (bool, bool, bool) {
    let role_selects_model = role_layer_toml.get("model").is_some();
    let role_selects_provider = role_layer_toml.get("model_provider").is_some();
    let role_selects_profile = role_layer_toml.get("profile").is_some();
    let role_updates_active_profile_model = config
        .active_profile
        .as_ref()
        .and_then(|active_profile| {
            role_layer_toml
                .get("profiles")
                .and_then(TomlValue::as_table)
                .and_then(|profiles| profiles.get(active_profile))
                .and_then(TomlValue::as_table)
                .map(|profile| profile.contains_key("model"))
        })
        .unwrap_or(false);
    let role_updates_active_profile_provider = config
        .active_profile
        .as_ref()
        .and_then(|active_profile| {
            role_layer_toml
                .get("profiles")
                .and_then(TomlValue::as_table)
                .and_then(|profiles| profiles.get(active_profile))
                .and_then(TomlValue::as_table)
                .map(|profile| profile.contains_key("model_provider"))
        })
        .unwrap_or(false);
    let preserve_current_profile = !role_selects_provider && !role_selects_profile;
    let preserve_current_provider =
        preserve_current_profile && !role_updates_active_profile_provider;
    let reroute_provider_for_selected_model =
        preserve_current_provider && (role_selects_model || role_updates_active_profile_model);
    (
        preserve_current_profile,
        preserve_current_provider,
        reroute_provider_for_selected_model,
    )
}

mod reload {
    use super::*;

    pub(super) fn build_next_config(
        config: &Config,
        role_layer_toml: TomlValue,
        preserve_current_profile: bool,
        preserve_current_provider: bool,
        reroute_provider_for_selected_model: bool,
    ) -> anyhow::Result<Config> {
        let active_profile_name = preserve_current_profile
            .then_some(config.active_profile.as_deref())
            .flatten();
        let config_layer_stack =
            build_config_layer_stack(config, &role_layer_toml, active_profile_name)?;
        let mut merged_config = deserialize_effective_config(config, &config_layer_stack)?;
        if preserve_current_profile {
            merged_config.profile = None;
        }

        let mut next_config = Config::load_config_with_layer_stack(
            merged_config,
            reload_overrides(config, preserve_current_provider),
            config.codex_home.clone(),
            config_layer_stack,
        )?;
        if preserve_current_profile {
            next_config.active_profile = config.active_profile.clone();
        }
        if preserve_current_provider {
            next_config.user_configured_provider = config.user_configured_provider.clone();
        }
        if reroute_provider_for_selected_model && let Some(model) = next_config.model.clone() {
            let mut provider_routing_config = next_config.clone();
            provider_routing_config.model_provider_id = config.model_provider_id.clone();
            provider_routing_config.model_provider = config.model_provider.clone();
            provider_routing_config.user_configured_provider =
                config.user_configured_provider.clone();
            if let Some((provider_id, provider)) =
                crate::utility_model::provider_for_model_slug(&provider_routing_config, &model)
            {
                next_config.model_provider_id = provider_id;
                next_config.model_provider = provider;
            }
        }
        Ok(next_config)
    }

    fn build_config_layer_stack(
        config: &Config,
        role_layer_toml: &TomlValue,
        active_profile_name: Option<&str>,
    ) -> anyhow::Result<ConfigLayerStack> {
        let mut layers = existing_layers(config);
        if let Some(resolved_profile_layer) =
            resolved_profile_layer(config, &layers, role_layer_toml, active_profile_name)?
        {
            insert_layer(&mut layers, resolved_profile_layer);
        }
        insert_layer(&mut layers, role_layer(role_layer_toml.clone()));
        Ok(ConfigLayerStack::new(
            layers,
            config.config_layer_stack.requirements().clone(),
            config.config_layer_stack.requirements_toml().clone(),
        )?)
    }

    fn resolved_profile_layer(
        config: &Config,
        existing_layers: &[ConfigLayerEntry],
        role_layer_toml: &TomlValue,
        active_profile_name: Option<&str>,
    ) -> anyhow::Result<Option<ConfigLayerEntry>> {
        let Some(active_profile_name) = active_profile_name else {
            return Ok(None);
        };

        let mut layers = existing_layers.to_vec();
        insert_layer(&mut layers, role_layer(role_layer_toml.clone()));
        let merged_config = deserialize_effective_config(
            config,
            &ConfigLayerStack::new(
                layers,
                config.config_layer_stack.requirements().clone(),
                config.config_layer_stack.requirements_toml().clone(),
            )?,
        )?;
        let resolved_profile =
            merged_config.get_config_profile(Some(active_profile_name.to_string()))?;
        Ok(Some(ConfigLayerEntry::new(
            ConfigLayerSource::SessionFlags,
            TomlValue::try_from(resolved_profile)?,
        )))
    }

    fn deserialize_effective_config(
        config: &Config,
        config_layer_stack: &ConfigLayerStack,
    ) -> anyhow::Result<crate::config::ConfigToml> {
        Ok(deserialize_config_toml_with_base(
            config_layer_stack.effective_config(),
            &config.codex_home,
        )?)
    }

    fn existing_layers(config: &Config) -> Vec<ConfigLayerEntry> {
        config
            .config_layer_stack
            .get_layers(
                ConfigLayerStackOrdering::LowestPrecedenceFirst,
                /*include_disabled*/ true,
            )
            .into_iter()
            .cloned()
            .collect()
    }

    fn insert_layer(layers: &mut Vec<ConfigLayerEntry>, layer: ConfigLayerEntry) {
        let insertion_index =
            layers.partition_point(|existing_layer| existing_layer.name <= layer.name);
        layers.insert(insertion_index, layer);
    }

    fn role_layer(role_layer_toml: TomlValue) -> ConfigLayerEntry {
        ConfigLayerEntry::new(ConfigLayerSource::SessionFlags, role_layer_toml)
    }

    fn reload_overrides(config: &Config, preserve_current_provider: bool) -> ConfigOverrides {
        ConfigOverrides {
            cwd: Some(config.cwd.to_path_buf()),
            model_provider: preserve_current_provider.then(|| config.model_provider_id.clone()),
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            main_execve_wrapper_exe: config.main_execve_wrapper_exe.clone(),
            js_repl_node_path: config.js_repl_node_path.clone(),
            ..Default::default()
        }
    }
}

pub(crate) mod spawn_tool_spec {
    use super::*;

    /// Builds the spawn-agent tool description text from built-in and configured roles.
    pub(crate) fn build(user_defined_agent_roles: &BTreeMap<String, AgentRoleConfig>) -> String {
        let built_in_roles = built_in::configs();
        build_from_configs(built_in_roles, user_defined_agent_roles)
    }

    // This function is not inlined for testing purpose.
    fn build_from_configs(
        built_in_roles: &BTreeMap<String, AgentRoleConfig>,
        user_defined_roles: &BTreeMap<String, AgentRoleConfig>,
    ) -> String {
        let mut seen = BTreeSet::new();
        let mut formatted_roles = Vec::new();
        for (name, declaration) in user_defined_roles {
            if seen.insert(name.as_str()) {
                formatted_roles.push(format_role(name, declaration));
            }
        }
        for (name, declaration) in built_in_roles {
            if seen.insert(name.as_str()) {
                formatted_roles.push(format_role(name, declaration));
            }
        }

        format!(
            "Optional type name for the new agent. If omitted, `{DEFAULT_ROLE_NAME}` is used.\nAvailable roles:\n{}",
            formatted_roles.join("\n"),
        )
    }

    fn format_role(name: &str, declaration: &AgentRoleConfig) -> String {
        let tags = declaration
            .tags
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        let tags_line = (!tags.is_empty()).then(|| format!("Tags: {}", tags.join(", ")));
        let locked_settings_note = declaration
            .config_file
            .as_ref()
            .and_then(|config_file| {
                built_in::config_file_contents(config_file)
                    .map(str::to_owned)
                    .or_else(|| std::fs::read_to_string(config_file).ok())
            })
            .and_then(|contents| toml::from_str::<TomlValue>(&contents).ok())
            .map(|role_toml| {
                let model = role_toml.get("model").and_then(TomlValue::as_str);
                let reasoning_effort = role_toml
                    .get("model_reasoning_effort")
                    .and_then(TomlValue::as_str);

                match (model, reasoning_effort) {
                    (Some(model), Some(reasoning_effort)) => format!(
                        "- This role's model is set to `{model}` and its reasoning effort is set to `{reasoning_effort}`. These settings cannot be changed."
                    ),
                    (Some(model), None) => {
                        format!("- This role's model is set to `{model}` and cannot be changed.")
                    }
                    (None, Some(reasoning_effort)) => {
                        format!(
                            "- This role's reasoning effort is set to `{reasoning_effort}` and cannot be changed."
                        )
                    }
                    (None, None) => String::new(),
                }
            })
            .filter(|note| !note.is_empty());

        let mut lines = Vec::new();
        if let Some(tags_line) = tags_line {
            lines.push(tags_line);
        }
        if let Some(description) = &declaration.description {
            lines.push(description.clone());
        } else if !lines.is_empty() {
            lines.push("no description".to_string());
        }
        if let Some(locked_settings_note) = locked_settings_note {
            lines.push(locked_settings_note);
        }

        if lines.is_empty() {
            format!("{name}: no description")
        } else {
            format!("{name}: {{\n{}\n}}", lines.join("\n"))
        }
    }
}

mod built_in {
    use super::*;

    /// Returns the cached built-in role declarations defined in this module.
    pub(super) fn configs() -> &'static BTreeMap<String, AgentRoleConfig> {
        static CONFIG: LazyLock<BTreeMap<String, AgentRoleConfig>> = LazyLock::new(|| {
            BTreeMap::from([
                (
                    DEFAULT_ROLE_NAME.to_string(),
                    AgentRoleConfig {
                        description: Some(
                            "Default agent.\nUses `model_sub` as the default child model when configured."
                                .to_string(),
                        ),
                        config_file: None,
                        tags: Vec::new(),
                        nickname_candidates: None,
                    }
                ),
                (
                    "explorer".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Use `explorer` for specific codebase questions.
Explorers are fast and authoritative.
They must be used to ask specific, well-scoped questions on the codebase.
Rules:
- In order to avoid redundant work, you should avoid exploring the same problem that explorers have already covered. Typically, you should trust the explorer results without additional verification. You are still allowed to inspect the code yourself to gain the needed context!
- You are encouraged to spawn up multiple explorers in parallel when you have multiple distinct questions to ask about the codebase that can be answered independently. This allows you to get more information faster without waiting for one question to finish before asking the next. While waiting for the explorer results, you can continue working on other local tasks that do not depend on those results. This parallelism is a key advantage of delegation, so use it whenever you have multiple questions to ask.
- Reuse existing explorers for related questions.
- Inherits `model_sub` when configured unless this spawn sets an explicit `model` override."#
                            .to_string()),
                        config_file: Some("explorer.toml".to_string().parse().unwrap_or_default()),
                        tags: vec!["fast".to_string(), "tool_intensive".to_string()],
                        nickname_candidates: None,
                    }
                ),
                (
                    "claude-opus".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Claude Opus 4.6 (1M context) for deep reasoning tasks.
Typical tasks:
- Complex cross-file refactoring and architecture redesign
- Root cause analysis of hard-to-reproduce bugs
- Security audits requiring understanding of full call chains
Rules:
- Prefer this role when reasoning depth matters more than latency.
- Provide concrete file/module scope so the agent can focus quickly."#.to_string()),
                        config_file: Some("claude-opus.toml".to_string().parse().unwrap_or_default()),
                        tags: vec!["large_context".to_string(), "deep_reasoning".to_string()],
                        nickname_candidates: None,
                    }
                ),
                (
                    "claude-sonnet".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Claude Sonnet 4.6 (1M context) for fast execution.
Typical tasks:
- Code exploration and targeted Q&A
- Test writing and fixture updates
- Straightforward bug fixes and docs updates
Rules:
- Prefer this role for speed-sensitive tasks with clear scope.
- Split larger work into parallel sub-tasks when possible."#.to_string()),
                        config_file: Some("claude-sonnet.toml".to_string().parse().unwrap_or_default()),
                        tags: vec!["large_context".to_string(), "fast".to_string()],
                        nickname_candidates: None,
                    }
                ),
                (
                    "claude-haiku".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Claude Haiku 4.5 (200K context) for fastest responses.
Typical tasks:
- Quick code reviews and simple refactoring
- Fast iteration on small changes
- Lightweight exploration and validation
Rules:
- Prefer this role for speed-critical tasks with limited scope.
- Best for tasks that don't require deep reasoning or large context."#.to_string()),
                        config_file: Some("claude-haiku.toml".to_string().parse().unwrap_or_default()),
                        tags: vec!["fast".to_string(), "lightweight".to_string()],
                        nickname_candidates: None,
                    }
                ),
                (
                    "worker".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Use for execution and production work.
Typical tasks:
- Implement part of a feature
- Fix tests or bugs
- Split large refactors into independent chunks
Rules:
- Explicitly assign **ownership** of the task (files / responsibility). When the subtask involves code changes, you should clearly specify which files or modules the worker is responsible for. This helps avoid merge conflicts and ensures accountability. For example, you can say "Worker 1 is responsible for updating the authentication module, while Worker 2 will handle the database layer." By defining clear ownership, you can delegate more effectively and reduce coordination overhead.
- Always tell workers they are **not alone in the codebase**, and they should not revert the edits made by others, and they should adjust their implementation to accommodate the changes made by others. This is important because there may be multiple workers making changes in parallel, and they need to be aware of each other's work to avoid conflicts and ensure a cohesive final product."#.to_string()),
                        config_file: None,
                        tags: vec!["execution".to_string(), "ownership".to_string()],
                        nickname_candidates: None,
                    }
                ),
                (
                    "awaiter".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Use an `awaiter` agent EVERY TIME you must run a command that might take some very long time.
This includes, but not only:
* testing
* monitoring of a long running process
* explicit ask to wait for something

When YOU wait for the `awaiter` agent to be done, use the largest possible timeout.
Be patient with the `awaiter`.
Close the awaiter when you're done with it."#.to_string()),
                        config_file: Some("awaiter.toml".to_string().parse().unwrap_or_default()),
                        tags: vec!["async".to_string(), "long_running".to_string()],
                        nickname_candidates: None,
                    }
                ),
            ])
        });
        &CONFIG
    }

    /// Resolves a built-in role `config_file` path to embedded content.
    pub(super) fn config_file_contents(path: &Path) -> Option<&'static str> {
        const EXPLORER: &str = include_str!("builtins/explorer.toml");
        const AWAITER: &str = include_str!("builtins/awaiter.toml");
        match path.to_str()? {
            "explorer.toml" => Some(EXPLORER),
            "awaiter.toml" => Some(AWAITER),
            "claude-opus.toml" => Some(BUILT_IN_CLAUDE_OPUS_CONFIG),
            "claude-sonnet.toml" => Some(BUILT_IN_CLAUDE_SONNET_CONFIG),
            "claude-haiku.toml" => Some(BUILT_IN_CLAUDE_HAIKU_CONFIG),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "role_tests.rs"]
mod tests;
