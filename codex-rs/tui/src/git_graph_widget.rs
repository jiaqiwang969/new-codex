use crate::pager_overlay::Overlay;
use codex_ansi_escape::ansi_escape_line;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::Path;
use std::process::Command;

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

/// Convert ASCII art to round/Unicode style
fn convert_to_round_style(line: &str) -> String {
    let mut result = line.to_string();
    result = result.replace('*', "●");
    result = result.replace('|', "│");
    result = result.replace('\\', "╲");
    result = result.replace('/', "╱");
    result = result.replace('-', "─");
    result
}

/// Generate git graph lines for display in the TUI overlay.
pub fn generate_git_graph<P: AsRef<Path>>(repo_path: P) -> Result<Vec<Line<'static>>, String> {
    // First try: high-quality round Unicode graph via git-graph library.
    if let Ok(lines) = generate_with_git_graph(repo_path.as_ref()) {
        return Ok(lines);
    }

    // Fallback: use `git log --graph` (ASCII) and do a best-effort conversion
    // to Unicode line drawing characters.
    let output = Command::new("git")
        .args([
            "log",
            "--graph",
            "--pretty=format:%C(auto)\x1b[2m%h\x1b[22m %s %C(green)(%cr) \x1b[2m<%an>\x1b[22m%C(auto)%d",
            "--all",
            "--color=always",
            "--abbrev-commit",
        ])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git log: {e}"))?;

    if !output.status.success() {
        // Fallback to simpler git log if the above fails
        let fallback_output = Command::new("git")
            .args(["log", "--graph", "--oneline", "--all", "--color=always"])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to execute fallback git log: {e}"))?;

        if !fallback_output.status.success() {
            return Err(format!(
                "Git command failed: {}",
                String::from_utf8_lossy(&fallback_output.stderr)
            ));
        }

        let output_str = String::from_utf8_lossy(&fallback_output.stdout);
        return if output_str.trim().is_empty() {
            Ok(vec!["No git history found.".dim().into()])
        } else {
            let lines: Vec<Line<'static>> = output_str
                .lines()
                .map(|line| {
                    let round_line = convert_to_round_style(line);
                    ansi_escape_line(&round_line)
                })
                .collect();
            Ok(lines)
        };
    }

    let output_str = String::from_utf8_lossy(&output.stdout);

    if output_str.trim().is_empty() {
        Ok(vec!["No git history found.".dim().into()])
    } else {
        let lines: Vec<Line<'static>> = output_str
            .lines()
            .map(|line| {
                let round_line = convert_to_round_style(line);
                ansi_escape_line(&round_line)
            })
            .collect();
        Ok(lines)
    }
}

/// Create a new git graph overlay for the TUI with enhanced title.
pub fn create_git_graph_overlay<P: AsRef<Path>>(repo_path: P) -> Result<Overlay, String> {
    let path = repo_path.as_ref().to_path_buf();
    let lines = generate_git_graph(&path)?;

    // Create a refresh callback that regenerates the git graph
    let refresh_callback = Box::new(move || generate_git_graph(&path));

    Ok(Overlay::new_static_with_title_no_wrap_refresh(
        lines,
        "G I T   G R A P H   │   j/k:scroll   r:refresh   q/Esc:close   │   C t r l + G"
            .to_string(),
        refresh_callback,
    ))
}

// Build lines using the embedded git-graph library with a "round" style.
fn generate_with_git_graph<P: AsRef<Path>>(repo_path: P) -> Result<Vec<Line<'static>>, String> {
    let repo = get_repo(repo_path, true).map_err(|e| format!("libgit2 error: {}", e.message()))?;

    let model_name = get_model_name(&repo, "git-graph.toml").unwrap_or(None);
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
    let (g_lines, t_lines, _indices) = print_unicode(&graph, &settings)?;

    let lines: Vec<Line<'static>> = g_lines
        .into_iter()
        .zip(t_lines)
        .map(|(g, t)| ansi_escape_line(&format!(" {g}  {t}")))
        .collect();
    Ok(lines)
}
