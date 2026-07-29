use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

const KEYBINDS: &[(&str, &str)] = &[
    ("F1", "This help"),
    ("F2 / r", "Rename"),
    ("/ or F3", "Fuzzy search"),
    ("Ctrl+f", "Regex filter"),
    ("F4", "Edit file in $EDITOR"),
    ("F5", "Copy to other pane"),
    ("F6", "Move to other pane"),
    ("F7", "Create directory"),
    ("F8 / Del", "Move to trash"),
    ("Ctrl+t", "Create empty file"),
    ("a", "Create file/dir (/ = dir)"),
    ("F10", "Quit"),
    ("Enter", "Open directory / edit file"),
    ("Backspace", "Parent directory"),
    ("Tab, h, l", "Switch panes"),
    ("j, k, Up, Down", "Move cursor"),
    ("g / G", "First / last entry"),
    ("x", "Toggle select file"),
    ("y / p / P", "Yank / paste copy / paste move"),
    ("dd", "Move to trash"),
    (":", "Command palette"),
    ("Space", "Preview"),
    ("Ctrl+h", "Toggle hidden files"),
    ("Ctrl+l", "Refresh panes / redraw"),
    ("Shift+Left/Right", "Change sort column"),
    ("Shift+O", "Reverse sort order"),
    ("Ctrl+j/k or Ctrl+arrows", "Scroll preview"),
    ("Ctrl+f/b", "Preview: page down/up"),
    ("Ctrl+d/u", "Preview: half page down/up"),
    ("?", "About"),
    ("Esc", "Close / clear / quit"),
    ("q", "Quit"),
];

const COMMANDS: &[(&str, &str)] = &[
    (":q / :quit", "Quit"),
    (":w / :write", "Save config"),
    (":e / :cd <path>", "Navigate to directory"),
    (":mkdir <name>", "Create directory"),
    (":touch <name>", "Create empty file"),
    (":delete", "Trash selected/current"),
    (":rename <new>", "Rename current entry"),
    (":theme [name]", "Switch theme / list themes"),
    (":help", "This help"),
    (":shell", "Interactive subshell"),
    (":!<cmd>", "Run shell command"),
];

#[derive(Debug, Default)]
pub struct PopupKeybinds {}

impl PopupKeybinds {
    pub fn new() -> Self {
        Self::default()
    }
}

fn column_lines<'a>(entries: &'a [(&str, &str)], theme: &Theme) -> Vec<Line<'a>> {
    entries
        .iter()
        .map(|(key, description)| {
            Line::from(vec![
                Span::from(format!("{key:<24}")).style(theme.colors.primary()),
                Span::from(*description),
            ])
        })
        .collect()
}

impl Component for PopupKeybinds {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let popup_area = Rect {
            x: area.x + area.width / 8,
            y: area.y + area.height / 8,
            width: area.width * 3 / 4,
            height: area.height * 3 / 4,
        };

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title("Help  (F1 / :help)")
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(inner);

        let mut key_lines = vec![Line::from(Span::styled(
            "Keybindings",
            Style::default().fg(theme.colors.highlight()),
        ))];
        key_lines.extend(column_lines(KEYBINDS, theme));
        frame.render_widget(Paragraph::new(key_lines), columns[0]);

        let mut cmd_lines = vec![Line::from(Span::styled(
            "Commands",
            Style::default().fg(theme.colors.highlight()),
        ))];
        cmd_lines.extend(column_lines(COMMANDS, theme));
        frame.render_widget(Paragraph::new(cmd_lines), columns[1]);
    }
}
