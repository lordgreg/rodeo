//! Configurable keybindings.
//!
//! Single-key actions are table-driven: [`default_keymap`] holds the defaults
//! and [`build_keymap`] merges the user's `[keybindings]` overrides on top.

use crossterm::event::KeyCode;

use crate::config::Config;

/// Every user action triggerable by a single (unmodified) key in normal mode.
/// Chords (`dd`), Ctrl/Shift combos, and Esc are not configurable yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    OpenEntry,
    ParentDir,
    Mkdir,
    GotoFirst,
    GotoLast,
    ToggleSelect,
    SelectGlob,
    DirSizes,
    Quit,
    PaneLeft,
    PaneRight,
    PaneToggle,
    About,
    Help,
    Preview,
    Search,
    CommandPalette,
    Rename,
    Create,
    Yank,
    Paste,
    PasteMove,
    DeleteChord,
    Copy,
    Move,
    Delete,
    MoveDown,
    MoveUp,
    BulkRename,
}

impl Action {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bulk_rename" => Self::BulkRename,
            "open" => Self::OpenEntry,
            "parent" => Self::ParentDir,
            "mkdir" => Self::Mkdir,
            "first" => Self::GotoFirst,
            "last" => Self::GotoLast,
            "select" => Self::ToggleSelect,
            "glob" => Self::SelectGlob,
            "sizes" => Self::DirSizes,
            "quit" => Self::Quit,
            "left" => Self::PaneLeft,
            "right" => Self::PaneRight,
            "switch" => Self::PaneToggle,
            "about" => Self::About,
            "help" => Self::Help,
            "preview" => Self::Preview,
            "search" => Self::Search,
            "palette" => Self::CommandPalette,
            "rename" => Self::Rename,
            "create" => Self::Create,
            "yank" => Self::Yank,
            "paste" => Self::Paste,
            "paste_move" => Self::PasteMove,
            "delete_chord" => Self::DeleteChord,
            "copy" => Self::Copy,
            "move" => Self::Move,
            "delete" => Self::Delete,
            "down" => Self::MoveDown,
            "up" => Self::MoveUp,
            _ => return None,
        })
    }
}

/// Parses a key name from the config: either a single character (`q`, `P`,
/// `/`, `:`) or a named key (`space`, `tab`, `enter`, `backspace`, `delete`,
/// `up`, `down`, `left`, `right`, `f1`–`f12`).
pub fn parse_key(s: &str) -> Option<KeyCode> {
    let lower = s.to_lowercase();
    match lower.as_str() {
        "space" => return Some(KeyCode::Char(' ')),
        "tab" => return Some(KeyCode::Tab),
        "enter" => return Some(KeyCode::Enter),
        "backspace" => return Some(KeyCode::Backspace),
        "delete" => return Some(KeyCode::Delete),
        "up" => return Some(KeyCode::Up),
        "down" => return Some(KeyCode::Down),
        "left" => return Some(KeyCode::Left),
        "right" => return Some(KeyCode::Right),
        _ => {}
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        return Some(KeyCode::F(n));
    }
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyCode::Char(c)),
        _ => None,
    }
}

