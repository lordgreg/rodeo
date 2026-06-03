use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
};

pub fn ui_panes(frame: &mut Frame<'_>) {
    let outer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
        .split(frame.area());

    let pane_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(outer_layout[0]);

    frame.render_widget(
        Block::default().title("Left Pane").borders(Borders::ALL),
        pane_layout[0],
    );

    frame.render_widget(
        Block::default().title("Right Pane").borders(Borders::ALL),
        pane_layout[1],
    );

    frame.render_widget(
        Paragraph::new("Footer")
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)),
        outer_layout[1],
    );
}
