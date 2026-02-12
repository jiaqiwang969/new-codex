use crate::memories::memory_root_for_cwd;
use crate::memories::memory_root_for_user;
use crate::memories::memory_summary_file;
use crate::truncate::TruncationPolicy;
use crate::truncate::truncate_text;
use askama::Template;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs;
use tracing::warn;

#[derive(Template)]
#[template(path = "memories/consolidation.md", escape = "none")]
struct ConsolidationPromptTemplate<'a> {
    memory_root: &'a str,
}

#[derive(Template)]
#[template(path = "memories/stage_one_input.md", escape = "none")]
struct StageOneInputTemplate<'a> {
    rollout_path: &'a str,
    rollout_cwd: &'a str,
    rollout_contents: &'a str,
}

#[derive(Template)]
#[template(path = "memories/read_path.md", escape = "none")]
struct MemoryToolDeveloperInstructionsTemplate<'a> {
    scope_kind: &'a str,
    scope_version: &'a str,
    summary_sha256: &'a str,
    binding_key: &'a str,
    base_path: &'a str,
    memory_summary: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryReadPathSource {
    pub(crate) scope_kind: &'static str,
    pub(crate) memory_root: PathBuf,
    pub(crate) memory_summary_path: PathBuf,
    pub(crate) memory_summary: String,
    pub(crate) memory_summary_sha256: String,
    pub(crate) memory_scope_version: String,
    pub(crate) memory_binding_key: String,
}

/// Builds the consolidation subagent prompt for a specific memory root.
///
/// Falls back to a simple string replacement if Askama rendering fails.
pub(crate) fn build_consolidation_prompt(memory_root: &Path) -> String {
    let memory_root = memory_root.display().to_string();
    let template = ConsolidationPromptTemplate {
        memory_root: &memory_root,
    };
    match template.render() {
        Ok(prompt) => prompt,
        Err(err) => {
            warn!("failed to render memories consolidation prompt template: {err}");
            include_str!("../../templates/memories/consolidation.md")
                .replace("{{ memory_root }}", &memory_root)
        }
    }
}

/// Builds the stage-1 user message containing rollout metadata and content.
///
/// Large rollout payloads are truncated to a bounded byte budget while keeping
/// both head and tail context.
pub(crate) fn build_stage_one_input_message(
    rollout_path: &Path,
    rollout_cwd: &Path,
    rollout_contents: &str,
) -> String {
    let truncated_rollout_contents =
        truncate_text(rollout_contents, TruncationPolicy::Tokens(150_000));
    if truncated_rollout_contents != rollout_contents {
        warn!(
            "truncated rollout {} for stage-1 memory prompt with standard truncation policy",
            rollout_path.display(),
        );
    }

    let rollout_path = rollout_path.display().to_string();
    let rollout_cwd = rollout_cwd.display().to_string();
    let template = StageOneInputTemplate {
        rollout_path: &rollout_path,
        rollout_cwd: &rollout_cwd,
        rollout_contents: &truncated_rollout_contents,
    };
    match template.render() {
        Ok(prompt) => prompt,
        Err(err) => {
            warn!("failed to render memories stage-one input template: {err}");
            include_str!("../../templates/memories/stage_one_input.md")
                .replace("{{ rollout_path }}", &rollout_path)
                .replace("{{ rollout_cwd }}", &rollout_cwd)
                .replace("{{ rollout_contents }}", &truncated_rollout_contents)
        }
    }
}

#[cfg(test)]
pub(crate) async fn build_memory_tool_developer_instructions(
    codex_home: &Path,
    cwd: &Path,
) -> Option<String> {
    select_memory_read_path_source(codex_home, cwd)
        .await
        .map(|source| render_memory_tool_developer_instructions(&source))
}

