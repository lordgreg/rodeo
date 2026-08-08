//! Permissions/ownership popup: chmod (an octal field kept in sync with a
//! 3×3 rwx toggle grid) and chown (owner/group, by name or numeric id).
//!
//! Applying one set of values to every target — seeded from the first when
//! the popup opens — mirrors how a multi-select permissions dialog works in
//! most GUI file managers: simpler than an indeterminate/mixed-value state,
//! and the common case (select several files, set the same mode) needs no
//! per-file UI at all.

use std::path::PathBuf;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use crate::fs::passwd;
use crate::ui::{
    component::{Component, centered_popup, content_size},
    textinput::TextInput,
    theme::Theme,
};

/// Which of the three fields is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Mode,
    Owner,
    Group,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Mode => Self::Owner,
            Self::Owner => Self::Group,
            Self::Group => Self::Mode,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Mode => Self::Group,
            Self::Owner => Self::Mode,
            Self::Group => Self::Owner,
        }
    }
}

#[derive(Debug)]
pub struct PermissionsEditor {
    pub targets: Vec<PathBuf>,
    /// The 3 octal digits, kept as text so it can render directly; the rwx
    /// grid (`bits`) is the field actually edited, `mode.value` is derived
    /// from it after every change (see [`Self::sync_mode_text`]).
    pub mode: TextInput,
    pub owner: TextInput,
    pub group: TextInput,
    pub focus: Field,
    /// `bits[row][col]`: row 0/1/2 = owner/group/other, col 0/1/2 = r/w/x.
    pub bits: [[bool; 3]; 3],
    /// Grid cursor, meaningful while `focus == Field::Mode`.
    pub row: usize,
    pub col: usize,
    pub error: Option<String>,
}

impl PermissionsEditor {
    pub fn new(targets: Vec<PathBuf>, mode: u32, uid: u32, gid: u32) -> Self {
        let bits = bits_from_mode(mode);
        let owner = passwd::user_name(uid).unwrap_or_else(|| uid.to_string());
        let group = passwd::group_name(gid).unwrap_or_else(|| gid.to_string());

        let mut editor = Self {
            targets,
            mode: TextInput::default(),
            owner: TextInput::new(owner),
            group: TextInput::new(group),
            focus: Field::Mode,
            bits,
            row: 0,
            col: 0,
            error: None,
        };
        editor.sync_mode_text();
        editor
    }

    fn sync_mode_text(&mut self) {
        self.mode = TextInput::new(mode_string(self.bits));
    }

    pub fn next_field(&mut self) {
        self.focus = self.focus.next();
    }

    pub fn prev_field(&mut self) {
        self.focus = self.focus.prev();
    }

    /// Moves the rwx grid cursor. No-op outside the [`Field::Mode`] field.
    pub fn move_cursor(&mut self, drow: isize, dcol: isize) {
        if self.focus != Field::Mode {
            return;
        }
        self.row = ((self.row as isize + drow).rem_euclid(3)) as usize;
        self.col = ((self.col as isize + dcol).rem_euclid(3)) as usize;
    }

    /// Flips the bit under the grid cursor.
    pub fn toggle_bit(&mut self) {
        if self.focus != Field::Mode {
            return;
        }
        self.bits[self.row][self.col] = !self.bits[self.row][self.col];
        self.sync_mode_text();
    }

    /// Types one octal digit into the current grid row (owner/group/other),
    /// then advances to the next row — the same left-to-right order as
    /// typing "755" by hand.
    pub fn type_digit(&mut self, digit: u8) {
        if self.focus != Field::Mode || digit > 7 {
            return;
        }
        self.bits[self.row] = [digit & 0o4 != 0, digit & 0o2 != 0, digit & 0o1 != 0];
        self.row = (self.row + 1).min(2);
        self.sync_mode_text();
    }

    /// Steps the grid cursor back a row, undoing [`Self::type_digit`]'s
    /// advance. Values are always valid octal digits, so there is nothing to
    /// erase — only where the next digit lands.
    pub fn backspace_mode(&mut self) {
        if self.focus == Field::Mode {
            self.row = self.row.saturating_sub(1);
        }
    }

    pub fn resolved_mode(&self) -> u32 {
        mode_value(self.bits)
    }

