//! Configurable keybindings.
//!
//! Every key rodeo reacts to in normal mode lives in one table: plain keys,
//! Ctrl/Shift/Alt combinations, and keys bound to a command line. The defaults
//! are in [`default_keymap`], and `[keybindings]` in the config file overrides
//! them:
//!
//! ```toml
//! [keybindings]
//! "ctrl+f" = "filter"          # an action name
//! "g" = ":term lazygit"        # or a command, run as if typed after `:`
//! "q" = "none"                 # or nothing, to free the key
//! ```
//!
//! Overriding a key that rodeo already uses is allowed but reported, because
//! it is otherwise easy to make a feature unreachable by accident.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;

/// Every user action triggerable by a key in normal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    OpenEntry,
    ParentDir,
    GotoFirst,
    GotoLast,
    ToggleSelect,
    SelectAll,
    SelectGlob,
    DirSizes,
    Quit,
    PaneLeft,
    PaneRight,
    PaneToggle,
    Help,
    Preview,
    Search,
    FilterRegex,
    FindInFiles,
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
    ToggleHidden,
    Refresh,
    SortNext,
    SortPrev,
    SortReverse,
    BulkRename,
}

impl Action {
    /// Name used in the config file.
    pub fn name(self) -> &'static str {
        match self {
            Self::BulkRename => "bulk_rename",
            Self::OpenEntry => "open",
            Self::ParentDir => "parent",
            Self::GotoFirst => "first",
            Self::GotoLast => "last",
            Self::ToggleSelect => "select",
            Self::SelectAll => "select_all",
            Self::SelectGlob => "glob",
            Self::DirSizes => "sizes",
            Self::Quit => "quit",
            Self::PaneLeft => "left",
            Self::PaneRight => "right",
            Self::PaneToggle => "switch",
            Self::Help => "help",
            Self::Preview => "preview",
            Self::Search => "search",
            Self::FilterRegex => "filter",
            Self::FindInFiles => "find",
            Self::CommandPalette => "palette",
            Self::Rename => "rename",
            Self::Create => "create",
            Self::Yank => "yank",
            Self::Paste => "paste",
            Self::PasteMove => "paste_move",
            Self::DeleteChord => "delete_chord",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Delete => "delete",
            Self::MoveDown => "down",
            Self::MoveUp => "up",
            Self::ToggleHidden => "hidden",
            Self::Refresh => "refresh",
            Self::SortNext => "sort_next",
            Self::SortPrev => "sort_prev",
            Self::SortReverse => "sort_reverse",
        }
    }

    /// Every action, so a config name can be resolved and mistakes listed.
    pub const ALL: &'static [Self] = &[
        Self::OpenEntry,
        Self::ParentDir,
        Self::GotoFirst,
        Self::GotoLast,
        Self::ToggleSelect,
        Self::SelectAll,
        Self::SelectGlob,
        Self::DirSizes,
        Self::Quit,
        Self::PaneLeft,
        Self::PaneRight,
        Self::PaneToggle,
        Self::Help,
        Self::Preview,
        Self::Search,
        Self::FilterRegex,
        Self::FindInFiles,
        Self::CommandPalette,
        Self::Rename,
        Self::Create,
        Self::Yank,
        Self::Paste,
        Self::PasteMove,
        Self::DeleteChord,
        Self::Copy,
        Self::Move,
        Self::Delete,
        Self::MoveDown,
        Self::MoveUp,
        Self::ToggleHidden,
        Self::Refresh,
        Self::SortNext,
        Self::SortPrev,
        Self::SortReverse,
        Self::BulkRename,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.name() == name)
    }
}

/// What a key does: a built-in action, or a command line to run as if it had
/// been typed after `:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Action(Action),
    Command(String),
}

impl Binding {
    /// Short description used in conflict reports.
    pub fn describe(&self) -> String {
        match self {
            Self::Action(action) => action.name().to_string(),
            Self::Command(command) => format!(":{command}"),
        }
    }
}

