//! File finder popup (`/`): find a file anywhere below the current directory
//! by name.
//!
//! The tree is walked once when the popup opens — subject to the configured
//! search filter — and every keystroke then filters that list in memory, which
//! is what keeps it usable on a large project. The query box is the same one
//! the pane filter uses: a plain word is matched fuzzily, a regular expression
//! is matched as a regular expression, and the popup says which of the two it
//! is doing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use syntect::highlighting;

use crate::{
    fs::filter::SearchFilter,
    ui::{
        component::Component,
        filepreview::{PreviewPane, build_dir_preview, build_preview},
        search::{FilterSpec, Query},
        textinput::TextInput,
        theme::Theme,
    },
};

/// Below this total width the preview pane is dropped: two cramped columns are
/// worse than one usable one. Shared with the find-in-files popup so both
/// behave the same on a narrow terminal.
pub const MIN_WIDTH_FOR_PREVIEW: u16 = 80;
/// Marker in front of the query, standing in for the input box's old border.
pub const PROMPT: &str = "❯ ";
/// Share of the popup width given to the result list. Shared with
/// find-in-files so the two popups keep the same proportions.
pub const LIST_PERCENT: u16 = 45;
/// Most paths collected from the tree. A walk of a home directory can produce
/// millions; past this many the finder stops and says the list is partial,
/// rather than spending seconds building a list nobody can read.
const MAX_ENTRIES: usize = 50_000;
/// Most results kept for one query. Nobody scrolls past this, and it bounds
/// the sort that happens on every keystroke.
const MAX_RESULTS: usize = 1_000;

/// One candidate path found below the search root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEntry {
    pub path: PathBuf,
    /// Path relative to the search root — what is matched and displayed.
    pub rel: String,
    pub is_dir: bool,
}

impl FoundEntry {
    /// How the entry appears in the list: directories keep a trailing slash,
    /// the way every other file manager writes them.
    pub fn display(&self) -> String {
        if self.is_dir {
            format!("{}/", self.rel)
        } else {
            self.rel.clone()
        }
    }
}

/// State for the file finder popup.
#[derive(Debug, Default)]
pub struct FileFinder {
    pub input: TextInput,
    /// Everything the walk found, scanned once when the popup opened.
    entries: Vec<FoundEntry>,
    /// Indices into `entries`, best match first.
    results: Vec<usize>,
    pub list_state: ListState,
    /// The walk hit [`MAX_ENTRIES`] and stopped early.
    truncated: bool,
    /// What the walk skipped, shown in the footer.
    filter_label: Option<String>,
    /// Cached preview of the highlighted entry, with its scroll position.
    preview: PreviewPane<PathBuf>,
}

impl FileFinder {
    /// Opens the finder on `root`, scanning it immediately.
    pub fn new(root: PathBuf, filter: &SearchFilter, syn_theme: Arc<highlighting::Theme>) -> Self {
        let (entries, truncated) = scan(&root, filter);

        let mut finder = Self {
            input: TextInput::default(),
            entries,
            results: Vec::new(),
            list_state: ListState::default(),
            truncated,
            filter_label: Some(filter.describe()),
            preview: PreviewPane::new(syn_theme),
        };
        finder.refilter();
        finder
    }

    /// Number of candidates the walk found.
    pub fn scanned(&self) -> usize {
        self.entries.len()
    }

    pub fn results(&self) -> impl Iterator<Item = &FoundEntry> {
        self.results.iter().filter_map(|i| self.entries.get(*i))
    }

    pub fn selected(&self) -> Option<&FoundEntry> {
        let index = *self.results.get(self.list_state.selected()?)?;
        self.entries.get(index)
    }

    /// Re-runs the current query over the scanned entries. Called after every
    /// edit of the query box, so it must stay linear and allocation-light.
    pub fn refilter(&mut self) {
        let query = self.input.value.clone();
        let mut matcher = Query::new(&query);

        let mut scored: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| matcher.score(&entry.rel).map(|score| (score, i)))
            .collect();
        // Stable, so equal scores (every regex and empty-query match) keep the
        // walk's order, which is depth-first and therefore already sensible.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.truncate(MAX_RESULTS);

