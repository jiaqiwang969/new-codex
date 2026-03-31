//! Test-only helpers exposed for cross-crate integration tests.
//!
//! Production code should not depend on this module.
//! We prefer this to using a crate feature to avoid building multiple
//! permutations of the crate.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use codex_exec_server::EnvironmentManager;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;
use once_cell::sync::Lazy;

use crate::AuthManager;
use crate::CodexAuth;
use crate::ModelProviderInfo;
use crate::ThreadManager;
use crate::approval_runtime::ApprovalRuntimeClient;
use crate::approval_runtime::InMemoryApprovalRuntimeClient;
use crate::approval_runtime::RuntimeFinishObservation;
use crate::approval_runtime::RuntimeFinishRequest;
use crate::approval_runtime::RuntimeHealth;
use crate::approval_runtime::RuntimeLease;
use crate::approval_runtime::RuntimeLeaseRegistration;
use crate::approval_runtime::RuntimePreflight;
use crate::approval_runtime::RuntimePreflightRequest;
use crate::approval_runtime::SharedApprovalRuntime;
use crate::config::Config;
use crate::models_manager::collaboration_mode_presets;
use crate::models_manager::manager::ModelsManager;
use crate::thread_manager;
use crate::unified_exec;

static TEST_MODEL_PRESETS: Lazy<Vec<ModelPreset>> = Lazy::new(|| {
    let file_contents = include_str!("../models.json");
    let mut response: ModelsResponse = serde_json::from_str(file_contents)
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));
    response.models.sort_by(|a, b| a.priority.cmp(&b.priority));
    let mut presets: Vec<ModelPreset> = response.models.into_iter().map(Into::into).collect();
    ModelPreset::mark_default_by_picker_visibility(&mut presets);
    presets
});

pub fn set_thread_manager_test_mode(enabled: bool) {
    thread_manager::set_thread_manager_test_mode_for_tests(enabled);
}

pub fn set_deterministic_process_ids(enabled: bool) {
    unified_exec::set_deterministic_process_ids_for_tests(enabled);
}

pub fn auth_manager_from_auth(auth: CodexAuth) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing(auth)
}

pub fn auth_manager_from_auth_with_home(auth: CodexAuth, codex_home: PathBuf) -> Arc<AuthManager> {
    AuthManager::from_auth_for_testing_with_home(auth, codex_home)
}

pub fn thread_manager_with_models_provider(
    auth: CodexAuth,
    provider: ModelProviderInfo,
) -> ThreadManager {
    ThreadManager::with_models_provider_for_tests(auth, provider)
}

