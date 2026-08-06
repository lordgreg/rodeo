//! Bulk rename popup: displays old → new names with live preview and applies
//! `s/regex/replacement/` or `%d` (sequential numbering) patterns.

use std::sync::OnceLock;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use regex::Regex;
use std::path::PathBuf;

use crate::ui::{component::Component, textinput::TextInput, theme::Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum RenameError {
    /// Two entries would get the same new name.
    Collision(String),
    /// Resulting name is empty.
    EmptyName(String),
    /// The regex in the pattern is invalid.
    BadPattern(String),
}

#[derive(Debug)]
pub struct BulkRename {
    /// Original file names (base names only).
    pub originals: Vec<PathBuf>,
    /// Pattern input bar (`s/old/new/` or `%d`).
    pub pattern: TextInput,
    /// Pre-computed new names (parallel to `originals`).
    pub previews: Vec<String>,
    /// Any validation errors across all renames.
    pub errors: Vec<RenameError>,
}

impl BulkRename {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let previews: Vec<String> = paths
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        Self {
            originals: paths,
            pattern: TextInput::default(),
            previews,
            errors: Vec::new(),
        }
    }

    /// Re-computes `previews` and `errors` from the current pattern string.
    pub fn update_preview(&mut self) {
        let pat = self.pattern.value.trim();

        // Reset to originals first.
        self.previews = self
            .originals
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        self.errors.clear();

        if pat.is_empty() {
            return;
        }

        // --- `s/regex/replacement/[flags]` substitution ---
        if let Some(rest) = pat.strip_prefix("s/") {
            // Find the delimiter between regex and replacement (first unescaped '/').
            let parts: Vec<&str> = rest.splitn(3, '/').collect();
            if parts.len() < 2 {
                self.errors.push(RenameError::BadPattern(
                    "Syntax: s/regex/replacement/".into(),
                ));
                return;
            }
            let (regex_str, replacement) = (parts[0], parts[1]);
            let flags = parts.get(2).copied().unwrap_or("");
            let global = flags.contains('g');

            let re = match Regex::new(regex_str) {
                Ok(r) => r,
                Err(e) => {
                    self.errors
                        .push(RenameError::BadPattern(format!("Invalid regex: {e}")));
                    return;
                }
            };

            for (preview, orig) in self.previews.iter_mut().zip(self.originals.iter()) {
                let name = orig
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                *preview = if global {
                    re.replace_all(&name, replacement).into_owned()
                } else {
                    re.replace(&name, replacement).into_owned()
                };
            }
        }
        // --- `%d` sequential numbering ---
        else if pat.contains("%d") || pat.contains("%0") {
            // Pattern like `photo_%03d.jpg` — extract extension from originals.
            for (i, (preview, orig)) in self
                .previews
                .iter_mut()
                .zip(self.originals.iter())
                .enumerate()
            {
                let ext = orig
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();

                // Support `%d`, `%Nd`, `%0Nd` padding.
                let numbered = apply_number_format(pat, i + 1, &ext);
                *preview = numbered;
            }
        }
        // Unknown pattern syntax — show error.
        else {
            self.errors.push(RenameError::BadPattern(
                "Use s/old/new/ for substitution or photo_%03d.jpg for numbering".into(),
            ));
            return;
        }

        // Validate: detect empty names and collisions.
        for preview in &self.previews {
            if preview.is_empty() {
                self.errors.push(RenameError::EmptyName(preview.clone()));
            }
        }

        let mut seen = std::collections::HashSet::new();
        for preview in &self.previews {
            if !seen.insert(preview.clone()) {
                self.errors.push(RenameError::Collision(preview.clone()));
            }
        }
    }

    /// Returns `true` if the preview is valid and can be applied.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
            && !self.pattern.value.trim().is_empty()
            && self
                .previews
                .iter()
                .zip(self.originals.iter())
                .any(|(new, orig)| {
                    orig.file_name()
                        .map(|n| n.to_string_lossy() != new.as_str())
                        .unwrap_or(false)
                })
    }

    /// Returns pairs of `(old_path, new_path)` for renames that actually change
    /// the name.
    pub fn rename_pairs(&self) -> Vec<(PathBuf, PathBuf)> {
        self.originals
            .iter()
            .zip(self.previews.iter())
            .filter_map(|(orig, new_name)| {
                let old_name = orig.file_name()?.to_string_lossy();
                if old_name == new_name.as_str() {
                    return None; // unchanged
                }
                let new_path = orig.with_file_name(new_name);
                Some((orig.clone(), new_path))
            })
            .collect()
    }
}

