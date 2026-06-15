use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear},
};

use crate::ui::{component::Component, panes::Entry, theme::Theme, uiconfig::UiConfig};

#[derive(Debug)]
pub struct PopupPreview {
    entry: Entry,
}

impl PopupPreview {
    pub fn new(entry: Entry) -> Self {
        Self { entry: entry }
    }
}
impl Component for PopupPreview {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let popup_area = Rect {
            x: area.x + area.width / 4,
            y: area.y + area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(format!("Preview {}", self.entry.name))
            .borders(Borders::ALL)
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        frame.render_widget(block, popup_area);
    }
}