pub fn thread_manager_with_models_provider_and_home(
    auth: CodexAuth,
    provider: ModelProviderInfo,
    codex_home: PathBuf,
    environment_manager: Arc<EnvironmentManager>,
) -> ThreadManager {
    ThreadManager::with_models_provider_and_home_for_tests(
        auth,
        provider,
        codex_home,
        environment_manager,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestApprovalRuntimePreflightRecord {
    pub lease_id: String,
    pub destructive: bool,
    pub permit_summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestApprovalRuntimeFinishRecord {
    pub lease_id: String,
    pub action_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestApprovalRuntimePreflight {
    Healthy,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
}

impl TestApprovalRuntimePreflight {
    pub fn healthy() -> Self {
        Self::Healthy
    }

    pub fn fallback_to_human(summary: impl Into<String>) -> Self {
        Self::FallbackToHuman {
            summary: summary.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestApprovalRuntimeFinish {
    Clean,
    Recovery { summary: String },
    FallbackToHuman { summary: String },
    Mismatch { summary: String },
    PolicyDrift { summary: String },
}

impl TestApprovalRuntimeFinish {
    pub fn clean() -> Self {
        Self::Clean
    }

    pub fn policy_drift(summary: impl Into<String>) -> Self {
        Self::PolicyDrift {
            summary: summary.into(),
        }
    }
}

#[derive(Default)]
struct ScriptedApprovalRuntimeState {
    next_action_id: usize,
    preflight_responses: std::collections::VecDeque<TestApprovalRuntimePreflight>,
    finish_responses: std::collections::VecDeque<TestApprovalRuntimeFinish>,
    preflight_requests: Vec<TestApprovalRuntimePreflightRecord>,
    finish_requests: Vec<TestApprovalRuntimeFinishRecord>,
}

impl ScriptedApprovalRuntimeState {
    fn next_action_id(&mut self) -> String {
        self.next_action_id += 1;
        format!("test-action-{}", self.next_action_id)
    }
}

#[derive(Default)]
struct ScriptedApprovalRuntime {
    lease_runtime: Arc<InMemoryApprovalRuntimeClient>,
    state: Mutex<ScriptedApprovalRuntimeState>,
}

#[async_trait]
impl ApprovalRuntimeClient for ScriptedApprovalRuntime {
    async fn register_lease(
        &self,
        request: RuntimeLeaseRegistration,
    ) -> anyhow::Result<RuntimeLease> {
        self.lease_runtime.register_lease(request).await
    }

    async fn derive_child_lease(
        &self,
        request: crate::approval_runtime::RuntimeChildLeaseRequest,
    ) -> anyhow::Result<RuntimeLease> {
        self.lease_runtime.derive_child_lease(request).await
    }

    async fn revoke_lease(&self, lease_id: &str) -> anyhow::Result<()> {
        self.lease_runtime.revoke_lease(lease_id).await
    }

    async fn preflight(
        &self,
        request: &RuntimePreflightRequest,
    ) -> anyhow::Result<RuntimePreflight> {
        let scripted = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .preflight_requests
                .push(TestApprovalRuntimePreflightRecord {
                    lease_id: request.lease_id.clone(),
                    destructive: request.destructive,
                    permit_summary: request.permit_summary.clone(),
                });
            match state.preflight_responses.pop_front() {
                Some(TestApprovalRuntimePreflight::Healthy) => Some(RuntimePreflight {
                    health: RuntimeHealth::Healthy,
                    action_id: Some(state.next_action_id()),
                }),
                Some(TestApprovalRuntimePreflight::Recovery { summary }) => {
                    Some(RuntimePreflight {
                        health: RuntimeHealth::Recovery { summary },
                        action_id: Some(state.next_action_id()),
                    })
                }
                Some(TestApprovalRuntimePreflight::FallbackToHuman { summary }) => {
                    Some(RuntimePreflight {
                        health: RuntimeHealth::FallbackToHuman { summary },
                        action_id: None,
                    })
                }
                None => None,
            }
        };
        if let Some(preflight) = scripted {
            Ok(preflight)
        } else {
            self.lease_runtime.preflight(request).await
        }
    }

    async fn finish(
        &self,
        request: &RuntimeFinishRequest,
    ) -> anyhow::Result<RuntimeFinishObservation> {
        let scripted = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.finish_requests.push(TestApprovalRuntimeFinishRecord {
                lease_id: request.lease_id.clone(),
                action_id: request.action_id.clone(),
            });
            state.finish_responses.pop_front()
        };
        Ok(match scripted {
            Some(TestApprovalRuntimeFinish::Clean) => RuntimeFinishObservation::Clean,
            Some(TestApprovalRuntimeFinish::Recovery { summary }) => {
                RuntimeFinishObservation::Recovery { summary }
            }
            Some(TestApprovalRuntimeFinish::FallbackToHuman { summary }) => {
                RuntimeFinishObservation::FallbackToHuman { summary }
            }
            Some(TestApprovalRuntimeFinish::Mismatch { summary }) => {
                RuntimeFinishObservation::Mismatch { summary }
            }
            Some(TestApprovalRuntimeFinish::PolicyDrift { summary }) => {
                RuntimeFinishObservation::PolicyDrift { summary }
            }
            None => self.lease_runtime.finish(request).await?,
        })
    }
}

#[derive(Clone, Default)]
pub struct TestApprovalRuntime {
    inner: Arc<ScriptedApprovalRuntime>,
}

impl TestApprovalRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_preflight(self, responses: Vec<TestApprovalRuntimePreflight>) -> Self {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.preflight_responses = responses.into();
        drop(state);
        self
    }

    pub fn with_finish(self, responses: Vec<TestApprovalRuntimeFinish>) -> Self {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.finish_responses = responses.into();
        drop(state);
        self
    }

    pub async fn preflight_requests(&self) -> Vec<TestApprovalRuntimePreflightRecord> {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.preflight_requests.clone()
    }

    pub async fn finish_requests(&self) -> Vec<TestApprovalRuntimeFinishRecord> {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.finish_requests.clone()
    }

    fn shared_runtime(&self) -> SharedApprovalRuntime {
        Arc::clone(&self.inner) as SharedApprovalRuntime
    }
}

pub async fn start_thread_with_test_overrides(
    thread_manager: &ThreadManager,
    config: Config,
    user_shell_override: Option<crate::shell::Shell>,
    approval_runtime: Option<TestApprovalRuntime>,
) -> crate::error::Result<crate::NewThread> {
    thread_manager
        .start_thread_with_test_overrides_for_tests(
            config,
            user_shell_override,
            approval_runtime.map(|runtime| runtime.shared_runtime()),
        )
        .await
}

pub async fn resume_thread_from_rollout_with_test_overrides(
    thread_manager: &ThreadManager,
    config: Config,
    rollout_path: PathBuf,
    auth_manager: Arc<AuthManager>,
    user_shell_override: Option<crate::shell::Shell>,
    approval_runtime: Option<TestApprovalRuntime>,
) -> crate::error::Result<crate::NewThread> {
    thread_manager
        .resume_thread_from_rollout_with_test_overrides_for_tests(
            config,
            rollout_path,
            auth_manager,
            user_shell_override,
            approval_runtime.map(|runtime| runtime.shared_runtime()),
        )
        .await
}

pub fn models_manager_with_provider(
    codex_home: PathBuf,
    auth_manager: Arc<AuthManager>,
    provider: ModelProviderInfo,
) -> ModelsManager {
    ModelsManager::with_provider_for_tests(codex_home, auth_manager, provider)
}

pub fn get_model_offline(model: Option<&str>) -> String {
    ModelsManager::get_model_offline_for_tests(model)
}

pub fn construct_model_info_offline(model: &str, config: &Config) -> ModelInfo {
    ModelsManager::construct_model_info_offline_for_tests(model, config)
}

pub fn all_model_presets() -> &'static Vec<ModelPreset> {
    &TEST_MODEL_PRESETS
}

pub fn builtin_collaboration_mode_presets() -> Vec<CollaborationModeMask> {
    collaboration_mode_presets::builtin_collaboration_mode_presets(
        collaboration_mode_presets::CollaborationModesConfig::default(),
    )
}
