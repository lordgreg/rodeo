//! Find-in-files popup: recursive regex search over file contents, honouring
//! the configured search filter (`.gitignore`, hidden files, extra entries).
//!
//! The popup is split like Telescope's: the hit list on the left, a syntax
//! highlighted preview of the selected hit — centred on the matching line —
//! on the right. Seeing the surrounding code is usually what decides whether a
//! hit is the one you want, so it must not cost a round trip through the
//! editor.

use std::path::PathBuf;
use std::sync::Arc;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use syntect::highlighting;

use crate::ui::{
    component::Component,
    filepreview::{Preview, build_preview, render_preview},
    popup_findfiles::{MIN_WIDTH_FOR_PREVIEW, render_query_line},
    textinput::TextInput,
    theme::Theme,
};

/// Share of the popup width given to the hit list.
const LIST_PERCENT: u16 = 45;

#[derive(Debug, Clone)]
pub struct FindMatch {
    pub path: PathBuf,
    pub line_num: usize,
    pub line_content: String,
}

impl FindMatch {
    pub fn display_path(&self) -> String {
        format!(
            "{}:{}: {}",
            self.path.display(),
            self.line_num,
            self.line_content.trim()
        )
    }
}

/// State for the find-in-files popup.
#[derive(Debug, Default)]
pub struct FindInFiles {
    pub input: TextInput,
    pub searching: bool,
    pub results: Vec<FindMatch>,
    pub list_state: ListState,
    /// The pattern the current `results` came from, so an empty list can be
    /// told apart from a search that has not been run yet — the popup must not
    /// claim "no matches" for a query nobody searched for.
    last_query: Option<String>,
    /// Directory the search was started from; hit paths are shown relative to
    /// it so the list stays readable.
    root: Option<PathBuf>,
    /// What the walk skipped, shown in the footer so a short result list is
    /// explained where it is seen.
    filter_label: Option<String>,
    /// Syntax colours shared with the file preview popup. `None` in tests and
    /// before the first search, where plain text is fine.
    syn_theme: Option<Arc<highlighting::Theme>>,
    /// Cached preview and the hit it belongs to.
    preview: Option<Preview>,
    preview_for: Option<(PathBuf, usize)>,
    /// Manual scroll away from the centred match line, in lines.
    preview_scroll: i32,
}

impl FindInFiles {
    pub fn new(syn_theme: Arc<highlighting::Theme>) -> Self {
        Self {
            syn_theme: Some(syn_theme),
            ..Self::default()
        }
    }

    /// `true` when `results` reflect exactly what is in the input box.
    pub fn results_are_current(&self) -> bool {
        self.last_query.as_deref() == Some(self.input.value.as_str())
    }

    pub fn start_search(&mut self, pattern: String) {
        self.searching = true;
        self.last_query = Some(pattern);
        self.results.clear();
        self.list_state.select(None);
        self.invalidate_preview();
    }

    /// Records the directory the search runs in, for relative display paths.
    pub fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
    }

    /// Records the active search filter, for the footer.
    pub fn set_filter_label(&mut self, label: String) {
        self.filter_label = Some(label);
    }

    pub fn finish_search(&mut self) {
        self.searching = false;
        if !self.results.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn add_result(&mut self, result: FindMatch) {
        self.results.push(result);
    }

    pub fn selected_match(&self) -> Option<&FindMatch> {
        self.list_state.selected().and_then(|i| self.results.get(i))
    }

    pub fn move_up(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        if i > 0 {
            self.list_state.select(Some(i - 1));
            self.preview_scroll = 0;
        }
    }

    pub fn move_down(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        if i + 1 < self.results.len() {
            self.list_state.select(Some(i + 1));
            self.preview_scroll = 0;
        }
    }

    /// Scrolls the preview without moving the selection, like Telescope's
    /// `Ctrl+d`/`Ctrl+u`. Clamped against the loaded window at render time.
    pub fn scroll_preview(&mut self, delta: i32) {
        self.preview_scroll = self.preview_scroll.saturating_add(delta);
    }

    pub fn clear(&mut self) {
        self.input = TextInput::default();
        self.results.clear();
        self.list_state.select(None);
        self.searching = false;
        self.last_query = None;
        self.invalidate_preview();
    }

    fn invalidate_preview(&mut self) {
        self.preview = None;
        self.preview_for = None;
        self.preview_scroll = 0;
    }

    /// Path of a hit as shown in the list: relative to the search root when
    /// possible, absolute otherwise.
    fn display_path(&self, m: &FindMatch) -> String {
        let rel = self
            .root
            .as_deref()
            .and_then(|root| m.path.strip_prefix(root).ok())
            .unwrap_or(m.path.as_path());
        rel.display().to_string()
    }

    /// Builds (or reuses) the preview for the current selection.
    fn ensure_preview(&mut self) {
        let Some(m) = self.selected_match() else {
            self.preview = None;
            self.preview_for = None;
            return;
        };
        let key = (m.path.clone(), m.line_num);
        if self.preview_for.as_ref() == Some(&key) {
            return;
        }
        let theme = self.syn_theme.clone().unwrap_or_default();
        self.preview = Some(build_preview(&key.0, key.1, &theme));
        self.preview_for = Some(key);
        self.preview_scroll = 0;
    }

    /// Draws the preview pane for the current selection into `area`.
    fn render_preview(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let title = match self.selected_match() {
            Some(m) => format!("{}:{}", self.display_path(m), m.line_num),
            None => "Preview".to_string(),
        };
        self.ensure_preview();
        let anchor = self.selected_match().map(|m| m.line_num);
        let mut scroll = self.preview_scroll;
        render_preview(
            frame,
            theme,
            area,
            &title,
            self.preview.as_ref(),
            anchor,
            &mut scroll,
        );
        self.preview_scroll = scroll;
    }
}

