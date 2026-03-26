use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use unicode_width::UnicodeWidthStr;

use crate::key_hint;
use crate::session_alias_manager::SessionAliasManager;
use crate::session_utils::SessionInfo;
use crate::session_utils::get_cwd_sessions_for;
use crossterm::event::KeyCode;

/// Bottom session bar (similar to tmux)
pub struct SessionBar {
    codex_home: PathBuf,
    cwd: PathBuf,
    /// List of sessions in current working directory
    sessions: Vec<SessionInfo>,
    /// Currently selected session index
    selected_index: usize,
    /// Whether selection is on the special "New" tab
    selected_on_new: bool,
    /// Whether the bar has focus
    has_focus: bool,
    /// Session loading state
    loading: bool,
    /// Error message if any
    error: Option<String>,
    /// Current active session ID (if any)
    current_session_id: Option<String>,
    /// Status for the current session only
    current_session_status: Option<String>,
    /// Cached labels derived from first user message (by path)
    label_cache: HashMap<PathBuf, String>,
    /// Time when sessions were last refreshed from disk.
    last_refresh_at: Option<Instant>,
    /// Session alias manager
    alias_manager: SessionAliasManager,
}

impl SessionBar {
    pub fn new(cwd: PathBuf, codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            cwd,
            sessions: Vec::new(),
            selected_index: 0,
            selected_on_new: false,
            has_focus: false,
            loading: false,
            error: None,
            current_session_id: None,
            current_session_status: None,
            label_cache: HashMap::new(),
            last_refresh_at: None,
            alias_manager: SessionAliasManager::load(),
        }
    }

    /// Update session list and derived labels from a precomputed cache.
    ///
    /// This is primarily used by tests and any future background session cache
    /// that already performed disk IO.
    pub fn update_from_cache(
        &mut self,
        sessions: Vec<SessionInfo>,
        label_cache: HashMap<PathBuf, String>,
    ) {
        self.loading = false;
        self.error = None;
        self.label_cache = label_cache;
        self.set_sessions(sessions);
        self.last_refresh_at = Some(Instant::now());
    }

    /// Apply sessions preloaded in the background.
    ///
    /// Returns `true` when the preloaded data was accepted.
    pub fn apply_prefetched_sessions(&mut self, sessions: Vec<SessionInfo>) -> bool {
        // Ignore stale prefetch results after any foreground refresh.
        if self.last_refresh_at.is_some() {
            return false;
        }
        self.loading = false;
        self.error = None;
        self.set_sessions(sessions);
        self.last_refresh_at = Some(Instant::now());
        true
    }

    /// Update the tracked cwd for session discovery.
    ///
    /// When the active project changes, discard the cached session list so the
    /// next refresh or accepted prefetch repopulates the bar from the new cwd.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        if self.cwd == cwd {
            return;
        }

        self.cwd = cwd;
        self.sessions.clear();
        self.selected_index = 0;
        self.selected_on_new = true;
        self.loading = false;
        self.error = None;
        self.current_session_status = None;
        self.label_cache.clear();
        self.last_refresh_at = None;
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Refresh the session list from disk
    pub fn refresh_sessions(&mut self) {
        self.loading = true;
        self.error = None;

        match get_cwd_sessions_for(&self.codex_home, &self.cwd) {
            Ok(sessions) => {
                self.loading = false;
                self.set_sessions(sessions);
            }
            Err(e) => {
                self.error = Some(e);
                self.loading = false;
                self.sessions.clear();
            }
        }
        self.last_refresh_at = Some(Instant::now());
    }

    /// Refresh sessions only when cache is stale.
    pub fn refresh_sessions_if_stale(&mut self, max_age: Duration) {
        if let Some(last_refresh) = self.last_refresh_at
            && last_refresh.elapsed() < max_age
        {
            return;
        }
        self.refresh_sessions();
    }

    /// Get the currently selected session
    pub fn selected_session(&self) -> Option<&SessionInfo> {
        if self.selected_on_new {
            None
        } else {
            self.sessions.get(self.selected_index)
        }
    }

    /// Is the special "New" tab currently selected
    pub fn selected_is_new(&self) -> bool {
        self.selected_on_new
    }

    /// Move selection left
    pub fn select_previous(&mut self) {
        if self.selected_on_new {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            let has_new = self
                .current_session_id
                .as_deref()
                .map(|id| !self.sessions.iter().any(|s| s.id == id))
                .unwrap_or(true);
            if has_new {
                self.selected_on_new = true;
            }
        }
    }

    /// Move selection right
    pub fn select_next(&mut self) {
        if self.selected_on_new {
            self.selected_on_new = false;
            if !self.sessions.is_empty() {
                self.selected_index = 0;
            }
            return;
        }
        if self.selected_index < self.sessions.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Set focus state
    pub fn set_focus(&mut self, focused: bool) {
        self.has_focus = focused;
    }

    /// Get focus state
    pub fn has_focus(&self) -> bool {
        self.has_focus
    }

    fn set_sessions(&mut self, mut sessions: Vec<SessionInfo>) {
        // De-duplicate by id (keep newest).
        let mut seen = HashSet::new();
        sessions.retain(|session| seen.insert(session.id.clone()));
        self.sessions = sessions;

        // Drop labels for removed sessions.
        let known_paths: HashSet<PathBuf> = self.sessions.iter().map(|s| s.path.clone()).collect();
        self.label_cache
            .retain(|path, _| known_paths.contains(path));

        // If current session is in history, select it by default.
        if let Some(cur) = self.current_session_id.as_ref()
            && let Some(pos) = self.sessions.iter().position(|s| &s.id == cur)
        {
            self.selected_index = pos;
            self.selected_on_new = false;
        }

        // Keep selection in bounds.
        if self.selected_index >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected_index = self.sessions.len() - 1;
        }

        // Labels are extracted while scanning sessions; hydrate cache without extra file IO.
        for session in &self.sessions {
            let must_update = session.id == self.current_session_id.as_deref().unwrap_or("")
                || !self.label_cache.contains_key(&session.path);
            if must_update {
                if let Some(snippet) = session.last_user_snippet.as_ref() {
                    // Unicode-safe truncation to keep bar compact.
                    let short = if snippet.chars().count() > 10 {
                        let truncated: String = snippet.chars().take(10).collect();
                        format!("{truncated}…")
                    } else {
                        snippet.clone()
                    };
                    self.label_cache.insert(session.path.clone(), short);
                } else {
                    self.label_cache.remove(&session.path);
                }
            }
        }
    }

    /// Set current session ID
    pub fn set_current_session(&mut self, session_id: Option<String>) {
        if self.current_session_id != session_id {
            self.current_session_status = None;
        }
        self.current_session_id = session_id;
    }

    /// Update status text for the current session only
    pub fn set_session_status(&mut self, session_id: String, status: String) {
        if self.current_session_id.as_ref() == Some(&session_id) {
            self.current_session_status = Some(status);
        }
    }

    /// Reset selection when the bar gains focus.
    pub fn reset_selection_for_focus(&mut self, current_session_id: Option<&str>) {
        if let Some(id) = current_session_id
            && let Some(pos) = self.sessions.iter().position(|s| s.id == id)
        {
            self.selected_index = pos;
            self.selected_on_new = false;
            return;
        }
        self.selected_on_new = true;
        if !self.sessions.is_empty() {
            self.selected_index = 0;
        }
    }

    /// Set alias for a session
    pub fn set_session_alias(&mut self, session_id: String, alias: String) {
        self.alias_manager.set_alias(session_id, alias);
    }

    /// Remove alias for a session
    pub fn remove_session_alias(&mut self, session_id: &str) {
        self.alias_manager.remove_alias(session_id);
    }

    /// Build the session bar lines.
    ///
    /// Returns: (sessions_line, status_line, help_line, sel_start, sel_end, total_left_width)
    fn build_bar_lines(
        &self,
        current_session_id: Option<&str>,
    ) -> (
        Line<'static>,
        Line<'static>,
        Line<'static>,
        Option<u16>,
        Option<u16>,
        u16,
    ) {
        if let Some(error) = &self.error {
            return (
                Line::from(vec![
                    Span::from(" Error: ").red().bold(),
                    Span::from(error.clone()).red(),
                ]),
                Line::from(""),
                Line::from(""),
                None,
                None,
                0,
            );
        }

        let mut left_spans = Vec::new();
        let mut cur_x: u16 = 0;
        let mut sel_start: Option<u16> = None;
        let mut sel_end: Option<u16> = None;
        let add_left =
            |spans: &mut Vec<Span<'static>>, cur_x: &mut u16, text: String, style: Style| {
                *cur_x = cur_x.saturating_add(UnicodeWidthStr::width(text.as_str()) as u16);
                spans.push(Span::styled(text, style));
            };

        let current_in_history = current_session_id
            .map(|id| self.sessions.iter().any(|s| s.id == id))
            .unwrap_or(false);

        // Only show a standalone "New" when the current session is not in history
        if !current_in_history {
            let new_style = if self.has_focus && self.selected_on_new {
                Style::default().cyan().add_modifier(Modifier::BOLD)
            } else {
                Style::default().dim()
            };
            if self.has_focus && self.selected_on_new && sel_start.is_none() {
                sel_start = Some(cur_x);
            }
            add_left(&mut left_spans, &mut cur_x, "New".to_string(), new_style);
            if self.has_focus && self.selected_on_new {
                sel_end = Some(cur_x);
            }
        }

        if self.sessions.is_empty() {
            add_left(
                &mut left_spans,
                &mut cur_x,
                " ".to_string(),
                Style::default(),
            );
            left_spans.push(Span::from("│").dim());
            add_left(
                &mut left_spans,
                &mut cur_x,
                " ".to_string(),
                Style::default(),
            );
            left_spans.push(Span::from("No history").italic().dim());
        } else {
            for (idx, session) in self.sessions.iter().enumerate() {
                let is_selected = self.selected_index == idx;

                if idx > 0 || !current_in_history {
                    add_left(
                        &mut left_spans,
                        &mut cur_x,
                        " • ".to_string(),
                        Style::default().dim(),
                    );
                } else {
                    add_left(
                        &mut left_spans,
                        &mut cur_x,
                        " ".to_string(),
                        Style::default(),
                    );
                }

                let session_id = if session.id.len() > 8 {
                    format!("{}…", &session.id[..7])
                } else {
                    session.id.clone()
                };

                let is_current = current_session_id.is_some_and(|id| id == session.id);
                let style = if is_current {
                    Style::default().green().add_modifier(Modifier::BOLD)
                } else if self.has_focus && is_selected {
                    Style::default().cyan().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let display_name = if let Some(alias) = self.alias_manager.get_alias(&session.id) {
                    alias
                } else if let Some(snippet) = self.label_cache.get(&session.path) {
                    if snippet.is_empty() {
                        session_id.clone()
                    } else {
                        format!("{snippet} · {session_id}")
                    }
                } else {
                    session_id.clone()
                };

                if is_selected && sel_start.is_none() {
                    sel_start = Some(cur_x);
                }
                add_left(&mut left_spans, &mut cur_x, display_name, style);
                if is_selected {
                    sel_end = Some(cur_x);
                }

                if !is_selected && session.message_count > 0 {
                    left_spans.push(Span::from(format!("({})", session.message_count)).dim());
                }
            }
        }

        // Build status line (right side of first line)
        let mut status_spans: Vec<Span<'static>> = Vec::new();
        status_spans.push(Span::from(" Status:").dim());
        status_spans.push(Span::from(" "));
        let (status_label, status_name) = if let Some(cur_id) = current_session_id {
            let display_name = if let Some(alias) = self.alias_manager.get_alias(cur_id) {
                alias
            } else if cur_id.len() > 8 {
                format!("{}…", &cur_id[..7])
            } else {
                cur_id.to_string()
            };
            let st = self
                .current_session_status
                .clone()
                .unwrap_or_else(|| "Ready".to_string());
            (st, display_name)
        } else {
            ("Ready".to_string(), "New".to_string())
        };
        status_spans.push(Span::from(status_label).green().bold());
        status_spans.push(Span::from("  "));
        status_spans.push(Span::from("Session:").dim());
        status_spans.push(Span::from(" "));
        status_spans.push(Span::from(status_name).bold());

        // Build help line (second line with keyboard shortcuts)
        let mut help_spans: Vec<Span<'static>> = Vec::new();
        if self.has_focus {
            help_spans.push(Span::from(key_hint::plain(KeyCode::Left)));
            help_spans.push(Span::from("/".to_string()).dim());
            help_spans.push(Span::from(key_hint::plain(KeyCode::Right)));
            help_spans.push(Span::from(" move  ").dim());

            help_spans.push(Span::from(key_hint::plain(KeyCode::Enter)));
            help_spans.push(Span::from(" open  ").dim());

            help_spans.push(Span::from(key_hint::plain(KeyCode::Char('n'))));
            help_spans.push(Span::from(" new  ").dim());

            help_spans.push(Span::from(key_hint::plain(KeyCode::Char('r'))));
            help_spans.push(Span::from(" rename  ").dim());

            help_spans.push(Span::from(key_hint::plain(KeyCode::Char('x'))));
            help_spans.push(Span::from(" delete  ").dim());

            help_spans.push(Span::from(key_hint::plain(KeyCode::Esc)));
            help_spans.push(Span::from(" exit").dim());
        } else {
            help_spans.push(Span::from(key_hint::ctrl(KeyCode::Char('p'))));
            help_spans.push(Span::from(" Sessions").dim());
        }

        (
            Line::from(left_spans),
            Line::from(status_spans),
            Line::from(help_spans),
            sel_start,
            sel_end,
            cur_x,
        )
    }
}

