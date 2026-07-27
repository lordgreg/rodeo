use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, panes::PaneStats, theme::Theme, uiconfig::UiConfig};

#[derive(Debug)]
pub struct Footer {
    pub keymaps: Vec<String>,
    stats: Option<PaneStats>,
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
        }
    }
}

impl Footer {
    pub fn _new(keymaps: Vec<String>) -> Self {
        Self {
            keymaps,
            stats: None,
        }
    }

    pub fn set_stats(&mut self, stats: PaneStats) {
        self.stats = Some(stats);
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

        let keymaps = self.visible_keymaps();

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
    }
}
