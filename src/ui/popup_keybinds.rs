use ratatui::{
    Frame,
    layout::Rect,
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
    ("F10", "Quit"),
    ("Enter", "Open directory / edit file"),
    ("Backspace", "Parent directory"),
    ("Tab, h, l", "Switch panes"),
    ("j, k, Up, Down", "Move cursor"),
    ("g / G", "First / last entry"),
    ("x", "Toggle select file"),
    ("Space", "Preview"),
    ("Ctrl+h", "Toggle hidden files"),
    ("Ctrl+l", "Refresh panes / redraw"),
    ("Shift+Left/Right", "Change sort column"),
    ("Shift+O", "Reverse sort order"),
    ("Ctrl+j / Ctrl+k", "Scroll preview"),
    ("?", "About"),
    ("Esc", "Close popup / clear filter / quit"),
    ("q", "Quit"),
];

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
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        let lines: Vec<Line> = KEYBINDS
            .iter()
            .map(|(key, description)| {
                Line::from(vec![
                    Span::from(format!("{key:<18}")).style(theme.colors.primary()),
                    Span::from(*description),
                ])
            })
            .collect();

        frame.render_widget(Paragraph::new(lines).block(block), popup_area);
    }
}