impl WidgetRef for &SessionBar {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Draw a top border line to separate from chat area
        let border_rect = Rect::new(area.x, area.y, area.width, 1);
        Span::from("─".repeat(border_rect.width as usize))
            .dim()
            .render_ref(border_rect, buf);

        let bar_area = Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: area.height.saturating_sub(1),
        };

        let (sessions_line, status_line, help_line, sel_start, sel_end, total_left_width) =
            self.build_bar_lines(self.current_session_id.as_deref());

        Clear.render(bar_area, buf);

        if bar_area.height > 0 {
            let first_line_y = bar_area.y;
            let first_line_area = Rect {
                x: bar_area.x,
                y: first_line_y,
                width: bar_area.width,
                height: 1,
            };

            let status_width: u16 = status_line
                .spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()) as u16)
                .sum();

            let sessions_width = first_line_area
                .width
                .saturating_sub(status_width.saturating_add(3));
            let sessions_area = Rect {
                x: first_line_area.x,
                y: first_line_area.y,
                width: sessions_width,
                height: 1,
            };

            if status_width > 0 && sessions_width < first_line_area.width {
                let sep_x = first_line_area.x + sessions_width;
                if sep_x < first_line_area.x + first_line_area.width {
                    Span::from(" │ ")
                        .dim()
                        .render_ref(Rect::new(sep_x, first_line_area.y, 3, 1), buf);
                }
                let status_area = Rect {
                    x: first_line_area.x + first_line_area.width.saturating_sub(status_width),
                    y: first_line_area.y,
                    width: status_width,
                    height: 1,
                };
                Paragraph::new(vec![status_line.clone()]).render(status_area, buf);
            }

            // Compute horizontal scroll for sessions list
            let mut scroll_x: u16 = 0;
            if let (Some(start), Some(end)) = (sel_start, sel_end) {
                let sel_center = start.saturating_add(end).saturating_div(2);
                let half = sessions_area.width.saturating_div(2);
                let desired = sel_center.saturating_sub(half);
                let max_scroll = total_left_width.saturating_sub(sessions_area.width);
                scroll_x = desired.min(max_scroll);
            } else if total_left_width > sessions_area.width {
                scroll_x = total_left_width.saturating_sub(sessions_area.width);
            }

            Paragraph::new(vec![sessions_line])
                .scroll((0, scroll_x))
                .render(sessions_area, buf);

            // Second line: help/keyboard shortcuts (right-aligned)
            if bar_area.height > 1 {
                let second_line_y = bar_area.y + 1;

                let help_width: u16 = help_line
                    .spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()) as u16)
                    .sum();

                let help_area = if help_width < bar_area.width {
                    Rect {
                        x: bar_area.x + bar_area.width.saturating_sub(help_width),
                        y: second_line_y,
                        width: help_width,
                        height: 1,
                    }
                } else {
                    Rect {
                        x: bar_area.x,
                        y: second_line_y,
                        width: bar_area.width,
                        height: 1,
                    }
                };
                Paragraph::new(vec![help_line]).render(help_area, buf);
            }
        }
    }
}
