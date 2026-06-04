use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Debug)]
pub struct Footer {
    pub keymaps: Vec<String>,
}

impl Default for Footer {
    fn default() -> Self {
        Self {
            keymaps: vec![
                "x Cut".to_string(),
                "p Paste".to_string(),
                "TAB switch".to_string(),
                "SPACE select".to_string(),
                "? preview".to_string(),
            ],
        }
    }
}

impl Footer {
    pub fn new(keymaps: Vec<String>) -> Self {
        Self { keymaps }
    }
}

impl Component for Footer {
    fn render(&self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(theme.colors.surface()));
        let inner_area = bg_block.inner(area);
        frame.render_widget(bg_block, area);

        let constraints: Vec<Constraint> = std::iter::repeat(Constraint::Max(15))
            .take(self.keymaps.len())
            .collect();

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(inner_area);

        for (i, keymap) in self.keymaps.iter().enumerate() {
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