/// Hardcoded defaults — the bindings documented in the F1 popup.
pub fn default_keymap() -> Vec<(KeyCode, Action)> {
    vec![
        (KeyCode::Enter, Action::OpenEntry),
        (KeyCode::F(4), Action::OpenEntry),
        (KeyCode::Backspace, Action::ParentDir),
        (KeyCode::F(7), Action::Mkdir),
        (KeyCode::Char('g'), Action::GotoFirst),
        (KeyCode::Char('G'), Action::GotoLast),
        (KeyCode::Char('x'), Action::ToggleSelect),
        (KeyCode::Char('B'), Action::BulkRename),
        (KeyCode::Char('*'), Action::SelectGlob),
        (KeyCode::Char('S'), Action::DirSizes),
        (KeyCode::Char('q'), Action::Quit),
        (KeyCode::F(10), Action::Quit),
        (KeyCode::Char('h'), Action::PaneLeft),
        (KeyCode::Char('l'), Action::PaneRight),
        (KeyCode::Tab, Action::PaneToggle),
        (KeyCode::Char('?'), Action::About),
        (KeyCode::F(1), Action::Help),
        (KeyCode::Char(' '), Action::Preview),
        (KeyCode::Char('/'), Action::Search),
        (KeyCode::F(3), Action::Search),
        (KeyCode::Char(':'), Action::CommandPalette),
        (KeyCode::Char('r'), Action::Rename),
        (KeyCode::F(2), Action::Rename),
        (KeyCode::Char('a'), Action::Create),
        (KeyCode::Char('y'), Action::Yank),
        (KeyCode::Char('p'), Action::Paste),
        (KeyCode::Char('P'), Action::PasteMove),
        (KeyCode::Char('d'), Action::DeleteChord),
        (KeyCode::F(5), Action::Copy),
        (KeyCode::F(6), Action::Move),
        (KeyCode::Delete, Action::Delete),
        (KeyCode::F(8), Action::Delete),
        (KeyCode::Char('j'), Action::MoveDown),
        (KeyCode::Down, Action::MoveDown),
        (KeyCode::Char('k'), Action::MoveUp),
        (KeyCode::Up, Action::MoveUp),
    ]
}

/// Defaults plus user overrides from `config.keybindings` (action name → key
/// name). An override replaces all default keys of that action. Invalid
/// action names or keys are ignored with a warning.
pub fn build_keymap(config: &Config) -> Vec<(KeyCode, Action)> {
    let mut map = default_keymap();

    for (action_name, key_name) in &config.keybindings {
        let Some(action) = Action::from_name(action_name) else {
            log::warn!("keybindings: unknown action '{action_name}'");
            continue;
        };
        let Some(code) = parse_key(key_name) else {
            log::warn!("keybindings: unknown key '{key_name}' for '{action_name}'");
            continue;
        };
        map.retain(|(_, a)| *a != action);
        map.push((code, action));
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_chars() {
        assert_eq!(parse_key("q"), Some(KeyCode::Char('q')));
        assert_eq!(parse_key("P"), Some(KeyCode::Char('P')));
        assert_eq!(parse_key("/"), Some(KeyCode::Char('/')));
        assert_eq!(parse_key(":"), Some(KeyCode::Char(':')));
    }

    #[test]
    fn parses_named_keys() {
        assert_eq!(parse_key("space"), Some(KeyCode::Char(' ')));
        assert_eq!(parse_key("Tab"), Some(KeyCode::Tab));
        assert_eq!(parse_key("ENTER"), Some(KeyCode::Enter));
        assert_eq!(parse_key("backspace"), Some(KeyCode::Backspace));
        assert_eq!(parse_key("delete"), Some(KeyCode::Delete));
        assert_eq!(parse_key("f5"), Some(KeyCode::F(5)));
        assert_eq!(parse_key("F12"), Some(KeyCode::F(12)));
    }

    #[test]
    fn rejects_invalid_keys() {
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("ab"), None);
        assert_eq!(parse_key("f13"), None);
        assert_eq!(parse_key("f0"), None);
    }

    #[test]
    fn override_replaces_default_keys_of_action() {
        let mut config = Config::default();
        config
            .keybindings
            .insert("quit".to_string(), "z".to_string());

        let map = build_keymap(&config);

        let quit_keys: Vec<KeyCode> = map
            .iter()
            .filter(|(_, a)| *a == Action::Quit)
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(quit_keys, vec![KeyCode::Char('z')]);
        // Other actions keep their defaults.
        assert!(map.contains(&(KeyCode::Char('y'), Action::Yank)));
    }

    #[test]
    fn invalid_overrides_are_ignored() {
        let mut config = Config::default();
        config
            .keybindings
            .insert("bogus".to_string(), "z".to_string());
        config
            .keybindings
            .insert("quit".to_string(), "notakey".to_string());

        let map = build_keymap(&config);

        // Defaults fully intact.
        assert_eq!(map.len(), default_keymap().len());
    }
}
