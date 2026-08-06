//! The bottom bar: context-dependent key hints, clipboard state and transient
//! status messages.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{
    component::Component,
    keymap::{Action, Keymap},
    panes::PaneStats,
    theme::Theme,
    uiconfig::UiConfig,
};

/// How long a status message stays visible.
const STATUS_TTL: Duration = Duration::from_secs(3);
/// Blank cells between two footer hints.
const ENTRY_GAP: u16 = 2;

#[derive(Debug)]
struct StatusMsg {
    text: String,
    is_error: bool,
    at: Instant,
}

/// What the hint bar advertises, in order. The keys come from the keymap, so
/// the bar tells the truth after a rebind instead of repeating the defaults.
const HINTS: &[(Action, &str)] = &[
    (Action::Help, "Help"),
    (Action::Rename, "Rename"),
    (Action::Preview, "View"),
    (Action::OpenEntry, "Edit"),
    (Action::Copy, "Copy"),
    (Action::Move, "Move"),
    (Action::Create, "Create"),
    (Action::Delete, "Delete"),
    (Action::PaneToggle, "Panes"),
    (Action::ToggleSelect, "Select"),
    (Action::ToggleHidden, "Hidden"),
    (Action::Search, "Find"),
    (Action::Quit, "Quit"),
];

/// What the bar advertises while entries are selected. Same contract as
/// [`HINTS`]: the keys come from the keymap, never from a literal.
const SELECTION_HINTS: &[(Action, &str)] = &[
    (Action::Copy, "Copy"),
    (Action::Move, "Move"),
    (Action::Delete, "Delete"),
];

