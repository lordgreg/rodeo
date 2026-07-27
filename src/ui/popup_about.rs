use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Debug, Default)]
pub struct PopupAbout {}

impl PopupAbout {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Component for PopupAbout {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let popup_area = Rect {
            x: area.x + area.width / 4,
            y: area.y + area.height / 4,
            width: area.width / 2,
            height: area.height / 2,
        };

        frame.render_widget(Clear, popup_area);

        let text = vec![
            ratatui::text::Line::from(env!("CARGO_PKG_NAME")),
            ratatui::text::Line::from(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("A modern terminal file manager"),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("Author: grepx"),
            ratatui::text::Line::from("https://codeberg.org/grepx/rodeo"),
        ];

        let block = Block::default().title("About").borders(Borders::ALL).style(
            Style::default()
                .bg(theme.colors.surface())
                .fg(theme.colors.foreground()),
        );

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(paragraph, popup_area);
    }
}
