//! The help popup: keybindings and commands, laid out in as many columns as
//! the terminal height needs.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use crate::ui::{
    component::{Component, centered_popup},
    theme::Theme,
    uiconfig::UiConfig,
};

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
    ("w", "Preview: toggle line wrap"),
    ("?", "About"),
    ("Esc", "Close / clear / quit"),
    ("q", "Quit"),
];

const COMMANDS: &[(&str, &str)] = &[
    (":q / :quit", "Quit"),
    (":w / :write", "Save config"),
    (":so / :source", "Reload config"),
    (":e / :cd <path>", "Navigate to directory"),
    (":mkdir <name>", "Create directory"),
    (":touch <name>", "Create empty file"),
    (":delete", "Trash selected/current"),
    (":rename <new>", "Rename current entry"),
    (":theme [name]", "Switch theme / list themes"),
    (":trash", "Browse the trash"),
    (":help", "This help"),
    (":shell", "Interactive subshell"),
    (":!<cmd>", "Run shell command"),
];

/// Gap between two rendered columns.
const COLUMN_GAP: u16 = 2;
/// Even a reference table stops being readable past this width.
const MAX_WIDTH: u16 = 130;

#[derive(Debug, Default)]
pub struct PopupKeybinds {}

impl PopupKeybinds {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Width of the key column: the longest key in either list, plus a space.
fn key_column() -> usize {
    KEYBINDS
        .iter()
        .chain(COMMANDS)
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or_default()
        + 1
}

/// Every line of the help text: both sections with their headings.
fn all_lines(theme: &Theme) -> Vec<Line<'static>> {
    let key_column = key_column();
    let heading = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(theme.colors.highlight()),
        ))
    };
    let entry = |(key, description): &(&str, &str)| {
        Line::from(vec![
            Span::from(format!("{key:<key_column$}")).style(theme.colors.primary()),
            Span::from(description.to_string()),
        ])
    };

    let mut lines = vec![heading("Keybindings")];
    lines.extend(KEYBINDS.iter().map(entry));
    lines.push(Line::from(""));
    lines.push(heading("Commands"));
    lines.extend(COMMANDS.iter().map(entry));
    lines
}

impl Component for PopupKeybinds {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let lines = all_lines(theme);
        let line_width = lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16;

        // Lay the entries out in as many columns as it takes to fit the
        // terminal height, instead of one tall column that gets cut off.
        let usable_rows = area.height.saturating_sub(4).max(5);
        let columns = lines.len().div_ceil(usable_rows as usize).max(1) as u16;
        let rows = (lines.len() as u16).div_ceil(columns);

        let want_width = columns * line_width + (columns - 1) * COLUMN_GAP + 4;
        let popup_area = centered_popup(
            area,
            (want_width, rows + 2),
            (40, 8),
            (MAX_WIDTH, area.height),
        );

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

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .spacing(COLUMN_GAP)
            .constraints(vec![Constraint::Fill(1); columns as usize])
            .split(inner);

        for (index, chunk) in lines.chunks(rows as usize).enumerate() {
            frame.render_widget(Paragraph::new(chunk.to_vec()), layout[index]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_column_fits_the_longest_key() {
        let longest = KEYBINDS
            .iter()
            .chain(COMMANDS)
            .map(|(key, _)| key.len())
            .max()
            .unwrap();
        assert!(key_column() > longest);
    }

    #[test]
    fn every_binding_has_a_description() {
        assert!(
            KEYBINDS
                .iter()
                .chain(COMMANDS)
                .all(|(key, description)| !key.is_empty() && !description.is_empty())
        );
    }
}
