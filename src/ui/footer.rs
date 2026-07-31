use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, panes::PaneStats, theme::Theme, uiconfig::UiConfig};

/// How long a status message stays visible.
const STATUS_TTL: Duration = Duration::from_secs(3);

#[derive(Debug)]
struct StatusMsg {
    text: String,
    is_error: bool,
    at: Instant,
}

#[derive(Debug)]
pub struct Footer {
    pub keymaps: Vec<String>,
    stats: Option<PaneStats>,
    clipboard: Option<(usize, bool)>,
    status: Option<StatusMsg>,
}

impl Default for Footer {
    fn default() -> Self {
        Self {
            keymaps: vec![
                "F1 Keys".to_string(),
                "F2 Rename".to_string(),
                "F3 Search".to_string(),
                "F4 Edit".to_string(),
                "F5 Copy".to_string(),
                "F6 Move".to_string(),
                "F7 Mkdir".to_string(),
                "F8 Delete".to_string(),
                "Space Preview".to_string(),
                "Tab Panes".to_string(),
                "x Select".to_string(),
                "^h Hidden".to_string(),
                "? About".to_string(),
                "F10 Quit".to_string(),
            ],
            stats: None,
            clipboard: None,
            status: None,
        }
    }
}

impl Footer {
    pub fn _new(keymaps: Vec<String>) -> Self {
        Self {
            keymaps,
            stats: None,
            clipboard: None,
            status: None,
        }
    }

    pub fn set_stats(&mut self, stats: PaneStats) {
        self.stats = Some(stats);
    }

    /// Shows a transient status message (auto-cleared after a few seconds).
    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status = Some(StatusMsg {
            text,
            is_error,
            at: Instant::now(),
        });
    }

    /// Clipboard state: (entry count, cut?) — shown until the clipboard is empty.
    pub fn set_clipboard(&mut self, clipboard: Option<(usize, bool)>) {
        self.clipboard = clipboard;
    }

    /// Bindings relevant to the current context: bulk actions when files are
    /// selected, the full key list otherwise.
    fn visible_keymaps(&self) -> Vec<String> {
        match &self.stats {
            Some(s) if s.selected > 0 => vec![
                format!("●{} selected", s.selected),
                "F5 Copy".to_string(),
                "F6 Move".to_string(),
                "F8 Delete".to_string(),
                "Esc Unselect".to_string(),
            ],
            _ => self.keymaps.clone(),
        }
    }
}

impl Component for Footer {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(theme.colors.surface()));
        let inner_area = bg_block.inner(area);
        frame.render_widget(bg_block, area);

        // Expired status messages are dropped.
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.at.elapsed() > STATUS_TTL)
        {
            self.status = None;
        }

        let mut keymaps = Vec::new();
        if let Some((count, cut)) = self.clipboard {
            keymaps.push(if cut {
                format!("[{count} cut]")
            } else {
                format!("[{count} yanked]")
            });
        }
        keymaps.extend(self.visible_keymaps());

        let constraints: Vec<Constraint> =
            std::iter::repeat_n(Constraint::Max(15), keymaps.len()).collect();

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner_area);

        for (i, keymap) in keymaps.iter().enumerate() {
            if let Some(&cell_area) = layout.get(i) {
                frame.render_widget(
                    Paragraph::new(keymap.as_str())
                        .style(Style::default().fg(theme.colors.foreground()))
                        .block(Block::default().padding(Padding::horizontal(1)))
                        .alignment(HorizontalAlignment::Left),
                    cell_area,
                );
            }
        }

        if let Some(status) = &self.status {
            let style = if status.is_error {
                Style::default().fg(theme.colors.error())
            } else {
                Style::default().fg(theme.colors.info())
            };
            frame.render_widget(
                Paragraph::new(status.text.as_str())
                    .style(style)
                    .block(Block::default().padding(Padding::horizontal(1)))
                    .alignment(HorizontalAlignment::Right),
                inner_area,
            );
        }
    }
}