/// A key plus its modifiers, normalised so a binding and a key press compare
/// equal however they were written.
///
/// Shift is folded into the character it produces (`shift+g` and `G` are the
/// same chord), because that is what terminals actually report; for keys with
/// no character, such as the arrows, Shift is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl Chord {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let mut modifiers =
            modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);

        let code = match code {
            KeyCode::Char(c) if modifiers.contains(KeyModifiers::SHIFT) => {
                modifiers -= KeyModifiers::SHIFT;
                c.to_ascii_uppercase()
            }
            KeyCode::Char(c) => c,
            other => {
                return Self {
                    code: other,
                    modifiers,
                };
            }
        };

        Self {
            code: KeyCode::Char(code),
            modifiers,
        }
    }

    pub fn from_event(key: &KeyEvent) -> Self {
        Self::new(key.code, key.modifiers)
    }

    /// How the chord is written in a config file, e.g. `ctrl+f`, `G`, `f5`.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            out.push_str("ctrl+");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            out.push_str("alt+");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            out.push_str("shift+");
        }
        out.push_str(&match self.code {
            KeyCode::Char(' ') => "space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("f{n}"),
            other => format!("{other:?}").to_lowercase(),
        });
        out
    }

    /// How the chord is shown to the user, e.g. `^f`, `Alt+j`, `Space`, `F5`.
    ///
    /// Deliberately lives beside [`Self::describe`], the config-file form. The
    /// footer used to derive this by string-parsing `describe`'s output, which
    /// meant two mapping tables in two files: it only ever stripped `ctrl+`, so
    /// a binding on `alt+j` was advertised as the literal text `alt+j`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            out.push('^');
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            out.push_str("Alt+");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            out.push_str("Shift+");
        }
        out.push_str(&match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Backspace => "Bksp".to_string(),
            KeyCode::Delete => "Del".to_string(),
            // `Debug` already yields `Tab`, `Enter`, `Esc`, `Left`, `PageUp`…
            other => format!("{other:?}"),
        });
        out
    }
}

/// Parses `ctrl+f`, `shift+right`, `space`, `f5`, `G`, `+` …
pub fn parse_chord(text: &str) -> Option<Chord> {
    let mut modifiers = KeyModifiers::NONE;
    let mut rest = text;

    while let Some((head, tail)) = rest.split_once('+') {
        // An empty head means the key itself is `+`.
        if head.is_empty() {
            break;
        }
        match head.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "alt" | "meta" | "option" => modifiers |= KeyModifiers::ALT,
            // Not a modifier, so it must be the key.
            _ => break,
        }
        rest = tail;
    }

    Some(Chord::new(parse_key_code(rest)?, modifiers))
}

/// Parses the key part: a single character or a named key.
fn parse_key_code(text: &str) -> Option<KeyCode> {
    match text.to_lowercase().as_str() {
        "space" => return Some(KeyCode::Char(' ')),
        "tab" => return Some(KeyCode::Tab),
        "enter" | "return" => return Some(KeyCode::Enter),
        "backspace" => return Some(KeyCode::Backspace),
        "delete" | "del" => return Some(KeyCode::Delete),
        "insert" => return Some(KeyCode::Insert),
        "home" => return Some(KeyCode::Home),
        "end" => return Some(KeyCode::End),
        "pageup" => return Some(KeyCode::PageUp),
        "pagedown" => return Some(KeyCode::PageDown),
        "up" => return Some(KeyCode::Up),
        "down" => return Some(KeyCode::Down),
        "left" => return Some(KeyCode::Left),
        "right" => return Some(KeyCode::Right),
        "esc" | "escape" => return Some(KeyCode::Esc),
        _ => {}
    }

    if let Some(n) = text
        .strip_prefix(['f', 'F'])
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        return Some(KeyCode::F(n));
    }

    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyCode::Char(c)),
        _ => None,
    }
}