/// Resolves `(action, label)` pairs against the keymap, dropping actions the
/// user has unbound.
fn resolve_hints(hints: &[(Action, &str)], keymap: &Keymap) -> Vec<String> {
    hints
        .iter()
        .filter_map(|(action, label)| {
            let key = keymap.display_key(*action)?;
            Some(format!("{key} {label}"))
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct Footer {
    pub keymaps: Vec<String>,
    /// Hints shown instead of [`Self::keymaps`] while entries are selected.
    selection_keymaps: Vec<String>,
    stats: Option<PaneStats>,
    clipboard: Option<(usize, bool)>,
    status: Option<StatusMsg>,
}

impl Footer {
    /// Rebuilds the hint bar from the active bindings. Called at startup and
    /// again after `:so`, so the bar follows the config.
    pub fn update_hints(&mut self, keymap: &Keymap) {
        self.keymaps = resolve_hints(HINTS, keymap);
        self.selection_keymaps = resolve_hints(SELECTION_HINTS, keymap);
    }

    pub fn set_stats(&mut self, stats: PaneStats) {
        self.stats = Some(stats);
    }

    /// Shows a transient status message (auto-cleared after a few seconds).
    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status = Some(StatusMsg {
            text,
            is_error,
            at: Instant::now(),
        });
    }

    /// The status message currently on show, if it has not expired.
    pub fn status_text(&self) -> Option<&str> {
        self.status
            .as_ref()
            .filter(|s| s.at.elapsed() <= STATUS_TTL)
            .map(|s| s.text.as_str())
    }

    /// Clipboard state: (entry count, cut?) — shown until the clipboard is empty.
    pub fn set_clipboard(&mut self, clipboard: Option<(usize, bool)>) {
        self.clipboard = clipboard;
    }

    /// Bindings relevant to the current context: bulk actions when files are
    /// selected, the full key list otherwise.
    fn visible_keymaps(&self) -> Vec<String> {
        match &self.stats {
            Some(s) if s.selected > 0 => {
                let mut hints = vec![format!("●{} selected", s.selected)];
                hints.extend(self.selection_keymaps.iter().cloned());
                // Esc is not a bindable action — it is handled directly in
                // `App::handle_esc` — so it is the one label here that cannot
                // come from the keymap.
                hints.push("Esc Unselect".to_string());
                hints
            }
            _ => self.keymaps.clone(),
        }
    }
}

impl Component for Footer {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(theme.colors.surface()));
        let inner_area = bg_block.inner(area);
        frame.render_widget(bg_block, area);

        // Expired status messages are dropped.
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.at.elapsed() > STATUS_TTL)
        {
            self.status = None;
        }

        let mut keymaps = Vec::new();
        if let Some((count, cut)) = self.clipboard {
            keymaps.push(if cut {
                format!("[{count} cut]")
            } else {
                format!("[{count} yanked]")
            });
        }
        keymaps.extend(self.visible_keymaps());

        // A fixed grid of narrow cells used to cut labels mid-word ("F7 Mkdi",
        // "^h Hidd"). Lay the hints out as one line instead and drop whole
        // entries from the end when they do not fit.
        let status_width = self
            .status
            .as_ref()
            .map(|s| s.text.chars().count() as u16 + 2)
            .unwrap_or(0);
        let available = inner_area.width.saturating_sub(status_width + 2);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0u16;
        let mut dropped = false;
        for entry in &keymaps {
            let width = entry.chars().count() as u16 + ENTRY_GAP;
            if used + width > available {
                dropped = true;
                break;
            }
            used += width;

            // "F5 Copy" renders the key in the accent colour, the action muted.
            match entry.split_once(' ') {
                Some((key, label)) => {
                    spans.push(Span::styled(
                        key.to_string(),
                        Style::default().fg(theme.colors.primary()),
                    ));
                    spans.push(Span::styled(
                        format!(" {label}"),
                        Style::default().fg(theme.colors.muted()),
                    ));
                }
                None => spans.push(Span::styled(
                    entry.clone(),
                    Style::default().fg(theme.colors.accent1()),
                )),
            }
            spans.push(Span::raw(" ".repeat(ENTRY_GAP as usize)));
        }
        if dropped {
            spans.push(Span::styled("…", Style::default().fg(theme.colors.muted())));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Left),
            inner_area,
        );

        if let Some(status) = &self.status {
            let style = if status.is_error {
                Style::default().fg(theme.colors.error())
            } else {
                Style::default().fg(theme.colors.info())
            };
            frame.render_widget(
                Paragraph::new(status.text.as_str())
                    .style(style)
                    .block(Block::default().padding(Padding::horizontal(1)))
                    .alignment(HorizontalAlignment::Right),
                inner_area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::keymap::build_keymap;

    fn hints_with(bindings: &[(&str, &str)]) -> Vec<String> {
        let config = Config {
            keybindings: bindings
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        };
        let mut footer = Footer::default();
        footer.update_hints(&build_keymap(&config));
        footer.keymaps
    }

    #[test]
    fn hints_are_built_from_the_defaults() {
        let hints = hints_with(&[]);

        assert!(hints.contains(&"Y Copy".to_string()), "{hints:?}");
        assert!(hints.contains(&"^h Hidden".to_string()), "{hints:?}");
        assert!(hints.contains(&"Space View".to_string()), "{hints:?}");
    }

    /// The bar used to be a hardcoded list, so it lied to anyone who rebound a
    /// key. It has to follow the keymap instead.
    #[test]
    fn hints_follow_a_rebound_key() {
        // The config key wins over the default it sits beside, so pasting the
        // Midnight Commander block relabels the bar.
        let hints = hints_with(&[("f5", "copy")]);

        assert!(hints.contains(&"F5 Copy".to_string()), "{hints:?}");
        assert!(!hints.iter().any(|h| h.starts_with("Y ")), "{hints:?}");
    }

    /// The bar used to derive its labels by string-parsing the config form and
    /// only knew how to strip `ctrl+`, so these rendered as the literal text
    /// `alt+y` and `shift+f5`.
    #[test]
    fn hints_keep_modifiers_other_than_ctrl() {
        let hints = hints_with(&[("alt+y", "copy"), ("shift+f5", "move")]);

        assert!(hints.contains(&"Alt+y Copy".to_string()), "{hints:?}");
        assert!(hints.contains(&"Shift+F5 Move".to_string()), "{hints:?}");
    }

    fn footer_with_selection(bindings: &[(&str, &str)], selected: usize) -> Vec<String> {
        let config = Config {
            keybindings: bindings
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        };
        let mut footer = Footer::default();
        footer.update_hints(&build_keymap(&config));
        footer.set_stats(PaneStats {
            selected,
            ..Default::default()
        });
        footer.visible_keymaps()
    }

    /// The selection bar hardcoded `F5 Copy` / `F6 Move` / `F8 Delete`, which
    /// is exactly the lie `hints_follow_a_rebound_key` forbids one function
    /// away.
    #[test]
    fn selection_hints_follow_the_keymap() {
        let hints = footer_with_selection(&[], 3);

        assert!(hints.contains(&"●3 selected".to_string()), "{hints:?}");
        // The defaults bind copy/move/delete to letters, not to F-keys.
        assert!(!hints.iter().any(|h| h.starts_with("F5 ")), "{hints:?}");
        assert!(hints.iter().any(|h| h.ends_with(" Copy")), "{hints:?}");
        assert!(hints.contains(&"Esc Unselect".to_string()), "{hints:?}");
    }

    #[test]
    fn selection_hints_follow_a_rebound_key() {
        let hints = footer_with_selection(&[("f5", "copy")], 1);

        assert!(hints.contains(&"F5 Copy".to_string()), "{hints:?}");
    }

    /// An action the user unbound has nothing to advertise.
    #[test]
    fn an_unbound_action_is_dropped_from_the_bar() {
        let hints = hints_with(&[("q", "none")]);

        assert!(!hints.iter().any(|h| h.ends_with(" Quit")), "{hints:?}");
    }
}
