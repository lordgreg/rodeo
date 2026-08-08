//! Bookmarks popup: lists the bookmarked paths, jumps to them, removes them.
//!
//! Whether a bookmark still exists is settled once, when the popup is built,
//! and cached as a [`PathState`]. `Component::render` runs inside
//! `terminal.draw` and must not touch the disk — stat'ing every bookmark on
//! every frame would put a filesystem walk in the paint path (see
//! `App::prepare_frame`). The cache is refreshed whenever the list changes,
//! and the path is re-checked before a jump, so a stale row cannot send the
//! pane somewhere that is no longer there.

use std::path::PathBuf;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{
    bookmarks::{Bookmarks, PathState},
    ui::{component::Component, theme::Theme},
};

/// Bookmarks past this many lose their number key; the list still scrolls.
const NUMBERED: usize = 9;

#[derive(Debug, Clone)]
pub struct BookmarkRow {
    pub path: PathBuf,
    /// What the path was when the popup opened. Re-checked before a jump, so a
    /// stale row cannot send the pane somewhere that is no longer there.
    pub state: PathState,
}

#[derive(Debug)]
pub struct BookmarksView {
    pub rows: Vec<BookmarkRow>,
    pub list_state: ListState,
}

impl BookmarksView {
    /// Builds the view, checking each path once.
    pub fn new(bookmarks: &Bookmarks) -> Self {
        let rows: Vec<BookmarkRow> = bookmarks
            .paths()
            .iter()
            .map(|path| BookmarkRow {
                state: PathState::of(path),
                path: path.clone(),
            })
            .collect();

        let mut list_state = ListState::default();
        if !rows.is_empty() {
            list_state.select(Some(0));
        }

        Self { rows, list_state }
    }

    pub fn selected_idx(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn row(&self, index: usize) -> Option<&BookmarkRow> {
        self.rows.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn missing_count(&self) -> usize {
        self.rows.iter().filter(|r| r.state.is_missing()).count()
    }

    pub fn move_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    pub fn move_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1).min(self.rows.len() - 1);
        self.list_state.select(Some(next));
    }

    /// Keeps the cursor on a row that still exists after one was removed.
    pub fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.list_state.select(None);
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.min(self.rows.len() - 1)));
    }

    /// One row: `1 ▸ name  parent  (missing)`.
    fn line(&self, index: usize, row: &BookmarkRow, theme: &Theme) -> Line<'static> {
        let key = if index < NUMBERED {
            format!("{} ", index + 1)
        } else {
            "  ".to_string()
        };

        let name = row
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            // `/` and other roots have no file name; show the path itself.
            .unwrap_or_else(|| row.path.display().to_string());

        let name_style = match row.state {
            // A dead bookmark is coloured, not merely annotated: the whole
            // point is that it stands out.
            PathState::Missing => Style::new().fg(theme.colors.error()),
            PathState::Unknown => Style::new().fg(theme.colors.warning()),
            PathState::Dir => Style::new().fg(theme.colors.accent1()).bold(),
            PathState::File => Style::new().fg(theme.colors.foreground()),
        };

        let mut spans = vec![
            Span::styled(key, Style::new().fg(theme.colors.muted())),
            Span::styled(name, name_style),
        ];

        if let Some(parent) = row.path.parent() {
            spans.push(Span::styled(
                format!("  {}", parent.display()),
                Style::new().fg(theme.colors.muted()),
            ));
        }

        match row.state {
            PathState::Missing => spans.push(Span::styled(
                "  (missing)",
                Style::new().fg(theme.colors.error()).bold(),
            )),
            // Said differently from "missing" on purpose: this one is not
            // pruned, because it may well still be there.
            PathState::Unknown => spans.push(Span::styled(
                "  (unreadable)",
                Style::new().fg(theme.colors.warning()).bold(),
            )),
            PathState::Dir | PathState::File => {}
        }

        Line::from(spans)
    }
}

impl Component for BookmarksView {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let popup = Rect {
            x: area.width / 10,
            y: area.height / 10,
            width: area.width * 4 / 5,
            height: area.height * 4 / 5,
        };
        frame.render_widget(Clear, popup);

        let missing = self.missing_count();
        let title = if missing > 0 {
            format!(" Bookmarks ({} — {missing} missing) ", self.rows.len())
        } else {
            format!(" Bookmarks ({}) ", self.rows.len())
        };

        let outer = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::new().fg(theme.colors.secondary()));
        let inner = outer.inner(popup);
        frame.render_widget(outer, popup);

        if self.rows.is_empty() {
            let msg = Paragraph::new("No bookmarks yet — press b on an entry to add one.")
                .style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, inner);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| ListItem::new(self.line(i, row, theme)))
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::new()
                    .fg(theme.colors.highlight())
                    .bg(theme.colors.surface())
                    .bold(),
            )
            .highlight_symbol("›");
        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Always the full set: keying the prune hint off `missing` hid the key
        // exactly when a bookmark died after the popup was opened.
        frame.render_widget(
            Paragraph::new("Enter jump  1-9 jump  d remove  P prune missing  Esc close")
                .style(Style::new().fg(theme.colors.muted())),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(paths: &[PathBuf]) -> BookmarksView {
        let mut bookmarks = Bookmarks::default();
        for p in paths {
            bookmarks.add(p.clone());
        }
        BookmarksView::new(&bookmarks)
    }

    #[test]
    fn a_path_that_no_longer_exists_is_marked_missing() {
        let dir = tempfile::tempdir().unwrap();
        let view = view_of(&[dir.path().join("long-gone")]);

        assert_eq!(view.rows[0].state, PathState::Missing);
        assert_eq!(view.missing_count(), 1);
    }

    #[test]
    fn a_directory_and_a_file_are_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"").unwrap();

        let view = view_of(&[dir.path().to_path_buf(), file]);

        assert_eq!(view.rows[0].state, PathState::Dir);
        assert_eq!(view.rows[1].state, PathState::File);
        assert_eq!(view.missing_count(), 0);
    }

    #[test]
    fn an_empty_list_has_nothing_under_the_cursor() {
        let view = view_of(&[]);

        assert!(view.is_empty());
        assert_eq!(view.selected_idx(), None);
    }

    #[test]
    fn the_cursor_starts_on_the_first_bookmark() {
        let view = view_of(&[PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(view.selected_idx(), Some(0));
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut view = view_of(&[PathBuf::from("/a"), PathBuf::from("/b")]);

        view.move_up();
        assert_eq!(view.selected_idx(), Some(0));

        view.move_down();
        view.move_down();
        assert_eq!(view.selected_idx(), Some(1));
    }

    /// Removing the last row used to leave the cursor pointing past the end.
    #[test]
    fn removing_the_last_row_pulls_the_cursor_back() {
        let mut view = view_of(&[PathBuf::from("/a"), PathBuf::from("/b")]);
        view.move_down();
        assert_eq!(view.selected_idx(), Some(1));

        view.rows.pop();
        view.clamp_selection();

        assert_eq!(view.selected_idx(), Some(0));
    }

    #[test]
    fn removing_the_only_row_leaves_no_cursor() {
        let mut view = view_of(&[PathBuf::from("/a")]);

        view.rows.clear();
        view.clamp_selection();

        assert_eq!(view.selected_idx(), None);
    }
}