    /// `Ok(None)` means "leave the owner unchanged" (the field is blank).
    pub fn resolved_owner(&self) -> Result<Option<u32>, String> {
        resolve_id(&self.owner.value, "user", passwd::name_to_uid)
    }

    /// `Ok(None)` means "leave the group unchanged" (the field is blank).
    pub fn resolved_group(&self) -> Result<Option<u32>, String> {
        resolve_id(&self.group.value, "group", passwd::name_to_gid)
    }
}

fn resolve_id(
    text: &str,
    kind: &str,
    lookup: impl Fn(&str) -> Option<u32>,
) -> Result<Option<u32>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Ok(id) = text.parse::<u32>() {
        return Ok(Some(id));
    }
    lookup(text)
        .map(Some)
        .ok_or_else(|| format!("unknown {kind} '{text}'"))
}

fn bits_from_mode(mode: u32) -> [[bool; 3]; 3] {
    let digit = |shift: u32| (mode >> shift) & 0o7;
    let row = |d: u32| [d & 0o4 != 0, d & 0o2 != 0, d & 0o1 != 0];
    [row(digit(6)), row(digit(3)), row(digit(0))]
}

fn mode_value(bits: [[bool; 3]; 3]) -> u32 {
    bits.iter().fold(0u32, |acc, row| {
        let d = (row[0] as u32) << 2 | (row[1] as u32) << 1 | row[2] as u32;
        (acc << 3) | d
    })
}

fn mode_string(bits: [[bool; 3]; 3]) -> String {
    format!("{:03o}", mode_value(bits))
}

