use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, panes::PaneStats, theme::Theme, uiconfig::UiConfig};

/// How long a status message stays visible.
const STATUS_TTL: Duration = Duration::from_secs(3);
/// Blank cells between two footer hints.
const ENTRY_GAP: u16 = 2;

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

        // A fixed grid of narrow cells used to cut labels mid-word ("F7 Mkdi",
        // "^h Hidd"). Lay the hints out as one line instead and drop whole
        // entries from the end when they do not fit.
        let status_width = self
            .status
            .as_ref()
            .map(|s| s.text.chars().count() as u16 + 2)
            .unwrap_or(0);
        let available = inner_area.width.saturating_sub(status_width + 2);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0u16;
        let mut dropped = false;
        for entry in &keymaps {
            let width = entry.chars().count() as u16 + ENTRY_GAP;
            if used + width > available {
                dropped = true;
                break;
            }
            used += width;

            // "F5 Copy" renders the key in the accent colour, the action muted.
            match entry.split_once(' ') {
                Some((key, label)) => {
                    spans.push(Span::styled(
                        key.to_string(),
                        Style::default().fg(theme.colors.primary()),
                    ));
                    spans.push(Span::styled(
                        format!(" {label}"),
                        Style::default().fg(theme.colors.muted()),
                    ));
                }
                None => spans.push(Span::styled(
                    entry.clone(),
                    Style::default().fg(theme.colors.accent1()),
                )),
            }
            spans.push(Span::raw(" ".repeat(ENTRY_GAP as usize)));
        }
        if dropped {
            spans.push(Span::styled("…", Style::default().fg(theme.colors.muted())));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Left),
            inner_area,
        );

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
