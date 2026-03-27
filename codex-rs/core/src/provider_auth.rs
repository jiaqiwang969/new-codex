use std::path::Path;

use crate::auth::AuthCredentialsStoreMode;
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

pub(crate) fn resolve_gemini_api_key(
    provider: &ModelProviderInfo,
    auth: Option<&CodexAuth>,
) -> Option<String> {
    if let Some(api_key) = resolve_provider_api_key(provider, auth) {
        return Some(api_key);
    }
    if provider.env_key.is_some() {
        return None;
    }
    crate::auth::auth::read_gemini_api_key_from_env().or_else(|| {
        codex_utils_home_dir::find_codex_home()
            .ok()
            .and_then(|codex_home| {
                resolve_gemini_api_key_with_auth_json_fallback(provider, auth, &codex_home)
            })
    })
}

fn resolve_gemini_api_key_with_codex_home(codex_home: &Path) -> Option<String> {
    crate::auth::auth::read_gemini_api_key_from_auth_json(
        codex_home,
        AuthCredentialsStoreMode::File,
    )
}

fn resolve_gemini_api_key_with_auth_json_fallback(
    provider: &ModelProviderInfo,
    auth: Option<&CodexAuth>,
    codex_home: &Path,
) -> Option<String> {
    if let Some(api_key) = resolve_provider_api_key(provider, auth) {
        return Some(api_key);
    }
    if provider.env_key.is_some() {
        return None;
    }
    resolve_gemini_api_key_with_codex_home(codex_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn resolve_gemini_api_key_uses_auth_json_when_provider_env_key_is_missing() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("auth.json"),
            r#"{
                "OPENAI_API_KEY":"sk-openai",
                "GEMINI_API_KEY":"gemini-key"
            }"#,
        )
        .expect("write auth.json");

        let mut provider = ModelProviderInfo::create_gemini_provider();
        provider.env_key = None;

        assert_eq!(
            resolve_gemini_api_key_with_auth_json_fallback(&provider, None, dir.path()),
            Some("gemini-key".to_string())
        );
    }
}
