use crate::model_provider_info::ModelProviderAccount;
use crate::model_provider_info::ModelProviderInfo;
use crate::provider_routing::normalize_account_pool_in_config_order;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
struct ProviderPoolRuntimeState {
    cooldowns: HashMap<ModelProviderAccount, Instant>,
}

impl ProviderPoolRuntimeState {
    fn cooldown_until(&mut self, account: &ModelProviderAccount, now: Instant) -> Option<Instant> {
        match self.cooldowns.get(account).copied() {
            Some(until) if until > now => Some(until),
            Some(_) => {
                self.cooldowns.remove(account);
                None
            }
            None => None,
        }
    }

    fn mark_cooling(
        &mut self,
        account: ModelProviderAccount,
        now: Instant,
        cooldown: Duration,
    ) -> Instant {
        let until = now + cooldown;
        self.cooldowns.insert(account, until);
        until
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderPoolState {
    providers: HashMap<String, ProviderPoolRuntimeState>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedTurnProvider {
    pub(crate) provider: ModelProviderInfo,
    pub(crate) background_message: Option<String>,
}

impl ProviderPoolState {
    pub(crate) fn mark_account_cooling(
        &mut self,
        provider_id: &str,
        account: ModelProviderAccount,
        now: Instant,
        cooldown: Duration,
    ) -> Instant {
        self.providers
            .entry(provider_id.to_string())
            .or_default()
            .mark_cooling(account, now, cooldown)
    }

    pub(crate) fn resolve_turn_provider(
        &mut self,
        provider_id: &str,
        provider: &ModelProviderInfo,
        now: Instant,
    ) -> ResolvedTurnProvider {
        let pool = normalize_account_pool_in_config_order(provider_id, provider);
        if pool.is_empty() {
            return ResolvedTurnProvider {
                provider: provider.clone(),
                background_message: None,
            };
        }

        let mut cooled_indices = Vec::new();
        let runtime = self.providers.entry(provider_id.to_string()).or_default();
        for (index, account) in pool.iter().enumerate() {
            if runtime.cooldown_until(account, now).is_some() {
                cooled_indices.push(index);
                continue;
            }

            let background_message = if pool.len() == 1 {
                None
            } else if cooled_indices.is_empty() {
                Some(format!(
                    "Provider pool {provider_id}: trying key {}/{}",
                    index + 1,
                    pool.len()
                ))
            } else {
                let skipped_keys = if cooled_indices.len() == 1 {
                    format!("key {}/{}", cooled_indices[0] + 1, pool.len())
                } else {
                    let keys = cooled_indices
                        .iter()
                        .map(|skipped_index| format!("{}/{}", skipped_index + 1, pool.len()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("keys {keys}")
                };
                Some(format!(
                    "Provider pool {provider_id}: {skipped_keys} cooling down; trying key {}/{}",
                    index + 1,
                    pool.len()
                ))
            };

            return ResolvedTurnProvider {
                provider: provider.with_account(account),
                background_message,
            };
        }

        ResolvedTurnProvider {
            provider: provider.with_account(&pool[0]),
            background_message: Some(format!(
                "Provider pool {provider_id}: all keys cooling down; forcing fresh probe from key 1/{}",
                pool.len()
            )),
        }
    }
}

pub(crate) fn next_account_from_pool(
    provider_id: &str,
    provider: &ModelProviderInfo,
    current_account: Option<&ModelProviderAccount>,
    attempted_accounts: &mut HashSet<ModelProviderAccount>,
) -> Option<ModelProviderAccount> {
    let pool = normalize_account_pool_in_config_order(provider_id, provider);
    let pool_len = pool.len();
    if pool_len == 0 {
        return None;
    }

    let start_index = current_account
        .and_then(|account| pool.iter().position(|item| item == account))
        .map(|index| (index + 1) % pool_len)
        .unwrap_or(0);

    for offset in 0..pool_len {
        let index = (start_index + offset) % pool_len;
        let account = pool[index].clone();
        if attempted_accounts.insert(account.clone()) {
            return Some(account);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::ProviderPoolState;
    use super::ResolvedTurnProvider;
    use crate::model_provider_info::ModelProviderAccount;
    use crate::model_provider_info::ModelProviderInfo;
    use crate::model_provider_info::WireApi;
    use pretty_assertions::assert_eq;
    use std::time::Duration;
    use std::time::Instant;

    fn provider(account_pool: Vec<ModelProviderAccount>) -> ModelProviderInfo {
        ModelProviderInfo {
            name: "Anthropic".to_string(),
            base_url: Some("https://api.anthropic.com".to_string()),
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
            env_key_instructions: None,
            experimental_bearer_token: None,
            wire_api: WireApi::Anthropic,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            requires_openai_auth: false,
            supports_websockets: false,
            account_pool,
        }
    }

    #[test]
    fn resolve_turn_provider_skips_cooled_account_and_reports_skipped_key() {
        let provider_id = "anthropic";
        let now = Instant::now();
        let account_1 = ModelProviderAccount {
            base_url: Some("https://pool-primary.example".to_string()),
            env_key: Some("ANTHROPIC_API_KEY_PRIMARY".to_string()),
        };
        let account_2 = ModelProviderAccount {
            base_url: Some("https://pool-secondary.example".to_string()),
            env_key: Some("ANTHROPIC_API_KEY_SECONDARY".to_string()),
        };
        let logical_provider = provider(vec![account_1.clone(), account_2.clone()]);
        let mut state = ProviderPoolState::default();

        state.mark_account_cooling(provider_id, account_1, now, Duration::from_secs(60));

        assert_eq!(
            state.resolve_turn_provider(provider_id, &logical_provider, now),
            ResolvedTurnProvider {
                provider: logical_provider.with_account(&account_2),
                background_message: Some(
                    "Provider pool anthropic: key 1/2 cooling down; trying key 2/2".to_string()
                ),
            }
        );
    }

    #[test]
    fn resolve_turn_provider_forces_fresh_probe_when_all_keys_are_cooling() {
        let provider_id = "anthropic";
        let now = Instant::now();
        let account_1 = ModelProviderAccount {
            base_url: Some("https://pool-primary.example".to_string()),
            env_key: Some("ANTHROPIC_API_KEY_PRIMARY".to_string()),
        };
        let account_2 = ModelProviderAccount {
            base_url: Some("https://pool-secondary.example".to_string()),
            env_key: Some("ANTHROPIC_API_KEY_SECONDARY".to_string()),
        };
        let logical_provider = provider(vec![account_1.clone(), account_2.clone()]);
        let mut state = ProviderPoolState::default();

        state.mark_account_cooling(provider_id, account_1.clone(), now, Duration::from_secs(60));
        state.mark_account_cooling(provider_id, account_2, now, Duration::from_secs(60));

        assert_eq!(
            state.resolve_turn_provider(provider_id, &logical_provider, now),
            ResolvedTurnProvider {
                provider: logical_provider.with_account(&account_1),
                background_message: Some(
                    "Provider pool anthropic: all keys cooling down; forcing fresh probe from key 1/2"
                        .to_string()
                ),
            }
        );
    }
}
