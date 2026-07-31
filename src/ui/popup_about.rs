//! The about popup, sized to its content.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::{
    component::{Component, centered_popup, content_size},
    theme::Theme,
    uiconfig::UiConfig,
};

#[derive(Debug, Default)]
pub struct PopupAbout {}

impl PopupAbout {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Component for PopupAbout {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let text = vec![
            ratatui::text::Line::from(env!("CARGO_PKG_NAME")),
            ratatui::text::Line::from(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("A modern terminal file manager"),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("Author: grepx"),
            ratatui::text::Line::from("https://codeberg.org/grepx/rodeo"),
        ];

        // Sized to its content — seven short lines never need half the screen.
        let widest = text
            .iter()
            .map(|l| l.width() as u16)
            .max()
            .unwrap_or_default();
        let popup_area = centered_popup(
            area,
            content_size(widest, text.len()),
            (24, 5),
            (60, area.height),
        );

        frame.render_widget(Clear, popup_area);

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
