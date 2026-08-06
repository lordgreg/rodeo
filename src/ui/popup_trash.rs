//! Trash browser popup: lists trashed items, allows restore and permanent delete.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::ui::{component::Component, theme::Theme};

#[derive(Debug, Clone)]
pub struct TrashEntry {
    pub id: std::ffi::OsString,
    pub name: String,
    pub original_path: std::path::PathBuf,
    pub original_parent: std::path::PathBuf,
    pub selected: bool,
}

#[derive(Debug)]
pub struct TrashView {
    pub entries: Vec<TrashEntry>,
    pub list_state: ListState,
    pub error: Option<String>,
}

impl TrashView {
    /// Loads the current trash contents. Returns `Err` if the OS doesn't
    /// support listing trash (e.g. macOS without `os_limited`).
    pub fn load() -> Self {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
        {
            match trash::os_limited::list() {
                Ok(items) => {
                    let entries = items
                        .into_iter()
                        .map(|item| TrashEntry {
                            id: item.id.clone(),
                            name: item.name.to_string_lossy().into_owned(),
                            original_path: item.original_path(),
                            original_parent: item.original_parent.clone(),
                            selected: false,
                        })
                        .collect();
                    let mut view = Self {
                        entries,
                        list_state: ListState::default(),
                        error: None,
                    };
                    if !view.entries.is_empty() {
                        view.list_state.select(Some(0));
                    }
                    view
                }
                Err(e) => Self {
                    entries: vec![],
                    list_state: ListState::default(),
                    error: Some(format!("Cannot list trash: {e}")),
                },
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
        Self {
            entries: vec![],
            list_state: ListState::default(),
            error: Some("Trash listing is only supported on Linux and Windows.".into()),
        }
    }

    pub fn selected_idx(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    pub fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1).min(self.entries.len() - 1);
        self.list_state.select(Some(next));
    }

    pub fn toggle_select(&mut self) {
        let Some(i) = self.list_state.selected() else {
            return;
        };
        if let Some(e) = self.entries.get_mut(i) {
            e.selected = !e.selected;
        }
    }

    /// Items that are either marked with `x` or, if none are marked,
    /// just the currently highlighted item.
    pub fn op_targets(&self) -> Vec<&TrashEntry> {
        let marked: Vec<&TrashEntry> = self.entries.iter().filter(|e| e.selected).collect();
        if !marked.is_empty() {
            return marked;
        }
        self.list_state
            .selected()
            .and_then(|i| self.entries.get(i))
            .into_iter()
            .collect()
    }

    /// Restore the target entries to their original locations.
    pub fn restore_targets(&self) -> Result<usize, String> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
        {
            let targets = self.op_targets();
            let count = targets.len();
            let items: Vec<trash::TrashItem> = targets
                .iter()
                .map(|e| trash::TrashItem {
                    id: e.id.clone(),
                    name: std::ffi::OsString::from(&e.name),
                    original_parent: e.original_parent.clone(),
                    time_deleted: 0,
                })
                .collect();
            trash::os_limited::restore_all(items).map_err(|e| format!("Restore failed: {e}"))?;
            Ok(count)
        }
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
        Err("Restore not supported on this platform".into())
    }

    /// Permanently delete the target entries from trash.
    pub fn purge_targets(&self) -> Result<usize, String> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
        {
            let targets = self.op_targets();
            let count = targets.len();
            let items: Vec<trash::TrashItem> = targets
                .iter()
                .map(|e| trash::TrashItem {
                    id: e.id.clone(),
                    name: std::ffi::OsString::from(&e.name),
                    original_parent: e.original_parent.clone(),
                    time_deleted: 0,
                })
                .collect();
            trash::os_limited::purge_all(items).map_err(|e| format!("Purge failed: {e}"))?;
            Ok(count)
        }
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows")))]
        Err("Purge not supported on this platform".into())
    }
}

impl Component for TrashView {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let popup = Rect {
            x: area.width / 10,
            y: area.height / 10,
            width: area.width * 4 / 5,
            height: area.height * 4 / 5,
        };
        frame.render_widget(Clear, popup);

        let n = self.entries.iter().filter(|e| e.selected).count();
        let title = if n > 0 {
            format!(
                " Trash ({} items, {} selected) — r=restore  D=delete  x=select  Esc=close ",
                self.entries.len(),
                n
            )
        } else {
            format!(
                " Trash ({} items) — r=restore  D=delete  x=select  Esc=close ",
                self.entries.len()
            )
        };

        let outer = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::new().fg(theme.colors.secondary()));
        let inner = outer.inner(popup);
        frame.render_widget(outer, popup);

        if let Some(err) = &self.error {
            let msg = Paragraph::new(err.as_str()).style(Style::new().fg(theme.colors.error()));
            frame.render_widget(msg, inner);
            return;
        }

        if self.entries.is_empty() {
            let msg =
                Paragraph::new("Trash is empty.").style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, inner);
            return;
        }

        // Split into list and footer hint.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let marker = if e.selected {
                    Span::styled("● ", Style::new().fg(theme.colors.accent1()))
                } else {
                    Span::raw("  ")
                };
                let name = Span::styled(
                    e.name.clone(),
                    if e.selected {
                        Style::new().fg(theme.colors.warning()).bold()
                    } else {
                        Style::new().fg(theme.colors.primary())
                    },
                );
                let orig = Span::styled(
                    format!("  ← {}", e.original_path.display()),
                    Style::new().fg(theme.colors.muted()),
                );
                ListItem::new(Line::from(vec![marker, name, orig]))
            })
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

        let hint = Paragraph::new("r restore  D delete permanently  x toggle select  Esc close")
            .style(Style::new().fg(theme.colors.muted()));
        frame.render_widget(hint, chunks[1]);
    }
}
