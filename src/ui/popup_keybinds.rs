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
    keymap::{Action, Keymap},
    theme::Theme,
};

/// One piece of a row's key column.
enum KeyPart {
    /// Fixed text for a key the keymap does not own — preview scrolling,
    /// `Esc`, the `:!` / `:term` pair.
    Literal(&'static str),
    /// One action; the row shows whatever keys are actually bound to it.
    Action(Action),
    /// Fixed text that still documents the given actions, for the two rows the
    /// live form would misrepresent: `dd` is a double press, and the cursor
    /// pair reads better interleaved than as each action's keys in sequence.
    /// The actions are only read by the coverage test below.
    Curated(
        &'static str,
        #[cfg_attr(not(test), allow(dead_code))] &'static [Action],
    ),
}

/// One row of the keybinding table.
///
/// `parts` decides what the key column shows: `Action` pieces are resolved
/// against the active keymap, everything else is fixed text, so a rebind
/// reaches this popup exactly as it already reaches the footer.
struct Keybind {
    parts: &'static [KeyPart],
    /// Text between parts, e.g. `" / "` for paired actions, `", "` for keys
    /// that belong to the same job.
    separator: &'static str,
    description: &'static str,
}

/// Shorthand for a table row.
const fn bind(
    parts: &'static [KeyPart],
    separator: &'static str,
    description: &'static str,
) -> Keybind {
    Keybind {
        parts,
        separator,
        description,
    }
}

/// Fixed text: keys the keymap does not own.
const fn lit(text: &'static str) -> KeyPart {
    KeyPart::Literal(text)
}

/// One action, shown with its live bindings.
const fn key(action: Action) -> KeyPart {
    KeyPart::Action(action)
}

/// Fixed text that still documents the given actions.
const fn curated(text: &'static str, actions: &'static [Action]) -> KeyPart {
    KeyPart::Curated(text, actions)
}

const KEYBINDS: &[Keybind] = &[
    bind(&[key(Action::Help)], ", ", "This help"),
    bind(&[key(Action::Rename)], ", ", "Rename"),
    bind(
        &[key(Action::BulkRename)],
        ", ",
        "Bulk rename (2+ selected)",
    ),
    bind(
        &[key(Action::BookmarkToggle), key(Action::Bookmarks)],
        " / ",
        "Bookmark entry / list bookmarks",
    ),
    bind(
        &[key(Action::Search)],
        ", ",
        "Find files by name (fuzzy or regex)",
    ),
    bind(
        &[key(Action::FilterRegex)],
        ", ",
        "Filter this pane (fuzzy or regex)",
    ),
    bind(
        &[key(Action::FindInFiles)],
        ", ",
        "Find in files (recursive grep)",
    ),
    bind(&[key(Action::Copy)], ", ", "Copy to other pane"),
    bind(&[key(Action::Move)], ", ", "Move to other pane"),
    bind(&[key(Action::CreateSymlink)], ", ", "Symlink to other pane"),
    bind(
        &[key(Action::ArchiveCreate)],
        ", ",
        "Create archive (zip/tar.gz) from selection",
    ),
    bind(
        &[key(Action::Permissions)],
        ", ",
        "Permissions/ownership (chmod/chown)",
    ),
    bind(
        &[curated("dd", &[Action::DeleteChord]), key(Action::Delete)],
        " / ",
        "Move to trash",
    ),
    bind(&[key(Action::Create)], ", ", "Create file/dir (/ = dir)"),
    bind(
        &[key(Action::OpenEntry)],
        ", ",
        "Open directory / edit file in $EDITOR",
    ),
    bind(&[key(Action::ParentDir)], ", ", "Parent directory"),
    bind(&[key(Action::ToggleTree)], ", ", "Tree view on/off"),
    bind(
        &[key(Action::TreeExpand), key(Action::TreeCollapse)],
        " / ",
        "Tree: open/close a directory",
    ),
    bind(
        &[
            key(Action::PaneToggle),
            key(Action::PaneLeft),
            key(Action::PaneRight),
        ],
        ", ",
        "Switch panes",
    ),
    bind(
        &[curated(
            "j, k, Up, Down",
            &[Action::MoveDown, Action::MoveUp],
        )],
        ", ",
        "Move cursor",
    ),
    bind(
        &[key(Action::GotoFirst), key(Action::GotoLast)],
        " / ",
        "First / last entry",
    ),
    bind(&[key(Action::ToggleSelect)], ", ", "Toggle select file"),
    bind(&[key(Action::SelectAll)], ", ", "Select all entries"),
    bind(&[key(Action::SelectGlob)], ", ", "Select by wildcard"),
    bind(
        &[
            key(Action::Yank),
            key(Action::Paste),
            key(Action::PasteMove),
        ],
        " / ",
        "Yank / paste copy / paste move",
    ),
    bind(&[key(Action::DirSizes)], ", ", "Compute directory sizes"),
    bind(
        &[key(Action::CommandPalette)],
        ", ",
        "Command palette (Tab completes)",
    ),
    bind(
        &[lit(":!cmd / :term cmd")],
        ", ",
        "Run: capture output / attach terminal",
    ),
    bind(&[key(Action::Preview)], ", ", "Preview (view file)"),
    bind(&[key(Action::ToggleHidden)], ", ", "Toggle hidden files"),
    bind(&[key(Action::Refresh)], ", ", "Refresh panes / redraw"),
    bind(
        &[key(Action::SortPrev), key(Action::SortNext)],
        "/",
        "Change sort column",
    ),
    bind(&[key(Action::SortReverse)], ", ", "Reverse sort order"),
    bind(&[lit("Ctrl+j/k or Ctrl+arrows")], ", ", "Scroll preview"),
    bind(&[lit("Ctrl+f/b")], ", ", "Preview: page down/up"),
    bind(&[lit("Ctrl+d/u")], ", ", "Preview: half page down/up"),
    bind(&[lit("w")], ", ", "Preview: toggle line wrap"),
    bind(
        &[lit("r / D / x (trash)")],
        ", ",
        "Restore / delete permanently / select",
    ),
    bind(
        &[lit("Enter / 1-9 / d / P (bookmarks)")],
        ", ",
        "Jump / jump to nth / remove / prune missing",
    ),
    bind(
        &[lit("Esc")],
        ", ",
        "Close / clear filter / clear selection",
    ),
    bind(&[key(Action::Quit)], ", ", "Quit"),
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

#[derive(Debug)]
pub struct PopupKeybinds<'a> {
    keymap: &'a Keymap,
}

impl<'a> PopupKeybinds<'a> {
    pub fn new(keymap: &'a Keymap) -> Self {
        Self { keymap }
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

/// What a row should show in the key column, with the active bindings folded
/// in. An action left with no key at all shows `—` rather than vanishing.
fn row_keys(row: &Keybind, keymap: &Keymap) -> String {
    row.parts
        .iter()
        .map(|part| match part {
            KeyPart::Literal(text) | KeyPart::Curated(text, _) => (*text).to_string(),
            KeyPart::Action(action) => {
                let labels = keymap.labels_for(*action);
                if labels.is_empty() {
                    "—".to_string()
                } else {
                    labels.join(", ")
                }
            }
        })
        .collect::<Vec<_>>()
        .join(row.separator)
}

/// Width of the key column: the longest key in either list, plus a space.
fn key_column<'a>(keys: impl Iterator<Item = &'a str>) -> usize {
    keys.map(str::len).max().unwrap_or_default() + 1
}

/// Every line of the help text: both sections with their headings.
fn all_lines(theme: &Theme, keymap: &Keymap) -> Vec<Line<'static>> {
    let commands = command_entries();
    let rows: Vec<(String, &'static str)> = KEYBINDS
        .iter()
        .map(|row| (row_keys(row, keymap), row.description))
        .collect();
    let key_column = key_column(
        rows.iter()
            .map(|(keys, _)| keys.as_str())
            .chain(commands.iter().map(|(names, _)| names.as_str())),
    );
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
        rows.iter()
            .map(|(keys, description)| entry(keys, description)),
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

impl Component for PopupKeybinds<'_> {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let lines = all_lines(theme, self.keymap);
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
    use crate::{
        config::Config,
        ui::keymap::{build_keymap, default_keymap},
    };

    fn keymap_with(bindings: &[(&str, &str)]) -> Keymap {
        let mut config = Config::default();
        for (key, value) in bindings {
            config
                .keybindings
                .insert(key.to_string(), value.to_string());
        }
        build_keymap(&config)
    }

    fn row_for(description: &str) -> &'static Keybind {
        KEYBINDS
            .iter()
            .find(|row| row.description == description)
            .expect("the row is documented")
    }

