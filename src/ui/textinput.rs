//! A single-line text field with a cursor, shared by dialogs, the command bar
//! and the search bars.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key press did to a [`TextInput`].
///
/// Callers need the distinction: work derived from the *text* (a filter, a
/// rename preview, a completion menu) only has to be redone on [`Self::Changed`],
/// while moving the cursor leaves it valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    /// The text changed, so anything derived from it is stale.
    Changed,
    /// Only the cursor moved; the text is unchanged.
    CursorMoved,
    /// The key means nothing to a text field — the caller should handle it.
    Ignored,
}

/// A single-line text field with a movable cursor.
///
/// The cursor is stored as a *character* index (not byte index) so multi-byte
/// UTF-8 input is handled correctly.
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    fn byte_pos(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.value.len())
    }

    pub fn insert(&mut self, c: char) {
        let b = self.byte_pos(self.cursor);
        self.value.insert(b, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let b = self.byte_pos(self.cursor - 1);
        self.value.remove(b);
        self.cursor -= 1;
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if self.cursor < self.value.chars().count() {
            self.cursor += 1;
        }
    }

    /// Applies one key press, reporting what it did.
    ///
    /// Every caller used to write this match out itself — six copies of the
    /// same four arms across `input.rs` and `dialog.rs` — which is how
    /// `Backspace` ended up subtly different in each of them. Keys this field
    /// has no meaning for come back as [`TextEdit::Ignored`] so the caller can
    /// deal with them.
    pub fn handle_key(&mut self, key: &KeyEvent) -> TextEdit {
        match key.code {
            KeyCode::Backspace => {
                self.backspace();
                TextEdit::Changed
            }
            KeyCode::Left => {
                self.left();
                TextEdit::CursorMoved
            }
            KeyCode::Right => {
                self.right();
                TextEdit::CursorMoved
            }
            // Ctrl/Alt combos are never literal text. Shift is, for capitals.
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert(c);
                TextEdit::Changed
            }
            _ => TextEdit::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn handle_key_reports_text_changes() {
        let mut input = TextInput::default();

        assert_eq!(
            input.handle_key(&key(KeyCode::Char('a'))),
            TextEdit::Changed
        );
        assert_eq!(
            input.handle_key(&key(KeyCode::Backspace)),
            TextEdit::Changed
        );
        assert_eq!(input.value, "");
    }

    #[test]
    fn handle_key_separates_cursor_moves_from_edits() {
        // Callers rebuild filters and previews only on Changed, so a cursor
        // move must not be reported as one.
        let mut input = TextInput::new("ab");

        assert_eq!(input.handle_key(&key(KeyCode::Left)), TextEdit::CursorMoved);
        assert_eq!(
            input.handle_key(&key(KeyCode::Right)),
            TextEdit::CursorMoved
        );
        assert_eq!(input.value, "ab");
    }

    #[test]
    fn handle_key_accepts_shifted_characters() {
        let mut input = TextInput::default();
        let shift_a = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);

        assert_eq!(input.handle_key(&shift_a), TextEdit::Changed);
        assert_eq!(input.value, "A");
    }

    #[test]
    fn handle_key_ignores_control_combos_and_unknown_keys() {
        // These belong to the popup around the field — Ctrl+n moves a list
        // selection, it must never be typed into the box.
        let mut input = TextInput::default();

        for event in [
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
            key(KeyCode::Up),
            key(KeyCode::Enter),
            key(KeyCode::Esc),
        ] {
            assert_eq!(input.handle_key(&event), TextEdit::Ignored, "{event:?}");
        }

        assert_eq!(input.value, "");
    }

    #[test]
    fn new_places_cursor_at_end() {
        let input = TextInput::new("abc");
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn insert_appends_at_cursor() {
        let mut input = TextInput::default();
        input.insert('a');
        input.insert('b');
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn insert_in_middle_after_moving_left() {
        let mut input = TextInput::new("ac");
        input.left();
        input.insert('b');
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn backspace_deletes_before_cursor() {
        let mut input = TextInput::new("abc");
        input.left();
        input.backspace();
        assert_eq!(input.value, "ac");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut input = TextInput::new("a");
        input.left();
        input.backspace();
        assert_eq!(input.value, "a");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn left_and_right_are_bounded() {
        let mut input = TextInput::new("ab");
        input.right(); // already at end — no-op
        assert_eq!(input.cursor, 2);
        input.left();
        input.left();
        input.left(); // beyond start — stays at 0
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn handles_multibyte_characters() {
        let mut input = TextInput::new("日本");
        assert_eq!(input.cursor, 2);
        input.left();
        input.backspace();
        assert_eq!(input.value, "本");
        assert_eq!(input.cursor, 0);
        input.insert('é');
        assert_eq!(input.value, "é本");
        assert_eq!(input.cursor, 1);
    }
}
