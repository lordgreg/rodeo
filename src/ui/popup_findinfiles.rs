//! Find-in-files popup: recursive regex search over file contents, honouring
//! `.gitignore`.
//!
//! The popup is split like Telescope's: the hit list on the left, a syntax
//! highlighted preview of the selected hit — centred on the matching line —
//! on the right. Seeing the surrounding code is usually what decides whether a
//! hit is the one you want, so it must not cost a round trip through the
//! editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use syntect::{easy::HighlightLines, highlighting};

use crate::ui::{
    component::Component,
    popup_preview::{syntax_set, syntect_style_to_ratatui},
    textinput::TextInput,
    theme::Theme,
};

/// Lines kept on each side of the match in the preview window. Enough to
/// scroll a screenful or two without re-reading the file.
const PREVIEW_CONTEXT: usize = 150;
/// Beyond this line number the preview stops highlighting from the top of the
/// file (which is what keeps multi-line constructs correct) and starts at the
/// window instead, to bound the work per keystroke.
const HIGHLIGHT_FROM_START_LIMIT: usize = 20_000;
/// Below this total width the preview pane is dropped: two cramped columns are
/// worse than one usable one.
const MIN_WIDTH_FOR_PREVIEW: u16 = 80;
/// Share of the popup width given to the hit list.
const LIST_PERCENT: u16 = 45;
/// Marker in front of the query, standing in for the input box's old border.
const PROMPT: &str = "❯ ";

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

/// The preview for one selected hit, cached until the selection moves.
#[derive(Debug)]
enum Preview {
    /// Highlighted window of the file plus the 1-based number of its first
    /// line, so the gutter and the match highlight line up.
    Lines {
        lines: Vec<Line<'static>>,
        first_line: usize,
    },
    Error(String),
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
}

/// Reads a window of `path` around `line_num` and highlights it.
fn build_preview(path: &Path, line_num: usize, syn_theme: &highlighting::Theme) -> Preview {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return Preview::Error(format!("Cannot preview file: {e}")),
    };

    let first_line = line_num.saturating_sub(PREVIEW_CONTEXT).max(1);
    let last_line = line_num.saturating_add(PREVIEW_CONTEXT);

    let ss = syntax_set();
    let syntax = ss
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, syn_theme);

    // Highlighting from the top of the file keeps block comments and strings
    // coloured correctly; for very deep matches that is too much work, so the
    // window is highlighted on its own.
    let highlight_from = if line_num > HIGHLIGHT_FROM_START_LIMIT {
        first_line
    } else {
        1
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (idx, raw) in content.lines().enumerate() {
        let num = idx + 1;
        if num > last_line {
            break;
        }
        if num < highlight_from {
            continue;
        }
        // Tabs render as a single cell in a Paragraph, which shifts code out
        // of alignment with the gutter; expand them like an editor would.
        let text = raw.replace('\t', "    ");
        let spans = match highlighter.highlight_line(&text, ss) {
            Ok(regions) => regions
                .iter()
                .map(|(style, part)| {
                    Span::styled(part.to_string(), syntect_style_to_ratatui(*style))
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![Span::raw(text)],
        };
        if num >= first_line {
            lines.push(Line::from(spans));
        }
    }

    if lines.is_empty() {
        return Preview::Error("(empty file)".to_string());
    }

    Preview::Lines { lines, first_line }
}

impl FindInFiles {
    /// Draws the preview pane for the current selection into `area`.
    fn render_preview(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let title = match self.selected_match() {
            Some(m) => format!(" {}:{} ", self.display_path(m), m.line_num),
            None => " Preview ".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(theme.colors.border()))
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        self.ensure_preview();
        let match_line = self.selected_match().map(|m| m.line_num).unwrap_or(0);

        let (lines, first_line) = match self.preview.as_ref() {
            Some(Preview::Lines { lines, first_line }) => (lines, *first_line),
            Some(Preview::Error(msg)) => {
                let p = Paragraph::new(msg.as_str()).style(Style::new().fg(theme.colors.muted()));
                frame.render_widget(p, inner);
                return;
            }
            None => return,
        };

        // Centre the match, then apply any manual scroll, clamped so the
        // window cannot be scrolled off the loaded range.
        let height = inner.height as usize;
        let match_idx = match_line.saturating_sub(first_line);
        let centred = match_idx.saturating_sub(height / 2) as i32;
        let max_top = lines.len().saturating_sub(height) as i32;
        let top = (centred + self.preview_scroll).clamp(0, max_top.max(0)) as usize;
        // Keep the stored scroll in step with what is actually shown, so
        // holding Ctrl+d does not build up an offset that must be undone.
        self.preview_scroll = top as i32 - centred;

        let last_shown = first_line + (top + height).min(lines.len());
        let gutter = last_shown.to_string().len();

        let rendered: Vec<Line<'static>> = lines
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(i, line)| {
                let num = first_line + i;
                let is_match = num == match_line;
                let num_style = if is_match {
                    Style::new().fg(theme.colors.accent1()).bold()
                } else {
                    Style::new().fg(theme.colors.muted())
                };
                let mut spans = vec![Span::styled(format!("{num:>gutter$} "), num_style)];
                spans.extend(line.spans.iter().cloned());
                let out = Line::from(spans);
                if is_match {
                    out.style(Style::new().bg(theme.colors.surface()))
                } else {
                    out
                }
            })
            .collect();

        frame.render_widget(Paragraph::new(rendered), inner);
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

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Query line on top, results below, parted by a single rule. A second
        // box inside the popup border would be one frame too many for one
        // input field.
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        let input_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::new().fg(theme.colors.border()));
        let input_inner = input_block.inner(chunks[0]);
        frame.render_widget(input_block, chunks[0]);

        let input_text = Paragraph::new(Line::from(vec![
            Span::styled(PROMPT, Style::new().fg(theme.colors.accent1())),
            Span::from(self.input.value.clone()),
            Span::from(" "),
        ]));
        frame.render_widget(input_text, input_inner);

        // Position cursor
        if !self.searching {
            let x = input_inner.x + PROMPT.chars().count() as u16 + self.input.cursor as u16;
            frame.set_cursor_position((x, input_inner.y));
        }

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
    fn preview_window_is_centred_on_the_match_and_highlighted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let Preview::Lines { lines, first_line } =
            build_preview(&path, 200, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };

        assert_eq!(first_line, 200 - PREVIEW_CONTEXT);
        // The match itself is inside the loaded window.
        let text: String = lines[200 - first_line]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(text, "line 200");
    }

    #[test]
    fn preview_of_a_short_file_starts_at_line_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();

        let Preview::Lines { first_line, lines } =
            build_preview(&path, 2, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };
        assert_eq!(first_line, 1);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn missing_file_previews_as_an_error_instead_of_panicking() {
        let preview = build_preview(
            Path::new("/definitely/not/here.txt"),
            1,
            &highlighting::Theme::default(),
        );
        assert!(matches!(preview, Preview::Error(_)));
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
