use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Debug, Default)]
pub struct PopupKeybinds {}

impl PopupKeybinds {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Component for PopupKeybinds {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let popup_area = Rect {
            x: area.x + area.width / 4,
            y: area.y + area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title("Keybinds")
            .borders(Borders::ALL)
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        frame.render_widget(block, popup_area);
    }
}