impl Component for FindInFiles {
    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        theme: &Theme,
        _ui: &crate::ui::uiconfig::UiConfig,
        area: Rect,
    ) {
        let title = if self.results_are_current() && !self.results.is_empty() {
            format!(" Find in Files — {} matches ", self.results.len())
        } else {
            " Find in Files (Ctrl+G) ".to_string()
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(theme.colors.secondary());

        // The popup-local keys are not in the global help table, so they are
        // spelled out on the border where they are needed.
        if area.width > 70 {
            block = block.title_bottom(
                Line::from(
                    " ↑↓/Ctrl+n,p select · Ctrl+d/u scroll preview · Enter open · Esc close ",
                )
                .style(Style::new().fg(theme.colors.muted()))
                .right_aligned(),
            );
        }
        // What the search was not allowed to look at belongs next to its
        // results, not only in the config file.
        if let Some(label) = &self.filter_label {
            block = block.title_bottom(
                Line::from(format!(" {label} "))
                    .style(Style::new().fg(theme.colors.muted()))
                    .left_aligned(),
            );
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Query line on top, results below, parted by a single rule. A second
        // box inside the popup border would be one frame too many for one
        // input field.
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        render_query_line(
            frame,
            theme,
            chunks[0],
            &self.input,
            !self.searching,
            // A find-in-files query is always a regex, so there is no mode to
            // report — only whether it compiles, which Enter already reports.
            None,
        );

        let body = chunks[1];

        // Render results or status message
        if self.searching {
            let msg = Paragraph::new("Searching...").style(Style::new().fg(theme.colors.info()));
            frame.render_widget(msg, body);
            return;
        }
        if !self.results_are_current() {
            // Nothing has been searched for what is in the box yet. Say so
            // rather than showing a verdict on a search that never ran.
            let hint = if self.input.value.is_empty() {
                "Type a regular expression, then Enter to search this directory and below"
            } else {
                "Press Enter to search"
            };
            let msg = Paragraph::new(hint).style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, body);
            return;
        }
        if self.results.is_empty() {
            let msg =
                Paragraph::new("No matches found").style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, body);
            return;
        }

        // Telescope-style split: hits left, preview right. Narrow terminals
        // keep the full width for the list.
        let (list_area, preview_area) = if body.width >= MIN_WIDTH_FOR_PREVIEW {
            let cols = Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(LIST_PERCENT),
                    Constraint::Percentage(100 - LIST_PERCENT),
                ])
                .split(body);
            (cols[0], Some(cols[1]))
        } else {
            (body, None)
        };

        let items: Vec<ListItem> = self
            .results
            .iter()
            .map(|m| {
                let line = Line::from(vec![
                    Span::styled(
                        format!("{}:{}", self.display_path(m), m.line_num),
                        Style::new().fg(theme.colors.accent1()),
                    ),
                    Span::from(": "),
                    Span::from(m.line_content.trim().to_string()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::new()
                    .fg(theme.colors.highlight())
                    .bg(theme.colors.surface())
                    .bold(),
            )
            .highlight_symbol("› ");

        frame.render_stateful_widget(list, list_area, &mut self.list_state);

        if let Some(preview_area) = preview_area {
            self.render_preview(frame, theme, preview_area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_with(results: Vec<FindMatch>) -> FindInFiles {
        let mut find = FindInFiles::default();
        find.input.value = "x".to_string();
        find.start_search("x".to_string());
        for r in results {
            find.add_result(r);
        }
        find.finish_search();
        find
    }

    #[test]
    fn preview_is_cached_until_the_selection_moves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\ntwo\n").unwrap();

        let mut find = find_with(vec![
            FindMatch {
                path: path.clone(),
                line_num: 1,
                line_content: "one".into(),
            },
            FindMatch {
                path: path.clone(),
                line_num: 2,
                line_content: "two".into(),
            },
        ]);

        find.ensure_preview();
        assert_eq!(find.preview_for, Some((path.clone(), 1)));

        find.move_down();
        // Selection moved: the cache no longer matches and is rebuilt.
        assert_eq!(find.preview_for, Some((path.clone(), 1)));
        find.ensure_preview();
        assert_eq!(find.preview_for, Some((path, 2)));
    }

    #[test]
    fn moving_the_selection_resets_manual_preview_scroll() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let mut find = find_with(vec![
            FindMatch {
                path: path.clone(),
                line_num: 1,
                line_content: "one".into(),
            },
            FindMatch {
                path,
                line_num: 2,
                line_content: "two".into(),
            },
        ]);

        find.scroll_preview(10);
        assert_eq!(find.preview_scroll, 10);
        find.move_down();
        assert_eq!(find.preview_scroll, 0);
    }

    #[test]
    fn hits_are_listed_relative_to_the_search_root() {
        let mut find = FindInFiles::default();
        find.set_root(PathBuf::from("/home/user/project"));
        let m = FindMatch {
            path: PathBuf::from("/home/user/project/src/main.rs"),
            line_num: 3,
            line_content: "fn main() {}".into(),
        };
        assert_eq!(find.display_path(&m), "src/main.rs");

        // Outside the root the absolute path is kept — a relative path would
        // be a lie about where the hit is.
        let outside = FindMatch {
            path: PathBuf::from("/etc/hosts"),
            line_num: 1,
            line_content: "localhost".into(),
        };
        assert_eq!(find.display_path(&outside), "/etc/hosts");
    }
}
