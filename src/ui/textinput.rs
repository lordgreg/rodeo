//! A single-line text field with a cursor, shared by dialogs, the command bar
//! and the search bars.

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
