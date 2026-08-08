//! Modal dialogs: confirmations, single-line input and messages.
//!
//! A dialog carries the action to perform when it is confirmed
//! ([`DialogAction`]), so the caller can hand it off and forget about it until
//! a [`DialogResult`] comes back.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use crate::fs::archive::ArchiveKind;
use crate::ui::{
    component::{Component, centered_popup, content_size},
    textinput::TextInput,
    theme::Theme,
};

/// Dialogs never get narrower than this, so a short question still reads as a
/// dialog rather than a sliver.
const MIN_DIALOG_WIDTH: u16 = 34;
/// …nor wider than this, which is plenty for a question or a warning list.
const MAX_DIALOG_WIDTH: u16 = 76;

/// What the dialog was opened for — determines the follow-up action on confirm/submit.
#[derive(Debug)]
pub enum DialogAction {
    Mkdir {
        parent: PathBuf,
    },
    Create {
        parent: PathBuf,
    },
    SelectGlob,
    TouchOverwrite {
        path: PathBuf,
    },
    Rename {
        from: PathBuf,
    },
    RenameOverwrite {
        from: PathBuf,
        to: PathBuf,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    DeletePermanent {
        paths: Vec<PathBuf>,
    },
    Copy {
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
    },
    Move {
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
    },
    PasteMove {
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
    },
    ExtractArchive {
        archive_path: PathBuf,
        kind: ArchiveKind,
        names: BTreeSet<String>,
        dest_dir: PathBuf,
        total: u64,
    },
    /// `(target, link)` pairs, same shape as `Copy`/`Move`'s batch: one
    /// confirm approves overwriting every conflicting name at once.
    CreateSymlink {
        pairs: Vec<(PathBuf, PathBuf)>,
    },
    None,
}

#[derive(Debug)]
pub enum DialogKind {
    Confirm { message: String },
    Input { prompt: String, value: TextInput },
    Message { text: String },
}

#[derive(Debug, PartialEq)]
pub enum DialogResult {
    Confirmed,
    Submitted(String),
    Cancelled,
}

#[derive(Debug)]
pub struct Dialog {
    pub title: String,
    pub kind: DialogKind,
    pub action: DialogAction,
}

impl Dialog {
    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        action: DialogAction,
    ) -> Self {
        Self {
            title: title.into(),
            kind: DialogKind::Confirm {
                message: message.into(),
            },
            action,
        }
    }

    pub fn input(
        title: impl Into<String>,
        prompt: impl Into<String>,
        initial: impl Into<String>,
        action: DialogAction,
    ) -> Self {
        Self {
            title: title.into(),
            kind: DialogKind::Input {
                prompt: prompt.into(),
                value: TextInput::new(initial.into()),
            },
            action,
        }
    }

    pub fn message(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            kind: DialogKind::Message { text: text.into() },
            action: DialogAction::None,
        }
    }

    /// Returns `Some(result)` when the dialog should close, `None` while it stays open.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<DialogResult> {
        match &mut self.kind {
            DialogKind::Confirm { .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    Some(DialogResult::Confirmed)
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    Some(DialogResult::Cancelled)
                }
                _ => None,
            },
            DialogKind::Input { value, .. } => match key.code {
                KeyCode::Enter => Some(DialogResult::Submitted(value.value.clone())),
                KeyCode::Esc => Some(DialogResult::Cancelled),
                _ => {
                    value.handle_key(key);
                    None
                }
            },
            DialogKind::Message { .. } => match key.code {
                KeyCode::Enter | KeyCode::Esc => Some(DialogResult::Cancelled),
                _ => None,
            },
        }
    }
}

impl Component for Dialog {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let content_lines: Vec<Line> = match &self.kind {
            DialogKind::Confirm { message } => vec![
                Line::from(message.as_str()),
                Line::from(""),
                Line::from("[y]es / [n]o"),
            ],
            DialogKind::Input { prompt, value } => vec![
                Line::from(prompt.as_str()),
                Line::from(format!("> {}", value.value)),
            ],
            DialogKind::Message { text } => text.lines().map(Line::from).collect::<Vec<Line>>(),
        };