pub(crate) fn render_memory_tool_developer_instructions(source: &MemoryReadPathSource) -> String {
    let scope_kind = source.scope_kind;
    let scope_version = source.memory_scope_version.as_str();
    let summary_sha256 = source.memory_summary_sha256.as_str();
    let binding_key = source.memory_binding_key.as_str();
    let base_path = source.memory_root.display().to_string();
    let template = MemoryToolDeveloperInstructionsTemplate {
        scope_kind,
        scope_version,
        summary_sha256,
        binding_key,
        base_path: &base_path,
        memory_summary: &source.memory_summary,
    };
    match template.render() {
        Ok(prompt) => prompt,
        Err(err) => {
            warn!("failed to render memories read-path prompt template: {err}");
            include_str!("../../templates/memories/read_path.md")
                .replace("{{ scope_kind }}", scope_kind)
                .replace("{{ scope_version }}", scope_version)
                .replace("{{ summary_sha256 }}", summary_sha256)
                .replace("{{ binding_key }}", binding_key)
                .replace("{{ base_path }}", &base_path)
                .replace("{{ memory_summary }}", &source.memory_summary)
        }
    }
}

pub(crate) async fn select_memory_read_path_source(
    codex_home: &Path,
    cwd: &Path,
) -> Option<MemoryReadPathSource> {
    let candidate_sources = [
        (
            crate::memories::MEMORY_SCOPE_KIND_CWD,
            memory_root_for_cwd(codex_home, cwd),
        ),
        (
            crate::memories::MEMORY_SCOPE_KIND_USER,
            memory_root_for_user(codex_home),
        ),
    ];

    for (scope_kind, memory_root) in candidate_sources {
        let memory_summary_path = memory_summary_file(&memory_root);
        let Ok(memory_summary) = fs::read_to_string(&memory_summary_path).await else {
            continue;
        };
        let memory_summary = memory_summary.trim().to_string();
        if memory_summary.is_empty() {
            continue;
        }
        let memory_summary_sha256 = crate::memories::memory_summary_sha256(&memory_summary);
        let memory_scope_version =
            crate::memories::memory_scope_version(scope_kind, &memory_summary_sha256);
        let memory_binding_key =
            crate::memories::memory_binding_key(&memory_scope_version, &memory_summary_sha256);
        return Some(MemoryReadPathSource {
            scope_kind,
            memory_root,
            memory_summary_path,
            memory_summary,
            memory_summary_sha256,
            memory_scope_version,
            memory_binding_key,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn build_stage_one_input_message_truncates_rollout_with_standard_policy() {
        let input = format!("{}{}{}", "a".repeat(700_000), "middle", "z".repeat(700_000));
        let expected_truncated = truncate_text(&input, TruncationPolicy::Tokens(150_000));
        let prompt = build_stage_one_input_message(
            Path::new("/tmp/rollout.jsonl"),
            Path::new("/tmp"),
            &input,
        );

        assert!(expected_truncated.contains("tokens truncated"));
        assert!(expected_truncated.starts_with('a'));
        assert!(expected_truncated.ends_with('z'));
        assert!(prompt.contains(&expected_truncated));
    }

    #[tokio::test]
    async fn build_memory_tool_developer_instructions_prefers_cwd_scope_then_user_scope() {
        let tempdir = tempdir().expect("tempdir");
        let codex_home = tempdir.path();
        let cwd = codex_home.join("workspace");
        let cwd_root = memory_root_for_cwd(codex_home, cwd.as_path());
        tokio::fs::create_dir_all(&cwd_root)
            .await
            .expect("create cwd memory root");
        tokio::fs::write(memory_summary_file(&cwd_root), "cwd summary")
            .await
            .expect("write cwd summary");

        let user_root = memory_root_for_user(codex_home);
        tokio::fs::create_dir_all(&user_root)
            .await
            .expect("create user memory root");
        tokio::fs::write(memory_summary_file(&user_root), "user summary")
            .await
            .expect("write user summary");

        let source = select_memory_read_path_source(codex_home, cwd.as_path())
            .await
            .expect("memory source");
        assert_eq!(source.scope_kind, crate::memories::MEMORY_SCOPE_KIND_CWD);
        assert_eq!(source.memory_root, cwd_root);
        assert_eq!(
            source.memory_summary_sha256,
            crate::memories::memory_summary_sha256("cwd summary")
        );
        assert_eq!(
            source.memory_scope_version,
            crate::memories::memory_scope_version(
                crate::memories::MEMORY_SCOPE_KIND_CWD,
                &source.memory_summary_sha256,
            )
        );
        assert_eq!(
            source.memory_binding_key,
            crate::memories::memory_binding_key(
                &source.memory_scope_version,
                &source.memory_summary_sha256,
            )
        );

        let instructions = build_memory_tool_developer_instructions(codex_home, cwd.as_path())
            .await
            .expect("memory instructions");

        assert!(instructions.contains("cwd summary"));
        assert!(instructions.contains("Active memory scope: cwd"));
        assert!(instructions.contains("Active memory scope version: cwd:"));
        assert!(instructions.contains("Active memory summary sha256: "));
        assert!(instructions.contains("Active memory binding key: cwd:"));
        assert!(instructions.contains(cwd_root.display().to_string().as_str()));
        assert!(!instructions.contains("user summary"));
    }

    #[tokio::test]
    async fn select_memory_read_path_source_falls_back_to_user_scope() {
        let tempdir = tempdir().expect("tempdir");
        let codex_home = tempdir.path();
        let cwd = codex_home.join("workspace");

        let user_root = memory_root_for_user(codex_home);
        tokio::fs::create_dir_all(&user_root)
            .await
            .expect("create user memory root");
        tokio::fs::write(memory_summary_file(&user_root), " user summary ")
            .await
            .expect("write user summary");

        let source = select_memory_read_path_source(codex_home, cwd.as_path())
            .await
            .expect("memory source");
        assert_eq!(source.scope_kind, crate::memories::MEMORY_SCOPE_KIND_USER);
        assert_eq!(source.memory_root, user_root);
        assert_eq!(source.memory_summary, "user summary");
        assert_eq!(
            source.memory_summary_sha256,
            crate::memories::memory_summary_sha256("user summary")
        );
        assert_eq!(
            source.memory_scope_version,
            crate::memories::memory_scope_version(
                crate::memories::MEMORY_SCOPE_KIND_USER,
                &source.memory_summary_sha256,
            )
        );
        assert_eq!(
            source.memory_binding_key,
            crate::memories::memory_binding_key(
                &source.memory_scope_version,
                &source.memory_summary_sha256,
            )
        );

        let instructions = render_memory_tool_developer_instructions(&source);
        assert!(instructions.contains("Active memory scope: user"));
        assert!(instructions.contains("Active memory scope version: user:"));
        assert!(instructions.contains("Active memory summary sha256: "));
        assert!(instructions.contains("Active memory binding key: user:"));
    }

    #[tokio::test]
    async fn select_memory_read_path_source_returns_none_when_summaries_missing_or_empty() {
        let tempdir = tempdir().expect("tempdir");
        let codex_home = tempdir.path();
        let cwd = codex_home.join("workspace");

        let cwd_root = memory_root_for_cwd(codex_home, cwd.as_path());
        tokio::fs::create_dir_all(&cwd_root)
            .await
            .expect("create cwd memory root");
        tokio::fs::write(memory_summary_file(&cwd_root), "   \n")
            .await
            .expect("write empty cwd summary");

        let user_root = memory_root_for_user(codex_home);
        tokio::fs::create_dir_all(&user_root)
            .await
            .expect("create user memory root");
        tokio::fs::write(memory_summary_file(&user_root), "\t")
            .await
            .expect("write empty user summary");

        let source = select_memory_read_path_source(codex_home, cwd.as_path()).await;
        assert_eq!(source, None);

        let instructions =
            build_memory_tool_developer_instructions(codex_home, cwd.as_path()).await;
        assert_eq!(instructions, None);
    }
}