/// Expands a numbering pattern like `photo_%03d.jpg` → `photo_001.jpg`.
fn apply_number_format(pat: &str, n: usize, ext: &str) -> String {
    // Replace %0Nd or %Nd with zero-padded / unpadded number; append extension
    // when the pattern doesn't already contain one. The pattern is a literal,
    // so it is compiled once and cannot fail at runtime.
    static NUMBER_PATTERN: OnceLock<Regex> = OnceLock::new();
    let re = NUMBER_PATTERN
        .get_or_init(|| Regex::new(r"%0?(\d*)d").expect("literal numbering pattern must compile"));
    let result = re.replace(pat, |caps: &regex::Captures| {
        let width: usize = caps[1].parse().unwrap_or(0);
        if width > 0 {
            format!("{:0>width$}", n, width = width)
        } else {
            n.to_string()
        }
    });
    // If the pattern has no extension placeholder, append the original extension.
    if !result.contains('.') {
        format!("{result}{ext}")
    } else {
        result.into_owned()
    }
}

impl Component for BulkRename {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        // Centered popup: 80 % wide, 80 % tall.
        let popup = Rect {
            x: area.width / 10,
            y: area.height / 10,
            width: area.width * 4 / 5,
            height: area.height * 4 / 5,
        };
        frame.render_widget(Clear, popup);

        let border_style = if self.errors.is_empty() && !self.pattern.value.trim().is_empty() {
            Style::new().fg(theme.colors.success())
        } else if !self.errors.is_empty() {
            Style::new().fg(theme.colors.error())
        } else {
            Style::new().fg(theme.colors.secondary())
        };

