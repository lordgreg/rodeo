//! The syntax-highlighted preview pane shared by the search popups.
//!
//! Find-in-files shows the file around a matching line, the file finder shows
//! the top of the selected file, and both want the same thing: a bounded
//! window of highlighted text with a line-number gutter. Keeping it here means
//! the two popups cannot drift apart, and the (surprisingly fiddly) scrolling
//! arithmetic exists once.

use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use syntect::{easy::HighlightLines, highlighting};

use crate::ui::{
    popup_preview::{syntax_set, syntect_style_to_ratatui},
    theme::Theme,
};

/// Lines kept on each side of the anchor line. Enough to scroll a screenful
/// or two without re-reading the file.
pub const PREVIEW_CONTEXT: usize = 150;
/// Beyond this line number the preview stops highlighting from the top of the
/// file (which is what keeps multi-line constructs correct) and starts at the
/// window instead, to bound the work per keystroke.
const HIGHLIGHT_FROM_START_LIMIT: usize = 20_000;
/// Most directory entries listed in a directory preview.
const MAX_DIR_ENTRIES: usize = 500;

/// The preview for one selected item, cached by its owner until the selection
/// moves.
#[derive(Debug)]
pub enum Preview {
    /// Highlighted window of the file plus the 1-based number of its first
    /// line, so the gutter and the highlighted line line up.
    Lines {
        lines: Vec<Line<'static>>,
        first_line: usize,
    },
    Error(String),
}

/// Reads a window of `path` around `anchor` (a 1-based line) and highlights it.
pub fn build_preview(path: &Path, anchor: usize, syn_theme: &highlighting::Theme) -> Preview {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return Preview::Error(format!("Cannot preview file: {e}")),
    };

    let first_line = anchor.saturating_sub(PREVIEW_CONTEXT).max(1);
    let last_line = anchor.saturating_add(PREVIEW_CONTEXT);

    let ss = syntax_set();
    let syntax = ss
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, syn_theme);

    // Highlighting from the top of the file keeps block comments and strings
    // coloured correctly; for very deep anchors that is too much work, so the
    // window is highlighted on its own.
    let highlight_from = if anchor > HIGHLIGHT_FROM_START_LIMIT {
        first_line
    } else {
        1
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (idx, raw) in content.lines().enumerate() {
        let num = idx + 1;
        if num > last_line {
            break;
        }
        if num < highlight_from {
            continue;
        }
        // Tabs render as a single cell in a Paragraph, which shifts code out
        // of alignment with the gutter; expand them like an editor would.
        let text = raw.replace('\t', "    ");
        let spans = match highlighter.highlight_line(&text, ss) {
            Ok(regions) => regions
                .iter()
                .map(|(style, part)| {
                    Span::styled(part.to_string(), syntect_style_to_ratatui(*style))
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![Span::raw(text)],
        };
        if num >= first_line {
            lines.push(Line::from(spans));
        }
    }

    if lines.is_empty() {
        return Preview::Error("(empty file)".to_string());
    }

    Preview::Lines { lines, first_line }
}

/// Lists a directory, so selecting one in the file finder still shows
/// something useful instead of an error about not being a file.
pub fn build_dir_preview(path: &Path) -> Preview {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return Preview::Error(format!("Cannot read directory: {e}")),
    };

    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .take(MAX_DIR_ENTRIES)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir { format!("{name}/") } else { name }
        })
        .collect();
    names.sort();

    if names.is_empty() {
        return Preview::Error("(empty directory)".to_string());
    }

    Preview::Lines {
        lines: names.into_iter().map(Line::from).collect(),
        first_line: 1,
    }
}

/// Draws `preview` into `area` inside a titled block.
///
/// `anchor` is the line to centre on and highlight (the find-in-files hit);
/// `None` shows the window from its start, which is what the file finder
/// wants. `scroll` is the caller's manual offset and is written back clamped,
/// so holding a scroll key cannot build up an offset that must be undone
/// before the view moves again.
pub fn render_preview(
    frame: &mut Frame<'_>,
    theme: &Theme,
    area: Rect,
    title: &str,
    preview: Option<&Preview>,
    anchor: Option<usize>,
    scroll: &mut i32,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.colors.border()))
        .title(format!(" {title} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let (lines, first_line) = match preview {
        Some(Preview::Lines { lines, first_line }) => (lines, *first_line),
        Some(Preview::Error(msg)) => {
            let p = Paragraph::new(msg.as_str()).style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(p, inner);
            return;
        }
        None => return,
    };

    let height = inner.height as usize;
    let anchor_idx = anchor.unwrap_or(first_line).saturating_sub(first_line);
    // With an anchor the window is centred on it; without one it starts at the
    // top of what was loaded.
    let base = match anchor {
        Some(_) => anchor_idx.saturating_sub(height / 2) as i32,
        None => 0,
    };
    let max_top = lines.len().saturating_sub(height) as i32;
    let top = (base + *scroll).clamp(0, max_top.max(0)) as usize;
    *scroll = top as i32 - base;

    let last_shown = first_line + (top + height).min(lines.len());
    let gutter = last_shown.to_string().len();

    let rendered: Vec<Line<'static>> = lines
        .iter()
        .enumerate()
        .skip(top)
        .take(height)
        .map(|(i, line)| {
            let num = first_line + i;
            let is_anchor = anchor == Some(num);
            let num_style = if is_anchor {
                Style::new().fg(theme.colors.accent1()).bold()
            } else {
                Style::new().fg(theme.colors.muted())
            };
            let mut spans = vec![Span::styled(format!("{num:>gutter$} "), num_style)];
            spans.extend(line.spans.iter().cloned());
            let out = Line::from(spans);
            if is_anchor {
                out.style(Style::new().bg(theme.colors.surface()))
            } else {
                out
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rendered), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_window_is_centred_on_the_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let Preview::Lines { lines, first_line } =
            build_preview(&path, 200, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };

        assert_eq!(first_line, 200 - PREVIEW_CONTEXT);
        let text: String = lines[200 - first_line]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(text, "line 200");
    }

    #[test]
    fn preview_of_a_short_file_starts_at_line_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.txt");
        std::fs::write(&path, "a\nb\nc\n").unwrap();

        let Preview::Lines { first_line, lines } =
            build_preview(&path, 2, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };
        assert_eq!(first_line, 1);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn missing_file_previews_as_an_error_instead_of_panicking() {
        let preview = build_preview(
            Path::new("/definitely/not/here.txt"),
            1,
            &highlighting::Theme::default(),
        );
        assert!(matches!(preview, Preview::Error(_)));
    }

    #[test]
    fn a_directory_previews_as_its_sorted_listing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();

        let Preview::Lines { lines, .. } = build_dir_preview(dir.path()) else {
            panic!("expected a listing");
        };
        let names: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert_eq!(names, vec!["a.txt".to_string(), "sub/".to_string()]);
    }

    #[test]
    fn an_empty_directory_says_so() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(build_dir_preview(dir.path()), Preview::Error(_)));
    }
}