        self.results = scored.into_iter().map(|(_, i)| i).collect();
        self.list_state
            .select((!self.results.is_empty()).then_some(0));
        self.invalidate_preview();
    }

    pub fn move_up(&mut self) {
        let Some(i) = self.list_state.selected() else {
            return;
        };
        if i > 0 {
            self.list_state.select(Some(i - 1));
            self.preview.reset_scroll();
        }
    }

    pub fn move_down(&mut self) {
        let Some(i) = self.list_state.selected() else {
            return;
        };
        if i + 1 < self.results.len() {
            self.list_state.select(Some(i + 1));
            self.preview.reset_scroll();
        }
    }

    pub fn scroll_preview(&mut self, delta: i32) {
        self.preview.scroll_by(delta);
    }

    fn invalidate_preview(&mut self) {
        self.preview.invalidate();
    }

    /// Builds (or reuses) the preview for the current selection.
    ///
    /// Called before the frame, never during it: building reads a file — or
    /// lists a directory — from disk, which has no business happening inside
    /// a draw.
    pub(crate) fn prepare(&mut self) {
        let selected = self.selected().cloned();
        let is_dir = selected.as_ref().is_some_and(|e| e.is_dir);

        self.preview
            .ensure(selected.map(|e| e.path), |path, syntax| {
                if is_dir {
                    build_dir_preview(path)
                } else {
                    build_preview(path, 1, syntax)
                }
            });
    }

    fn render_preview(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let title = match self.selected() {
            Some(entry) => entry.display(),
            None => "Preview".to_string(),
        };

        // No anchor: a file finder preview starts at the top of the file.
        self.preview.render(frame, theme, area, &title, None);
    }
}

