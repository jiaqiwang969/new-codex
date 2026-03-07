use git_graph::config::get_model_name;
use git_graph::get_repo;
use git_graph::graph::GitGraph;
use git_graph::print::format::CommitFormat;
use git_graph::print::unicode::print_unicode;
use git_graph::settings::BranchOrder;
use git_graph::settings::BranchSettings;
use git_graph::settings::BranchSettingsDef;
use git_graph::settings::Characters;
use git_graph::settings::MergePatterns;
use git_graph::settings::Settings;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

const REPO_CONFIG_FILE: &str = "git-graph.toml";

pub(crate) async fn get_git_graph(repo_path: PathBuf) -> io::Result<(bool, String)> {
    if !inside_git_repo(&repo_path).await? {
        return Ok((false, String::new()));
    }

    let library_repo_path = repo_path.clone();
    match tokio::task::spawn_blocking(move || generate_with_git_graph(&library_repo_path)).await {
        Ok(Ok(graph_text)) => Ok((true, graph_text)),
        Ok(Err(_)) => run_git_log_graph(&repo_path)
            .await
            .map(|graph_text| (true, graph_text)),
        Err(err) => Err(io::Error::other(format!("git graph task failed: {err}"))),
    }
}

async fn inside_git_repo(repo_path: &Path) -> io::Result<bool> {
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repo_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match status {
        Ok(status) if status.success() => Ok(true),
        Ok(_) => Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

async fn run_git_log_graph(repo_path: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args([
            "log",
            "--graph",
            "--pretty=format:%C(auto)\x1b[2m%h\x1b[22m %s %C(green)(%cr) \x1b[2m<%an>\x1b[22m%C(auto)%d",
            "--all",
            "--color=always",
            "--abbrev-commit",
        ])
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if output.status.success() {
        let output = String::from_utf8_lossy(&output.stdout);
        if output.trim().is_empty() {
            Ok("No git history found.".to_string())
        } else {
            Ok(output
                .lines()
                .map(convert_to_round_style)
                .collect::<Vec<_>>()
                .join("\n"))
        }
    } else {
        let fallback_output = Command::new("git")
            .args(["log", "--graph", "--oneline", "--all", "--color=always"])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !fallback_output.status.success() {
            return Err(io::Error::other(format!(
                "git log failed: {}",
                String::from_utf8_lossy(&fallback_output.stderr)
            )));
        }

        let output = String::from_utf8_lossy(&fallback_output.stdout);
        if output.trim().is_empty() {
            Ok("No git history found.".to_string())
        } else {
            Ok(output
                .lines()
                .map(convert_to_round_style)
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

fn convert_to_round_style(line: &str) -> String {
    let mut result = line.replace('*', "●");
    result = result.replace('|', "│");
    result = result.replace('\\', "╲");
    result = result.replace('/', "╱");
    result.replace('-', "─")
}

fn generate_with_git_graph(repo_path: &Path) -> Result<String, String> {
    let repo = get_repo(repo_path, true).map_err(|e| format!("libgit2 error: {}", e.message()))?;

    let model_name = get_model_name(&repo, REPO_CONFIG_FILE).unwrap_or(None);
    let model_def = match model_name.as_deref() {
        Some("git-flow") => BranchSettingsDef::git_flow(),
        Some("simple") => BranchSettingsDef::simple(),
        Some("none") => BranchSettingsDef::none(),
        _ => BranchSettingsDef::simple(),
    };
    let branches = BranchSettings::from(model_def).map_err(|e| format!("settings error: {e}"))?;

    let settings = Settings {
        reverse_commit_order: false,
        debug: false,
        compact: true,
        colored: true,
        include_remote: true,
        format: CommitFormat::Format("%h%d %s (%ar) <%an>".to_string()),
        wrapping: None,
        characters: Characters::round(),
        branch_order: BranchOrder::ShortestFirst(true),
        branches,
        merge_patterns: MergePatterns::default(),
    };

    let graph = GitGraph::new(repo, &settings, None)?;
    let (graph_lines, text_lines, _indices) = print_unicode(&graph, &settings)?;
    let lines = graph_lines
        .into_iter()
        .zip(text_lines)
        .map(|(graph_line, text_line)| format!(" {graph_line}  {text_line}"))
        .collect::<Vec<_>>();

    if lines.is_empty() {
        Ok("No git history found.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}
