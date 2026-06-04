use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders},
};

use crate::ui::{
    component::Component,
    theme::Theme,
    uiconfig::{ActivePane, UiConfig},
};

#[derive(Debug, Default)]
pub struct Panes {
    pub left_title: String,
    pub right_title: String,
}

impl Panes {
    pub fn new(left_title: impl Into<String>, right_title: impl Into<String>) -> Self {
        Self {
            left_title: left_title.into(),
            right_title: right_title.into(),
        }
    }
}

impl Component for Panes {
    fn render(&self, frame: &mut Frame<'_>, theme: &Theme, ui: &UiConfig, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let (left_style, right_style) = match ui.active_pane {
            ActivePane::Left => (
                Style::default().fg(theme.colors.primary()),
                Style::default().fg(theme.colors.border()),
            ),
            ActivePane::Right => (
                Style::default().fg(theme.colors.border()),
                Style::default().fg(theme.colors.primary()),
            ),
        };

        frame.render_widget(
            Block::default()
                .title(&*self.left_title)
                .borders(Borders::ALL)
                .border_style(left_style),
            layout[0],
        );

        frame.render_widget(
            Block::default()
                .title(&*self.right_title)
                .borders(Borders::ALL)
                .border_style(right_style),
            layout[1],
        );
    }
}
