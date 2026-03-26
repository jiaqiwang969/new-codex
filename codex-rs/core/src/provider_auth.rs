use crate::auth::CodexAuth;
use crate::model_provider_info::ModelProviderInfo;

pub(crate) fn resolve_provider_api_key(
    provider: &ModelProviderInfo,
    auth: Option<&CodexAuth>,
) -> Option<String> {
    let env_key = provider.env_key.as_deref()?;
    std::env::var(env_key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            auth.and_then(|auth| auth.api_key_for_env_key(env_key))
                .map(str::to_string)
        })
        .or_else(|| auth.and_then(CodexAuth::api_key).map(str::to_string))
}