        // Sized to its content: half the screen cut keybinding warnings and
        // command output off mid-word on a narrow terminal, and wasted two
        // thirds of the box on a short question.
        let longest = content_lines
            .iter()
            .map(|line| line.width() as u16)
            .max()
            .unwrap_or_default();
        let popup_area = centered_popup(
            area,
            content_size(longest, content_lines.len()),
            (MIN_DIALOG_WIDTH, 3),
            (MAX_DIALOG_WIDTH, area.height.saturating_sub(2)),
        );

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .title(self.title.as_str())
            .borders(Borders::ALL)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .bg(theme.colors.surface())
                    .fg(theme.colors.foreground()),
            );

        frame.render_widget(
            Paragraph::new(content_lines)
                .block(block)
                .wrap(Wrap { trim: false })
                .alignment(Alignment::Left),
            popup_area,
        );

        if let DialogKind::Input { value, .. } = &self.kind {
            // +2: border + horizontal padding; +2 more: "> " prompt prefix
            let cursor_x = popup_area.x + 4 + value.cursor as u16;
            let cursor_y = popup_area.y + 2; // border + prompt line
            let max_x = popup_area.x + popup_area.width.saturating_sub(2);
            frame.set_cursor_position((cursor_x.min(max_x), cursor_y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn confirm_dialog() -> Dialog {
        Dialog::confirm("Delete?", "Delete 3 files?", DialogAction::None)
    }

    fn input_dialog() -> Dialog {
        Dialog::input(
            "mkdir",
            "Directory name:",
            "",
            DialogAction::Mkdir {
                parent: PathBuf::from("/tmp"),
            },
        )
    }

    #[test]
    fn confirm_y_returns_confirmed() {
        let mut d = confirm_dialog();
        assert_eq!(
            d.handle_key(&key(KeyCode::Char('y'))),
            Some(DialogResult::Confirmed)
        );
    }

    #[test]
    fn confirm_enter_returns_confirmed() {
        let mut d = confirm_dialog();
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Confirmed)
        );
    }

    #[test]
    fn confirm_n_and_esc_return_cancelled() {
        let mut d = confirm_dialog();
        assert_eq!(
            d.handle_key(&key(KeyCode::Char('n'))),
            Some(DialogResult::Cancelled)
        );
        let mut d = confirm_dialog();
        assert_eq!(
            d.handle_key(&key(KeyCode::Esc)),
            Some(DialogResult::Cancelled)
        );
    }

    #[test]
    fn confirm_other_keys_stay_open() {
        let mut d = confirm_dialog();
        assert_eq!(d.handle_key(&key(KeyCode::Char('x'))), None);
        assert_eq!(d.handle_key(&key(KeyCode::Tab)), None);
    }

    #[test]
    fn input_typing_appends_and_submits() {
        let mut d = input_dialog();
        assert_eq!(d.handle_key(&key(KeyCode::Char('f'))), None);
        assert_eq!(d.handle_key(&key(KeyCode::Char('o'))), None);
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("fo".to_string()))
        );
    }

    #[test]
    fn input_shift_char_appends_uppercase() {
        let mut d = input_dialog();
        let shift_a = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(d.handle_key(&shift_a), None);
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("A".to_string()))
        );
    }

    #[test]
    fn input_backspace_deletes() {
        let mut d = input_dialog();
        d.handle_key(&key(KeyCode::Char('a')));
        d.handle_key(&key(KeyCode::Char('b')));
        d.handle_key(&key(KeyCode::Backspace));
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("a".to_string()))
        );
    }

    #[test]
    fn input_esc_cancels() {
        let mut d = input_dialog();
        assert_eq!(
            d.handle_key(&key(KeyCode::Esc)),
            Some(DialogResult::Cancelled)
        );
    }

    #[test]
    fn input_left_right_moves_cursor_and_edits_mid_text() {
        let mut d = input_dialog();
        d.handle_key(&key(KeyCode::Char('a')));
        d.handle_key(&key(KeyCode::Char('c')));
        d.handle_key(&key(KeyCode::Left));
        d.handle_key(&key(KeyCode::Char('b')));
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("abc".to_string()))
        );
    }

    #[test]
    fn input_backspace_deletes_at_cursor() {
        let mut d = input_dialog();
        d.handle_key(&key(KeyCode::Char('a')));
        d.handle_key(&key(KeyCode::Char('c')));
        d.handle_key(&key(KeyCode::Left));
        d.handle_key(&key(KeyCode::Backspace));
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("c".to_string()))
        );
    }

    #[test]
    fn input_cursor_movement_is_bounded() {
        let mut d = input_dialog();
        d.handle_key(&key(KeyCode::Char('a')));
        d.handle_key(&key(KeyCode::Right)); // already at end
        d.handle_key(&key(KeyCode::Left));
        d.handle_key(&key(KeyCode::Left)); // beyond start
        d.handle_key(&key(KeyCode::Char('x')));
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("xa".to_string()))
        );
    }

    #[test]
    fn input_ctrl_char_is_ignored() {
        let mut d = input_dialog();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(d.handle_key(&ctrl_c), None);
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Submitted("".to_string()))
        );
    }

    #[test]
    fn message_closes_on_enter_and_esc() {
        let mut d = Dialog::message("Error", "something failed");
        assert_eq!(
            d.handle_key(&key(KeyCode::Enter)),
            Some(DialogResult::Cancelled)
        );
        let mut d = Dialog::message("Error", "something failed");
        assert_eq!(
            d.handle_key(&key(KeyCode::Esc)),
            Some(DialogResult::Cancelled)
        );
    }

    #[test]
    fn message_other_keys_stay_open() {
        let mut d = Dialog::message("Error", "something failed");
        assert_eq!(d.handle_key(&key(KeyCode::Char('q'))), None);
    }
}
