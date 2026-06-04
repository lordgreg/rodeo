use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Debug, Default)]
pub struct Header {
    pub info: String,
    pub directory: String,
    pub git_status: String,
}

impl Header {
    pub fn new(info: impl Into<String>, directory: impl Into<String>, git_status: impl Into<String>) -> Self {
        Self {
            info: info.into(),
            directory: directory.into(),
            git_status: git_status.into(),
        }
    }
}

impl Component for Header {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(theme.colors.surface()));
        let inner_area = bg_block.inner(area);
        frame.render_widget(bg_block, area);

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .flex(ratatui::layout::Flex::SpaceBetween)
            .constraints([
                Constraint::Fill(1),
                Constraint::Percentage(50),
                Constraint::Fill(1),
            ])
            .split(inner_area);

        frame.render_widget(
            Paragraph::new(&*self.info)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.foreground())),
            layout[0],
        );
        frame.render_widget(
            Paragraph::new(&*self.directory)
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Center)
                .style(Style::default().fg(theme.colors.foreground())),
            layout[1],
        );
        frame.render_widget(
            Paragraph::new(&*self.git_status)
                .alignment(HorizontalAlignment::Right)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.muted())),
            layout[2],
        );
    }
}