impl Component for PermissionsEditor {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let cell = |row: usize, col: usize| -> Span<'static> {
            let ch = ['r', 'w', 'x'][col];
            let set = self.bits[row][col];
            let label = if set { ch } else { '-' };
            let mut style = if set {
                Style::new().fg(theme.colors.success())
            } else {
                Style::new().fg(theme.colors.muted())
            };
            if self.focus == Field::Mode && self.row == row && self.col == col {
                style = style.bg(theme.colors.surface()).bold();
            }
            Span::styled(label.to_string(), style)
        };
        let triad_line = |label: &'static str, row: usize| -> Line<'static> {
            Line::from(vec![
                Span::from(format!("{label:<7}")),
                cell(row, 0),
                Span::from(" "),
                cell(row, 1),
                Span::from(" "),
                cell(row, 2),
            ])
        };

        let field_style = |focused: bool| {
            if focused {
                Style::new().fg(theme.colors.highlight())
            } else {
                Style::new().fg(theme.colors.primary())
            }
        };

        let mut lines = vec![
            Line::from(vec![
                Span::from("        "),
                Span::styled("r", Style::new().fg(theme.colors.muted())),
                Span::from(" "),
                Span::styled("w", Style::new().fg(theme.colors.muted())),
                Span::from(" "),
                Span::styled("x", Style::new().fg(theme.colors.muted())),
            ]),
            triad_line("Owner", 0),
            triad_line("Group", 1),
            triad_line("Other", 2),
            Line::from(format!("Mode:   {}", self.mode.value)),
            Line::from(""),
            Line::from(vec![
                Span::from("Owner:  "),
                Span::styled(
                    self.owner.value.clone(),
                    field_style(self.focus == Field::Owner),
                ),
            ]),
            Line::from(vec![
                Span::from("Group:  "),
                Span::styled(
                    self.group.value.clone(),
                    field_style(self.focus == Field::Group),
                ),
            ]),
        ];

        if let Some(error) = &self.error {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("⚠ {error}"),
                Style::new().fg(theme.colors.error()),
            )));
        }

        let longest = lines.iter().map(|l| l.width() as u16).max().unwrap_or(20);
        let popup_area = centered_popup(
            area,
            content_size(longest.max(30), lines.len()),
            (34, lines.len() as u16 + 2),
            (60, area.height.saturating_sub(2)),
        );

        frame.render_widget(Clear, popup_area);

        let count = self.targets.len();
        let title = format!(
            " Permissions — {count} file{} • Tab=field  Space=toggle  Enter=apply  Esc=cancel ",
            if count == 1 { "" } else { "s" }
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            popup_area,
        );

        // Cursor only makes sense on a text field; the grid shows focus via
        // the highlighted cell instead.
        let field_row = match self.focus {
            Field::Owner => Some((6u16, &self.owner)),
            Field::Group => Some((7u16, &self.group)),
            Field::Mode => None,
        };
        if let Some((line, field)) = field_row {
            let cursor_x = popup_area.x + 2 + 8 + field.cursor as u16;
            let cursor_y = popup_area.y + 1 + line;
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(mode: u32) -> PermissionsEditor {
        PermissionsEditor::new(vec![PathBuf::from("/tmp/a")], mode, 1000, 1000)
    }

    #[test]
    fn new_seeds_the_grid_and_mode_text_from_the_given_mode() {
        let e = editor(0o754);
        assert_eq!(e.mode.value, "754");
        assert_eq!(
            e.bits,
            [
                [true, true, true],
                [true, false, true],
                [true, false, false]
            ]
        );
    }

    #[test]
    fn toggling_a_bit_updates_the_mode_text() {
        let mut e = editor(0o644);
        e.row = 0;
        e.col = 2; // owner execute
        e.toggle_bit();
        assert_eq!(e.mode.value, "744");
    }

    #[test]
    fn typing_a_digit_sets_the_whole_row_and_advances() {
        let mut e = editor(0o000);
        e.type_digit(7);
        assert_eq!(e.row, 1);
        e.type_digit(5);
        assert_eq!(e.row, 2);
        e.type_digit(5);
        assert_eq!(e.mode.value, "755");
    }

    #[test]
    fn typing_past_the_last_row_keeps_overwriting_it() {
        let mut e = editor(0o000);
        e.type_digit(7);
        e.type_digit(5);
        e.type_digit(5);
        e.type_digit(0); // still row 2 — overwrites "other"
        assert_eq!(e.mode.value, "750");
    }

    #[test]
    fn backspace_steps_the_cursor_back_without_changing_bits() {
        let mut e = editor(0o000);
        e.type_digit(7);
        e.backspace_mode();
        assert_eq!(e.row, 0);
        e.type_digit(1);
        assert_eq!(e.mode.value, "100");
    }

    #[test]
    fn cursor_moves_wrap_within_the_grid() {
        let mut e = editor(0o000);
        e.move_cursor(0, -1);
        assert_eq!((e.row, e.col), (0, 2), "left from col 0 wraps to col 2");
        e.move_cursor(-1, 0);
        assert_eq!((e.row, e.col), (2, 2), "up from row 0 wraps to row 2");
    }

    #[test]
    fn grid_interactions_are_ignored_outside_the_mode_field() {
        let mut e = editor(0o644);
        e.focus = Field::Owner;
        e.toggle_bit();
        e.type_digit(7);
        assert_eq!(e.mode.value, "644", "owner focus must not touch the grid");
    }

    #[test]
    fn resolved_mode_matches_the_seeded_value() {
        assert_eq!(editor(0o755).resolved_mode(), 0o755);
        assert_eq!(editor(0o600).resolved_mode(), 0o600);
    }

    #[test]
    fn a_blank_owner_or_group_field_means_leave_it_unchanged() {
        let mut e = editor(0o644);
        e.owner = TextInput::new("");
        e.group = TextInput::new("  ");
        assert_eq!(e.resolved_owner(), Ok(None));
        assert_eq!(e.resolved_group(), Ok(None));
    }

    #[test]
    fn a_numeric_owner_or_group_is_used_directly() {
        let mut e = editor(0o644);
        e.owner = TextInput::new("1234");
        e.group = TextInput::new("5678");
        assert_eq!(e.resolved_owner(), Ok(Some(1234)));
        assert_eq!(e.resolved_group(), Ok(Some(5678)));
    }

    #[test]
    fn an_unresolvable_name_is_reported() {
        let mut e = editor(0o644);
        e.owner = TextInput::new("definitely-not-a-real-user");
        assert_eq!(
            e.resolved_owner(),
            Err("unknown user 'definitely-not-a-real-user'".to_string())
        );
    }

    #[test]
    fn field_focus_cycles_forward_and_back() {
        assert_eq!(Field::Mode.next(), Field::Owner);
        assert_eq!(Field::Owner.next(), Field::Group);
        assert_eq!(Field::Group.next(), Field::Mode);
        assert_eq!(Field::Mode.prev(), Field::Group);
    }
}
