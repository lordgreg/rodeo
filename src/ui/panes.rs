use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Row, Table, TableState},
};

use crate::ui::{
    component::Component,
    theme::Theme,
    uiconfig::{ActivePane, UiConfig},
};

#[derive(PartialEq)]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Debug)]
pub struct Panes {
    pub table_state_left: TableState,
    pub table_state_right: TableState,
    rows_left: Vec<Row<'static>>,
    rows_right: Vec<Row<'static>>,
    widths: [Constraint; 4],
}

impl Panes {
    pub fn new() -> Self {
        Self {
            table_state_left: TableState::default(),
            table_state_right: TableState::default(),
            rows_left: vec![
                Row::new(["", "..", "/", ""]),
                Row::new(["", "foo", "12kb", "date()"]),
                Row::new(["◌", "foo", "12kb", "date()"]),
                Row::new(["", "foo", "12kb", "date()"]),
                Row::new(["", "foo", "12kb", "date()"]),
            ],
            rows_right: vec![
                Row::new(["", "..", "/", ""]),
                Row::new(["", "foo2", "12kb", "date()"]),
                Row::new(["", "foo2", "12kb", "date()"]),
                Row::new(["", "foo2", "12kb", "date()"]),
                Row::new(["", "foo2", "12kb", "date()"]),
            ],
            widths: [
                Constraint::Max(1),
                Constraint::Fill(1),
                Constraint::Percentage(20),
                Constraint::Percentage(30),
            ],
        }
    }

    pub fn next_index(row_count: usize, current: Option<usize>, direction: MoveDirection) -> usize {
        let max = row_count.saturating_sub(1);
        match direction {
            MoveDirection::Down => match current {
                Some(i) if i >= max => 0,
                Some(i) => i + 1,
                None => 0,
            },
            MoveDirection::Up => match current {
                Some(0) => max,
                Some(i) => i - 1,
                None => 0,
            },
        }
    }

    pub fn goto_next(&mut self, ui: &UiConfig, direction: MoveDirection) {
        let (current, table_state, row_count) = if ui.active_pane == ActivePane::Left {
            (
                self.table_state_left.selected(),
                &mut self.table_state_left,
                self.rows_left.len(),
            )
        } else {
            (
                self.table_state_right.selected(),
                &mut self.table_state_right,
                self.rows_right.len(),
            )
        };

        let next = Self::next_index(row_count, current, direction);
        table_state.select(Some(next));
    }

    fn render_pane(
        frame: &mut Frame,
        area: Rect,
        table_state: &mut TableState,
        active: bool,
        theme: &Theme,
        rows: &[Row<'static>],
        widths: [Constraint; 4],
    ) {
        let color_border = if active {
            theme.colors.highlight()
        } else {
            theme.colors.border()
        };

        let header = Row::new(["", "Name", "Size", "Modify Time"])
            .style(Style::new().fg(theme.colors.highlight()).bold());

        let table = Table::new(rows.iter().cloned(), widths)
            .header(header)
            .column_spacing(1)
            .style(Style::new().fg(theme.colors.primary()))
            .row_highlight_style(
                Style::new()
                    .fg(theme.colors.secondary())
                    .bg(theme.colors.background())
                    .bold(),
            )
            .cell_highlight_style(Style::new().reversed().yellow())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("~")
                    .border_style(color_border)
                    .title_style(Style::new().fg(theme.colors.primary())),
            );

        frame.render_stateful_widget(table, area, table_state);
    }
}

impl Component for Panes {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, ui: &UiConfig, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        Self::render_pane(
            frame,
            layout[0],
            &mut self.table_state_left,
            ui.active_pane == ActivePane::Left,
            theme,
            &self.rows_left,
            self.widths,
        );

        Self::render_pane(
            frame,
            layout[1],
            &mut self.table_state_right,
            ui.active_pane == ActivePane::Right,
            theme,
            &self.rows_right,
            self.widths,
        );
    }
}
