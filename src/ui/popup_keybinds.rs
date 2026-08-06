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
    command,
    component::{Component, centered_popup},
    keymap::Action,
    theme::Theme,
    uiconfig::UiConfig,
};

/// One row of the keybinding table.
///
/// `actions` names the actions the row documents, so
/// `every_action_is_documented` fails when a new binding is added to the
/// keymap but not here. Rows for keys handled outside the keymap (preview
/// scrolling, `Esc`, …) list no action.
struct Keybind {
    keys: &'static str,
    description: &'static str,
    /// Only read by the coverage test below.
    #[cfg_attr(not(test), allow(dead_code))]
    actions: &'static [Action],
}

/// Shorthand for a table row.
const fn bind(
    keys: &'static str,
    description: &'static str,
    actions: &'static [Action],
) -> Keybind {
    Keybind {
        keys,
        description,
        actions,
    }
}

const KEYBINDS: &[Keybind] = &[
    bind("?", "This help", &[Action::Help]),
    bind("r", "Rename", &[Action::Rename]),
    bind("B", "Bulk rename (2+ selected)", &[Action::BulkRename]),
    bind(
        "/",
        "Find files by name (fuzzy or regex)",
        &[Action::Search],
    ),
    bind(
        "Ctrl+f",
        "Filter this pane (fuzzy or regex)",
        &[Action::FilterRegex],
    ),
    bind(
        "Ctrl+g",
        "Find in files (recursive grep)",
        &[Action::FindInFiles],
    ),
    bind("Y", "Copy to other pane", &[Action::Copy]),
    bind("M", "Move to other pane", &[Action::Move]),
    bind(
        "dd / Del",
        "Move to trash",
        &[Action::Delete, Action::DeleteChord],
    ),
    bind("a", "Create file/dir (/ = dir)", &[Action::Create]),
    bind(
        "Enter",
        "Open directory / edit file in $EDITOR",
        &[Action::OpenEntry],
    ),
    bind("Backspace", "Parent directory", &[Action::ParentDir]),
    bind(
        "Tab, h, l",
        "Switch panes",
        &[Action::PaneToggle, Action::PaneLeft, Action::PaneRight],
    ),
    bind(
        "j, k, Up, Down",
        "Move cursor",
        &[Action::MoveDown, Action::MoveUp],
    ),
    bind(
        "g / G",
        "First / last entry",
        &[Action::GotoFirst, Action::GotoLast],
    ),
    bind("x", "Toggle select file", &[Action::ToggleSelect]),
    bind("Ctrl+a", "Select all entries", &[Action::SelectAll]),
    bind("*", "Select by wildcard", &[Action::SelectGlob]),
    bind(
        "y / p / P",
        "Yank / paste copy / paste move",
        &[Action::Yank, Action::Paste, Action::PasteMove],
    ),
    bind("S", "Compute directory sizes", &[Action::DirSizes]),
    bind(
        ":",
        "Command palette (Tab completes)",
        &[Action::CommandPalette],
    ),
    bind(
        ":!cmd / :term cmd",
        "Run: capture output / attach terminal",
        &[],
    ),
    bind("Space", "Preview (view file)", &[Action::Preview]),
    bind("Ctrl+h", "Toggle hidden files", &[Action::ToggleHidden]),
    bind("Ctrl+l", "Refresh panes / redraw", &[Action::Refresh]),
    bind(
        "Shift+Left/Right",
        "Change sort column",
        &[Action::SortPrev, Action::SortNext],
    ),
    bind("Shift+O", "Reverse sort order", &[Action::SortReverse]),
    bind("Ctrl+j/k or Ctrl+arrows", "Scroll preview", &[]),
    bind("Ctrl+f/b", "Preview: page down/up", &[]),
    bind("Ctrl+d/u", "Preview: half page down/up", &[]),
    bind("w", "Preview: toggle line wrap", &[]),
    bind(
        "r / D / x (trash)",
        "Restore / delete permanently / select",
        &[],
    ),
    bind("Esc", "Close / clear filter / clear selection", &[]),
    bind("q", "Quit", &[Action::Quit]),
];

/// The former About popup, now a line on the bottom border of this popup:
/// one key fewer to remember, and the version is where people already look.
fn about_line() -> String {
    format!(
        " {} v{}  ·  {} ",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_REPOSITORY"),
    )
}

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

/// The command list, rendered from the shared table so the help popup cannot
/// drift from what the palette actually accepts.
fn command_entries() -> Vec<(String, &'static str)> {
    command::COMMANDS
        .iter()
        .map(|spec| {
            let mut names = spec.display_names();
            if !spec.args.is_empty() {
                names.push(' ');
                names.push_str(spec.args);
            }
            (names, spec.description)
        })
        .collect()
}

/// Width of the key column: the longest key in either list, plus a space.
fn key_column(commands: &[(String, &'static str)]) -> usize {
    KEYBINDS
        .iter()
        .map(|bind| bind.keys.len())
        .chain(commands.iter().map(|(names, _)| names.len()))
        .max()
        .unwrap_or_default()
        + 1
}

/// Every line of the help text: both sections with their headings.
fn all_lines(theme: &Theme) -> Vec<Line<'static>> {
    let commands = command_entries();
    let key_column = key_column(&commands);
    let heading = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(theme.colors.highlight()),
        ))
    };
    let entry = |key: &str, description: &str| {
        Line::from(vec![
            Span::from(format!("{key:<key_column$}")).style(theme.colors.primary()),
            Span::from(description.to_string()),
        ])
    };

    let mut lines = vec![heading("Keybindings")];
    lines.extend(
        KEYBINDS
            .iter()
            .map(|bind| entry(bind.keys, bind.description)),
    );
    lines.push(Line::from(""));
    lines.push(heading("Commands  (Tab completes, Shift+Tab goes back)"));
    lines.extend(
        commands
            .iter()
            .map(|(names, description)| entry(names, description)),
    );
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

        // The about line lives on the bottom border, so it costs no rows —
        // but the popup still has to be wide enough to show it.
        let about = Line::from(about_line()).centered();
        let want_width =
            (columns * line_width + (columns - 1) * COLUMN_GAP + 4).max(about.width() as u16 + 2);
        let popup_area = centered_popup(
            area,
            (want_width, rows + 2),
            (40, 8),
            (MAX_WIDTH, area.height),
        );

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title("Help  (? / :help)")
            .title_bottom(about.style(Style::default().fg(theme.colors.muted())))
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
        let commands = command_entries();
        let longest = KEYBINDS
            .iter()
            .map(|bind| bind.keys.len())
            .chain(commands.iter().map(|(names, _)| names.len()))
            .max()
            .unwrap();
        assert!(key_column(&commands) > longest);
    }

    #[test]
    fn every_binding_has_a_description() {
        assert!(
            KEYBINDS
                .iter()
                .all(|bind| !bind.keys.is_empty() && !bind.description.is_empty())
        );
    }

    /// A feature nobody can find is as good as missing, so every action in the
    /// keymap has to show up in this popup.
    #[test]
    fn every_action_is_documented() {
        let missing: Vec<&str> = Action::ALL
            .iter()
            .filter(|action| !KEYBINDS.iter().any(|bind| bind.actions.contains(action)))
            .map(|action| action.name())
            .collect();

        assert!(missing.is_empty(), "undocumented actions: {missing:?}");
    }

    #[test]
    fn commands_come_from_the_shared_table() {
        let entries = command_entries();
        assert_eq!(entries.len(), command::COMMANDS.len());
        assert!(entries.iter().any(|(names, _)| names.starts_with(":q /")));
    }
}
