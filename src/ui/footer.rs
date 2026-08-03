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

/// Turns a config-style chord (`ctrl+h`, `space`, `f5`) into something worth
/// putting on a status bar.
fn pretty_key(chord: &str) -> String {
    let (prefix, key) = match chord.strip_prefix("ctrl+") {
        Some(rest) => ("^", rest),
        None => ("", chord),
    };

    let key = match key {
        "space" => "Space".to_string(),
        "tab" => "Tab".to_string(),
        "enter" => "Enter".to_string(),
        "backspace" => "Bksp".to_string(),
        "delete" => "Del".to_string(),
        // Function keys are written `f5` in a config but `F5` on a keyboard.
        other
            if other.starts_with('f')
                && other.len() > 1
                && other[1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            other.to_uppercase()
        }
        other => other.to_string(),
    };

    format!("{prefix}{key}")
}

#[derive(Debug, Default)]
pub struct Footer {
    pub keymaps: Vec<String>,
    stats: Option<PaneStats>,
    clipboard: Option<(usize, bool)>,
    status: Option<StatusMsg>,
}

impl Footer {
    /// Rebuilds the hint bar from the active bindings. Called at startup and
    /// again after `:so`, so the bar follows the config.
    pub fn update_hints(&mut self, keymap: &Keymap) {
        self.keymaps = HINTS
            .iter()
            .filter_map(|(action, label)| {
                let key = keymap.display_key(*action)?;
                Some(format!("{} {label}", pretty_key(&key)))
            })
            .collect();
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
            Some(s) if s.selected > 0 => vec![
                format!("●{} selected", s.selected),
                "F5 Copy".to_string(),
                "F6 Move".to_string(),
                "F8 Delete".to_string(),
                "Esc Unselect".to_string(),
            ],
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

    /// An action the user unbound has nothing to advertise.
    #[test]
    fn an_unbound_action_is_dropped_from_the_bar() {
        let hints = hints_with(&[("q", "none")]);

        assert!(!hints.iter().any(|h| h.ends_with(" Quit")), "{hints:?}");
    }
}