/// The active bindings, plus anything worth telling the user about the config.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: Vec<(Chord, Binding)>,
    /// Problems found while merging the user's overrides, in the order found.
    pub warnings: Vec<String>,
}

impl Keymap {
    pub fn binding_for(&self, key: &KeyEvent) -> Option<&Binding> {
        let chord = Chord::from_event(key);
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == chord)
            .map(|(_, binding)| binding)
    }

    /// The key to advertise for an action in the footer hint bar, ready to
    /// display: the most recently bound one, so a key added in the config wins
    /// over the default it sits beside. `None` when the action is unbound.
    pub fn display_key(&self, action: Action) -> Option<String> {
        self.bindings
            .iter()
            .rev()
            .find(|(_, binding)| *binding == Binding::Action(action))
            .map(|(chord, _)| chord.label())
    }

    /// Keys bound to an action, for the help popup.
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        self.bindings
            .iter()
            .filter(|(_, binding)| *binding == Binding::Action(action))
            .map(|(chord, _)| chord.describe())
            .collect()
    }

    fn set(&mut self, chord: Chord, binding: Binding) {
        self.bindings.retain(|(bound, _)| *bound != chord);
        self.bindings.push((chord, binding));
    }

    fn unset(&mut self, chord: Chord) {
        self.bindings.retain(|(bound, _)| *bound != chord);
    }

    fn get(&self, chord: Chord) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == chord)
            .map(|(_, binding)| binding)
    }
}

/// The built-in bindings.
pub fn default_keymap() -> Keymap {
    use Action::*;
    use KeyCode::*;

    let plain = |code| Chord::new(code, KeyModifiers::NONE);
    let ctrl = |code| Chord::new(code, KeyModifiers::CONTROL);
    let shift = |code| Chord::new(code, KeyModifiers::SHIFT);

    let defaults: Vec<(Chord, Action)> = vec![
        (plain(Enter), OpenEntry),
        (plain(Backspace), ParentDir),
        (plain(Char('g')), GotoFirst),
        (plain(Char('G')), GotoLast),
        (plain(Char('x')), ToggleSelect),
        (ctrl(Char('a')), SelectAll),
        (plain(Char('B')), BulkRename),
        (plain(Char('*')), SelectGlob),
        (plain(Char('S')), DirSizes),
        (plain(Char('q')), Quit),
        (plain(Char('h')), PaneLeft),
        (plain(Char('l')), PaneRight),
        (plain(Tab), PaneToggle),
        (plain(Char('?')), Help),
        (plain(Char(' ')), Preview),
        (plain(Char('/')), Search),
        (ctrl(Char('f')), FilterRegex),
        (ctrl(Char('g')), FindInFiles),
        (plain(Char(':')), CommandPalette),
        (plain(Char('r')), Rename),
        (plain(Char('a')), Create),
        (plain(Char('y')), Yank),
        (plain(Char('p')), Paste),
        (plain(Char('P')), PasteMove),
        (plain(Char('d')), DeleteChord),
        // The one-key form of yank-switch-paste, and the only operation that
        // names the other pane, so it gets the shifted "do it over there" key.
        (plain(Char('Y')), Copy),
        (plain(Char('M')), Move),
        (plain(KeyCode::Delete), Action::Delete),
        (plain(Char('j')), MoveDown),
        (plain(Down), MoveDown),
        (plain(Char('k')), MoveUp),
        (plain(Up), MoveUp),
        (ctrl(Char('h')), ToggleHidden),
        (ctrl(Char('l')), Refresh),
        (shift(Right), SortNext),
        (shift(Left), SortPrev),
        (plain(Char('O')), SortReverse),
    ];

    Keymap {
        bindings: defaults
            .into_iter()
            .map(|(chord, action)| (chord, Binding::Action(action)))
            .collect(),
        warnings: Vec::new(),
    }
}