        let count = self.rename_pairs().len();
        let title = format!(
            " Bulk Rename — {} file{} • Enter=apply  Esc=cancel ",
            count,
            if count == 1 { "" } else { "s" }
        );
        let outer_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);
        let inner = outer_block.inner(popup);
        frame.render_widget(outer_block, popup);

        // Layout: list (fill) + error bar (if any) + pattern input (3 lines).
        let input_height = 3u16;
        let error_height = if self.errors.is_empty() { 0u16 } else { 1u16 };
        let list_height = inner.height.saturating_sub(input_height + error_height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(list_height),
                Constraint::Length(error_height),
                Constraint::Length(input_height),
            ])
            .split(inner);

        // --- Two-column rename preview ---
        let col_width = (chunks[0].width / 2).saturating_sub(2) as usize;
        let items: Vec<ListItem> = self
            .originals
            .iter()
            .zip(self.previews.iter())
            .map(|(orig, new)| {
                let old_name = orig
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let changed = old_name != *new;
                let old_span = Span::styled(
                    truncate(&old_name, col_width),
                    if changed {
                        Style::new().fg(theme.colors.muted())
                    } else {
                        Style::new().fg(theme.colors.primary())
                    },
                );
                let arrow = Span::styled(
                    if changed { " → " } else { "   " },
                    Style::new().fg(theme.colors.accent1()),
                );
                let new_span = if changed {
                    Span::styled(
                        truncate(new, col_width),
                        Style::new().fg(theme.colors.success()).bold(),
                    )
                } else {
                    Span::styled(
                        truncate(new, col_width),
                        Style::new().fg(theme.colors.primary()),
                    )
                };
                ListItem::new(Line::from(vec![old_span, arrow, new_span]))
            })
            .collect();
        let list = List::new(items);
        frame.render_widget(list, chunks[0]);

        // --- Error bar ---
        if !self.errors.is_empty() {
            let msg = match &self.errors[0] {
                RenameError::Collision(n) => format!("⚠ collision: '{n}'"),
                RenameError::EmptyName(_) => "⚠ result would be empty".into(),
                RenameError::BadPattern(m) => format!("⚠ {m}"),
            };
            let err = Paragraph::new(msg).style(Style::new().fg(theme.colors.error()));
            frame.render_widget(err, chunks[1]);
        }

        // --- Pattern input ---
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(" Pattern: s/old/new/[g]  or  prefix_%03d ")
            .border_style(Style::new().fg(theme.colors.border()));
        let input_inner = input_block.inner(chunks[2]);
        frame.render_widget(input_block, chunks[2]);

        frame.render_widget(Paragraph::new(self.pattern.value.clone()), input_inner);
        frame.set_cursor_position((input_inner.x + self.pattern.cursor as u16, input_inner.y));
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(names: &[&str], pattern: &str) -> BulkRename {
        let mut br = BulkRename::new(
            names
                .iter()
                .map(|n| PathBuf::from("/tmp").join(n))
                .collect(),
        );
        br.pattern = TextInput::new(pattern);
        br.update_preview();
        br
    }

    fn previews(br: &BulkRename) -> Vec<&str> {
        br.previews.iter().map(String::as_str).collect()
    }

    #[test]
    fn substitution_replaces_the_first_match_only() {
        let br = rename(&["a_a_1.txt", "a_b_2.txt"], "s/a/X/");
        assert_eq!(previews(&br), vec!["X_a_1.txt", "X_b_2.txt"]);
        assert!(br.errors.is_empty());
    }

    #[test]
    fn the_g_flag_replaces_every_match() {
        let br = rename(&["a_a_1.txt"], "s/a/X/g");
        assert_eq!(previews(&br), vec!["X_X_1.txt"]);
    }

    #[test]
    fn substitution_can_use_capture_groups() {
        let br = rename(&["img_1.png", "img_2.png"], r"s/img_(\d)/shot_$1/");
        assert_eq!(previews(&br), vec!["shot_1.png", "shot_2.png"]);
    }

    #[test]
    fn numbering_pads_to_the_requested_width_and_keeps_the_extension() {
        let br = rename(&["a.txt", "b.txt", "c.txt"], "photo_%03d");
        assert_eq!(
            previews(&br),
            vec!["photo_001.txt", "photo_002.txt", "photo_003.txt"]
        );
    }

    #[test]
    fn numbering_without_padding_counts_from_one() {
        let br = rename(&["a.md", "b.md"], "n%d");
        assert_eq!(previews(&br), vec!["n1.md", "n2.md"]);
    }

    /// The pattern is applied to every entry, so a constant result collides.
    #[test]
    fn a_pattern_that_maps_two_names_onto_one_is_rejected() {
        let br = rename(&["a.txt", "b.txt"], "s/[ab]/same/");
        assert!(
            br.errors
                .iter()
                .any(|e| matches!(e, RenameError::Collision(_))),
            "{:?}",
            br.errors
        );
        assert!(!br.is_valid(), "a colliding rename must not be applicable");
    }

    #[test]
    fn a_pattern_that_empties_a_name_is_rejected() {
        let br = rename(&["a.txt"], "s/a.txt//");
        assert!(
            br.errors
                .iter()
                .any(|e| matches!(e, RenameError::EmptyName(_))),
            "{:?}",
            br.errors
        );
        assert!(!br.is_valid());
    }

    #[test]
    fn an_invalid_regex_is_reported_and_leaves_the_names_alone() {
        let br = rename(&["a.txt"], "s/[unclosed/x/");
        assert!(
            br.errors
                .iter()
                .any(|e| matches!(e, RenameError::BadPattern(_))),
            "{:?}",
            br.errors
        );
        assert_eq!(previews(&br), vec!["a.txt"], "originals must be preserved");
        assert!(!br.is_valid());
    }

    #[test]
    fn unknown_pattern_syntax_is_reported() {
        let br = rename(&["a.txt"], "just some text");
        assert!(matches!(
            br.errors.first(),
            Some(RenameError::BadPattern(_))
        ));
        assert!(!br.is_valid());
    }

    #[test]
    fn an_empty_pattern_changes_nothing_and_is_not_applicable() {
        let br = rename(&["a.txt", "b.txt"], "");
        assert_eq!(previews(&br), vec!["a.txt", "b.txt"]);
        assert!(br.errors.is_empty());
        assert!(!br.is_valid(), "nothing to do is not a valid rename");
    }

    /// A pattern that matches nothing leaves every name as it was, which is
    /// also not worth applying.
    #[test]
    fn a_pattern_that_matches_nothing_is_not_applicable() {
        let br = rename(&["a.txt"], "s/zzz/x/");
        assert_eq!(previews(&br), vec!["a.txt"]);
        assert!(br.errors.is_empty());
        assert!(!br.is_valid());
    }

    #[test]
    fn rename_pairs_skip_unchanged_names_and_keep_the_directory() {
        // Only `b.txt` matches, so only it should be renamed.
        let br = rename(&["a.txt", "b.txt"], "s/b/c/");
        let pairs = br.rename_pairs();

        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].0, PathBuf::from("/tmp/b.txt"));
        assert_eq!(pairs[0].1, PathBuf::from("/tmp/c.txt"));
    }

    #[test]
    fn rename_pairs_stay_in_the_original_directory() {
        let mut br = BulkRename::new(vec![PathBuf::from("/one/deep/x.txt")]);
        br.pattern = TextInput::new("s/x/y/");
        br.update_preview();

        assert_eq!(
            br.rename_pairs()[0].1,
            PathBuf::from("/one/deep/y.txt"),
            "a rename must never move a file"
        );
    }

    #[test]
    fn multibyte_names_survive_a_rename() {
        let br = rename(&["日本語.txt", "café.txt"], "s/\\.txt/.md/");
        assert_eq!(previews(&br), vec!["日本語.md", "café.md"]);
    }
}