/// Walks `root` and collects every entry below it, filtered and bounded.
fn scan(root: &Path, filter: &SearchFilter) -> (Vec<FoundEntry>, bool) {
    let mut entries = Vec::new();
    let mut truncated = false;

    for entry in filter.walk(root) {
        let Ok(entry) = entry else { continue };
        // Depth 0 is the search root itself, which is not a result.
        if entry.depth() == 0 {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        entries.push(FoundEntry {
            path: path.to_path_buf(),
            rel: rel.to_string_lossy().into_owned(),
            is_dir: entry.file_type().is_some_and(|t| t.is_dir()),
        });
    }

    (entries, truncated)
}

/// Draws the shared query line: prompt, text, cursor and — when the caller
/// passes one — a right-aligned label saying how the query is being read.
pub fn render_query_line(
    frame: &mut Frame<'_>,
    theme: &Theme,
    area: Rect,
    input: &TextInput,
    show_cursor: bool,
    mode: Option<&str>,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::new().fg(theme.colors.border()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new(Line::from(vec![
        Span::styled(PROMPT, Style::new().fg(theme.colors.accent1())),
        Span::from(input.value.clone()),
        Span::from(" "),
    ]));
    frame.render_widget(text, inner);

    if let Some(mode) = mode {
        let label = Paragraph::new(
            Line::from(format!("{mode} "))
                .style(Style::new().fg(theme.colors.muted()))
                .right_aligned(),
        );
        frame.render_widget(label, inner);
    }

    if show_cursor {
        let x = inner.x + PROMPT.chars().count() as u16 + input.cursor as u16;
        frame.set_cursor_position((x, inner.y));
    }
}

/// How the query box is currently interpreting what was typed.
fn query_mode(query: &str) -> &'static str {
    if query.is_empty() {
        "all files"
    } else if FilterSpec::is_broken_regex(query) {
        // Reads as a regex but does not compile (yet): say so, otherwise the
        // fuzzy fallback looks like the regex silently doing the wrong thing.
        "regex (incomplete) → fuzzy"
    } else {
        FilterSpec::detect(query).kind_label()
    }
}

impl Component for FileFinder {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let title = format!(
            " Find Files — {} of {}{} ",
            self.results.len(),
            self.entries.len(),
            if self.truncated { "+" } else { "" }
        );
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(theme.colors.secondary());

        if area.width > 70 {
            block = block.title_bottom(
                Line::from(" ↑↓/Ctrl+n,p select · Ctrl+d/u scroll · Enter go to · Ctrl+e edit · Esc close ")
                    .style(Style::new().fg(theme.colors.muted()))
                    .right_aligned(),
            );
        }
        if let Some(label) = &self.filter_label {
            block = block.title_bottom(
                Line::from(format!(" {label} "))
                    .style(Style::new().fg(theme.colors.muted()))
                    .left_aligned(),
            );
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);

        render_query_line(
            frame,
            theme,
            chunks[0],
            &self.input,
            true,
            Some(query_mode(&self.input.value)),
        );

        let body = chunks[1];

        if self.entries.is_empty() {
            let msg = Paragraph::new("Nothing to search here (everything is filtered out?)")
                .style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, body);
            return;
        }
        if self.results.is_empty() {
            let msg = Paragraph::new("No files match").style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, body);
            return;
        }

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
            .results()
            .map(|entry| {
                let style = if entry.is_dir {
                    Style::new().fg(theme.colors.accent1()).bold()
                } else {
                    Style::new().fg(theme.colors.foreground())
                };
                ListItem::new(Line::from(Span::styled(entry.display(), style)))
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

    /// A small tree with something to filter out in it.
    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("src/deep/util.rs"), "// util\n").unwrap();
        std::fs::write(root.join("target/main.rs"), "// built\n").unwrap();
        std::fs::write(root.join(".hidden"), "x\n").unwrap();
        std::fs::write(root.join("README.md"), "# hi\n").unwrap();
        dir
    }

    fn finder(dir: &Path, filter: SearchFilter) -> FileFinder {
        FileFinder::new(dir.to_path_buf(), &filter, Arc::default())
    }

    fn shown(finder: &FileFinder) -> Vec<String> {
        finder.results().map(|e| e.rel.clone()).collect()
    }

    fn type_query(finder: &mut FileFinder, query: &str) {
        for c in query.chars() {
            finder.input.insert(c);
        }
        finder.refilter();
    }

    #[test]
    fn an_empty_query_lists_everything_that_was_scanned() {
        let dir = tree();
        let finder = finder(dir.path(), SearchFilter::default());
        let names = shown(&finder);
        assert!(names.contains(&"src/main.rs".to_string()), "{names:?}");
        assert!(names.contains(&"src/deep/util.rs".to_string()), "{names:?}");
        // Directories are candidates too — the finder navigates to them.
        assert!(names.contains(&"src".to_string()), "{names:?}");
    }

    #[test]
    fn typing_a_word_matches_fuzzily_across_subdirectories() {
        let dir = tree();
        let mut finder = finder(dir.path(), SearchFilter::default());
        type_query(&mut finder, "util");

        let names = shown(&finder);
        assert_eq!(names, vec!["src/deep/util.rs".to_string()]);
    }

    #[test]
    fn the_same_box_takes_a_regex_without_switching_modes() {
        let dir = tree();
        let mut finder = finder(dir.path(), SearchFilter::default());
        type_query(&mut finder, r"^src/.*\.rs$");

        let mut names = shown(&finder);
        names.sort();
        assert_eq!(
            names,
            vec!["src/deep/util.rs".to_string(), "src/main.rs".to_string()]
        );
        assert_eq!(query_mode(r"^src/.*\.rs$"), "regex");
        assert_eq!(query_mode("util"), "fuzzy");
    }

    #[test]
    fn a_half_typed_regex_falls_back_to_fuzzy_and_says_so() {
        let dir = tree();
        let mut finder = finder(dir.path(), SearchFilter::default());
        type_query(&mut finder, "(main");

        // No panic, no empty screen: it is simply matched as text.
        assert!(query_mode("(main").contains("incomplete"));
        assert!(finder.results.len() <= finder.entries.len());
    }

    #[test]
    fn filtered_entries_and_hidden_files_never_reach_the_list() {
        let dir = tree();
        let filter = SearchFilter {
            gitignore: false,
            hidden: true,
            entries: vec!["target".to_string()],
        };
        let finder = finder(dir.path(), filter);
        let names = shown(&finder);

        assert!(names.contains(&"src/main.rs".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("target")), "{names:?}");
        assert!(!names.contains(&".hidden".to_string()), "{names:?}");
    }

    #[test]
    fn the_selection_stays_inside_the_result_list() {
        let dir = tree();
        let mut finder = finder(dir.path(), SearchFilter::default());
        for _ in 0..100 {
            finder.move_down();
        }
        assert_eq!(finder.list_state.selected(), Some(finder.results.len() - 1));
        for _ in 0..100 {
            finder.move_up();
        }
        assert_eq!(finder.list_state.selected(), Some(0));
        assert!(finder.selected().is_some());
    }

    #[test]
    fn a_query_matching_nothing_leaves_no_selection() {
        let dir = tree();
        let mut finder = finder(dir.path(), SearchFilter::default());
        type_query(&mut finder, "definitely-not-here");

        assert!(finder.results.is_empty());
        assert!(finder.selected().is_none());
        // Moving with nothing selected must not panic or select a ghost.
        finder.move_down();
        assert!(finder.selected().is_none());
    }

    #[test]
    fn directories_are_shown_with_a_trailing_slash() {
        let entry = FoundEntry {
            path: PathBuf::from("/p/src"),
            rel: "src".to_string(),
            is_dir: true,
        };
        assert_eq!(entry.display(), "src/");
    }
}