/// Defaults plus the user's `[keybindings]` overrides.
///
/// Every override that takes a key rodeo already uses is reported, and so is
/// any action left with no key at all — silently losing a feature to a typo is
/// the failure mode worth protecting against.
pub fn build_keymap(config: &Config) -> Keymap {
    let mut map = default_keymap();
    let mut warnings = Vec::new();

    // Deterministic order, otherwise the warnings shuffle between runs.
    let mut overrides: Vec<(&String, &String)> = config.keybindings.iter().collect();
    overrides.sort();

    for (key_name, value) in overrides {
        let Some(chord) = parse_chord(key_name) else {
            warnings.push(format!("unknown key '{key_name}'"));
            continue;
        };

        // `none` frees the key instead of binding it.
        if matches!(value.to_lowercase().as_str(), "none" | "nop" | "") {
            map.unset(chord);
            continue;
        }

        let binding = match value.strip_prefix(':') {
            Some(command) => Binding::Command(command.trim().to_string()),
            None => match Action::from_name(value) {
                Some(action) => Binding::Action(action),
                None => {
                    warnings.push(format!(
                        "'{key_name}': unknown action '{value}' (a command must start with ':')"
                    ));
                    continue;
                }
            },
        };

        if let Some(existing) = map.get(chord)
            && *existing != binding
        {
            warnings.push(format!(
                "'{}' was {}, now {}",
                chord.describe(),
                existing.describe(),
                binding.describe()
            ));
        }

        map.set(chord, binding);
    }

    // Anything that can no longer be reached by any key.
    for action in Action::ALL {
        let bound_by_default = default_keymap()
            .bindings
            .iter()
            .any(|(_, b)| *b == Binding::Action(*action));
        if bound_by_default && map.keys_for(*action).is_empty() {
            warnings.push(format!("'{}' has no key any more", action.name()));
        }
    }

    map.warnings = warnings;
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(bindings: &[(&str, &str)]) -> Config {
        Config {
            keybindings: bindings
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn parses_plain_keys_and_named_keys() {
        assert_eq!(
            parse_chord("q"),
            Some(Chord::new(KeyCode::Char('q'), KeyModifiers::NONE))
        );
        assert_eq!(
            parse_chord("space"),
            Some(Chord::new(KeyCode::Char(' '), KeyModifiers::NONE))
        );
        assert_eq!(
            parse_chord("f5"),
            Some(Chord::new(KeyCode::F(5), KeyModifiers::NONE))
        );
        assert_eq!(
            parse_chord("+"),
            Some(Chord::new(KeyCode::Char('+'), KeyModifiers::NONE))
        );
        assert_eq!(parse_chord("nonsense"), None);
    }

    #[test]
    fn parses_modifiers() {
        assert_eq!(
            parse_chord("ctrl+f"),
            Some(Chord::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            parse_chord("shift+right"),
            Some(Chord::new(KeyCode::Right, KeyModifiers::SHIFT))
        );
        assert_eq!(
            parse_chord("alt+ctrl+j"),
            Some(Chord::new(
                KeyCode::Char('j'),
                KeyModifiers::ALT | KeyModifiers::CONTROL
            ))
        );
    }

    #[test]
    fn shift_is_folded_into_the_character() {
        // A terminal reports shift+g as an uppercase G, so both spellings and
        // the key press itself have to land on the same chord.
        let from_config = parse_chord("shift+g").unwrap();
        let written_upper = parse_chord("G").unwrap();
        let pressed = Chord::from_event(&press(KeyCode::Char('G'), KeyModifiers::SHIFT));

        assert_eq!(from_config, written_upper);
        assert_eq!(from_config, pressed);
    }

    #[test]
    fn shift_is_kept_for_keys_without_a_character() {
        let chord = parse_chord("shift+left").unwrap();
        assert_eq!(chord.modifiers, KeyModifiers::SHIFT);
        assert_ne!(chord, parse_chord("left").unwrap());
    }

    #[test]
    fn describe_round_trips() {
        for text in ["ctrl+f", "shift+right", "space", "f5", "G", "?"] {
            let chord = parse_chord(text).unwrap();
            assert_eq!(
                parse_chord(&chord.describe()),
                Some(chord),
                "{text} described as {}",
                chord.describe()
            );
        }
    }

    #[test]
    fn defaults_resolve_key_presses() {
        let map = default_keymap();

        assert_eq!(
            map.binding_for(&press(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(&Binding::Action(Action::Quit))
        );
        assert_eq!(
            map.binding_for(&press(KeyCode::Char('f'), KeyModifiers::CONTROL)),
            Some(&Binding::Action(Action::FilterRegex))
        );
        assert_eq!(
            map.binding_for(&press(KeyCode::Right, KeyModifiers::SHIFT)),
            Some(&Binding::Action(Action::SortNext))
        );
    }

    #[test]
    fn a_key_can_run_a_command() {
        let map = build_keymap(&config_with(&[("z", ":term lazygit")]));

        assert_eq!(
            map.binding_for(&press(KeyCode::Char('z'), KeyModifiers::NONE)),
            Some(&Binding::Command("term lazygit".to_string()))
        );
        assert!(map.warnings.is_empty(), "{:?}", map.warnings);
    }

    #[test]
    fn overriding_a_used_key_is_reported() {
        let map = build_keymap(&config_with(&[("x", ":term lazygit")]));

        assert_eq!(map.warnings.len(), 2, "{:?}", map.warnings);
        assert!(
            map.warnings[0].contains("'x' was select"),
            "{:?}",
            map.warnings
        );
        // And the action it displaced is now unreachable, which matters more.
        assert!(
            map.warnings
                .iter()
                .any(|w| w.contains("'select' has no key")),
            "{:?}",
            map.warnings
        );
    }

    #[test]
    fn rebinding_an_action_to_a_spare_key_keeps_it_reachable() {
        let map = build_keymap(&config_with(&[("z", "select")]));

        assert!(map.warnings.is_empty(), "{:?}", map.warnings);
        // Both the default and the new key work.
        assert_eq!(map.keys_for(Action::ToggleSelect).len(), 2);
    }

    #[test]
    fn a_key_can_be_freed() {
        // `Q` keeps quit reachable, so freeing `q` costs nothing.
        let map = build_keymap(&config_with(&[("Q", "quit"), ("q", "none")]));

        assert!(
            map.binding_for(&press(KeyCode::Char('q'), KeyModifiers::NONE))
                .is_none()
        );
        assert_eq!(
            map.binding_for(&press(KeyCode::Char('Q'), KeyModifiers::NONE)),
            Some(&Binding::Action(Action::Quit))
        );
        assert!(map.warnings.is_empty(), "{:?}", map.warnings);
    }

    #[test]
    fn freeing_the_last_key_of_an_action_is_reported() {
        let map = build_keymap(&config_with(&[("q", "none")]));

        assert!(
            map.warnings.iter().any(|w| w.contains("'quit' has no key")),
            "{:?}",
            map.warnings
        );
    }

    #[test]
    fn typos_are_reported_rather_than_ignored() {
        let map = build_keymap(&config_with(&[("ctrl+nonsense", "quit"), ("z", "qiut")]));

        assert!(map.warnings.iter().any(|w| w.contains("unknown key")));
        assert!(
            map.warnings
                .iter()
                .any(|w| w.contains("unknown action 'qiut'"))
        );
    }

    #[test]
    fn every_action_has_a_unique_name() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate action name");
    }

    #[test]
    fn every_action_can_be_looked_up_by_name() {
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(*action));
        }
    }

    #[test]
    fn every_action_has_a_default_key() {
        let map = default_keymap();
        for action in Action::ALL {
            assert!(
                !map.keys_for(*action).is_empty(),
                "{} is not bound to anything",
                action.name()
            );
        }
    }
}