    #[test]
    fn key_column_fits_the_longest_key() {
        let keymap = default_keymap();
        let rows: Vec<String> = KEYBINDS.iter().map(|row| row_keys(row, &keymap)).collect();
        let commands = command_entries();
        let longest = rows
            .iter()
            .map(|keys| keys.len())
            .chain(commands.iter().map(|(names, _)| names.len()))
            .max()
            .unwrap();
        let width = key_column(
            rows.iter()
                .map(String::as_str)
                .chain(commands.iter().map(|(names, _)| names.as_str())),
        );
        assert!(width > longest);
    }

    #[test]
    fn every_binding_has_a_description() {
        assert!(
            KEYBINDS
                .iter()
                .all(|bind| !bind.parts.is_empty() && !bind.description.is_empty())
        );
    }

    #[test]
    fn commands_come_from_the_shared_table() {
        let entries = command_entries();
        assert_eq!(entries.len(), command::COMMANDS.len());
        assert!(entries.iter().any(|(names, _)| names.starts_with(":q /")));
    }

    /// A feature nobody can find is as good as missing, so every action in the
    /// keymap has to show up in this popup.
    #[test]
    fn every_action_is_documented() {
        let mut documented = Vec::new();
        for row in KEYBINDS {
            for part in row.parts {
                match part {
                    KeyPart::Action(action) => documented.push(*action),
                    KeyPart::Curated(_, actions) => documented.extend_from_slice(actions),
                    KeyPart::Literal(_) => {}
                }
            }
        }

        let missing: Vec<&str> = Action::ALL
            .iter()
            .filter(|action| !documented.contains(action))
            .map(|action| action.name())
            .collect();

        assert!(missing.is_empty(), "undocumented actions: {missing:?}");
    }

    #[test]
    fn a_rebound_key_replaces_the_default_in_the_help_text() {
        // The config the classic-shortcut issue asked for: copy on F5.
        let keymap = keymap_with(&[("f5", "copy")]);
        assert_eq!(row_keys(row_for("Copy to other pane"), &keymap), "F5");
        // A row the user left alone still reads as its default.
        assert_eq!(row_keys(row_for("Move to other pane"), &keymap), "M");
    }

    #[test]
    fn the_default_help_text_uses_the_built_in_keys() {
        let keymap = default_keymap();
        assert_eq!(row_keys(row_for("Copy to other pane"), &keymap), "Y");
        assert_eq!(row_keys(row_for("Move cursor"), &keymap), "j, k, Up, Down");
    }

    #[test]
    fn a_freed_action_is_still_documented() {
        let keymap = keymap_with(&[("q", "none")]);
        assert_eq!(row_keys(row_for("Quit"), &keymap), "—");
    }
}
