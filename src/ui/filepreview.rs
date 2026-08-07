//! The syntax-highlighted preview pane shared by the search popups.
//!
//! Find-in-files shows the file around a matching line, the file finder shows
//! the top of the selected file, and both want the same thing: a bounded
//! window of highlighted text with a line-number gutter. Keeping it here means
//! the two popups cannot drift apart, and the (surprisingly fiddly) scrolling
//! arithmetic exists once.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::Arc;

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
/// Most bytes read to build one preview.
///
/// A preview only ever shows a few hundred lines, but the file has to be
/// walked to reach them. Without this the walk was a `read_to_string` of the
/// whole file on the UI thread: selecting a multi-gigabyte log, or a binary
/// blob containing no newline at all, pulled all of it into memory mid-draw.
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

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

/// The requested slice of a file, and whether the read stopped on the byte cap
/// rather than on the end of the file.
struct Window {
    /// Lines `from ..= last_line`, in order. Lines before `from` are read (the
    /// file has to be walked to reach the window) but never kept.
    lines: Vec<String>,
    /// The 1-based number of `lines[0]`, which is `from` unless the file ended
    /// first.
    from: usize,
    truncated: bool,
}

/// Reads lines `from ..= last_line` of `path`, stopping early at
/// [`MAX_PREVIEW_BYTES`].
///
/// Streaming rather than slurping is what bounds the work. The preview needs
/// at most a few hundred lines, so there is never a reason to hold the rest of
/// the file: earlier lines are walked past and dropped, later ones are never
/// read, and the byte cap catches the pathological case of a file with no
/// newline in it at all.
fn read_window(path: &Path, from: usize, last_line: usize) -> std::io::Result<Window> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file.take(MAX_PREVIEW_BYTES));

    let mut lines: Vec<String> = Vec::new();
    let mut seen: usize = 0;
    let mut read: u64 = 0;
    let mut buf = String::new();

    while seen < last_line {
        buf.clear();
        // Errors on invalid UTF-8, which is how binaries are rejected — the
        // same way the old `read_to_string` rejected them.
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        read += n as u64;
        seen += 1;
        if seen >= from {
            lines.push(buf.trim_end_matches(['\n', '\r']).to_string());
        }
    }

    Ok(Window {
        truncated: read >= MAX_PREVIEW_BYTES && seen < last_line,
        from,
        lines,
    })
}

/// Reads a window of `path` around `anchor` (a 1-based line) and highlights it.
pub fn build_preview(path: &Path, anchor: usize, syn_theme: &highlighting::Theme) -> Preview {
    let first_line = anchor.saturating_sub(PREVIEW_CONTEXT).max(1);
    let last_line = anchor.saturating_add(PREVIEW_CONTEXT);

    // Highlighting from the top of the file keeps block comments and strings
    // coloured correctly; for very deep anchors that is too much work, so the
    // window is highlighted on its own. This is also what bounds the number of
    // lines held in memory, to `HIGHLIGHT_FROM_START_LIMIT` at the very worst.
    let highlight_from = if anchor > HIGHLIGHT_FROM_START_LIMIT {
        first_line
    } else {
        1
    };

    let window = match read_window(path, highlight_from, last_line) {
        Ok(window) => window,
        Err(e) => return Preview::Error(format!("Cannot preview file: {e}")),
    };

    let ss = syntax_set();
    let syntax = ss
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, syn_theme);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (idx, raw) in window.lines.iter().enumerate() {
        let num = window.from + idx;
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
        return if window.truncated {
            // The anchor lies past the cap, so there is nothing to centre on.
            // Saying so beats the misleading "(empty file)".
            Preview::Error(format!(
                "(file too large to preview past {} MiB)",
                MAX_PREVIEW_BYTES / (1024 * 1024)
            ))
        } else {
            Preview::Error("(empty file)".to_string())
        };
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

/// A cached preview, what it was built for, and how far it is scrolled.
///
/// The file finder and find-in-files each carried this as three loose fields
/// plus a `syn_theme`, with their own copies of `scroll_preview`,
/// `invalidate_preview` and `ensure_preview` — about fifty identical lines.
/// `K` is whatever identifies the current selection: a path for one popup, a
/// path and line number for the other.
#[derive(Debug, Default)]
pub struct PreviewPane<K> {
    preview: Option<Preview>,
    /// What `preview` was built for. `None` means nothing is cached.
    built_for: Option<K>,
    scroll: i32,
    /// Syntax colours for the active theme. Non-optional: the popups used to
    /// hold `Option<Arc<..>>` purely so they could derive `Default` for tests,
    /// then unwrap it with `unwrap_or_default` at every use.
    syntax: Arc<highlighting::Theme>,
}

impl<K: PartialEq> PreviewPane<K> {
    pub fn new(syntax: Arc<highlighting::Theme>) -> Self {
        Self {
            preview: None,
            built_for: None,
            scroll: 0,
            syntax,
        }
    }

    /// Scrolls without moving the selection. Clamped against the loaded window
    /// at render time.
    pub fn scroll_by(&mut self, delta: i32) {
        self.scroll = self.scroll.saturating_add(delta);
    }

    /// Returns to the natural offset for the current content, without
    /// dropping it — the selection moved, so a manual scroll no longer
    /// applies.
    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
    }

    /// Drops the cache, e.g. because the result list changed underneath it.
    pub fn invalidate(&mut self) {
        self.preview = None;
        self.built_for = None;
        self.scroll = 0;
    }

    /// Builds the preview for `key`, reusing the cached one when the key has
    /// not changed. `None` clears it.
    ///
    /// Rebuilding resets the scroll: the offset belonged to the old content.
    pub fn ensure(
        &mut self,
        key: Option<K>,
        build: impl FnOnce(&K, &highlighting::Theme) -> Preview,
    ) {
        let Some(key) = key else {
            self.preview = None;
            self.built_for = None;
            return;
        };

        if self.built_for.as_ref() == Some(&key) {
            return;
        }

        self.preview = Some(build(&key, &self.syntax));
        self.built_for = Some(key);
        self.scroll = 0;
    }

    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        theme: &Theme,
        area: Rect,
        title: &str,
        anchor: Option<usize>,
    ) {
        let mut scroll = self.scroll;
        render_preview(
            frame,
            theme,
            area,
            title,
            self.preview.as_ref(),
            anchor,
            &mut scroll,
        );
        // `render_preview` clamps against the window it actually drew.
        self.scroll = scroll;
    }

    #[cfg(test)]
    pub fn built_for(&self) -> Option<&K> {
        self.built_for.as_ref()
    }

    #[cfg(test)]
    pub fn scroll(&self) -> i32 {
        self.scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod preview_pane {
        use super::*;

        fn pane() -> PreviewPane<u32> {
            PreviewPane::new(Arc::new(highlighting::Theme::default()))
        }

        fn build(key: &u32, _syntax: &highlighting::Theme) -> Preview {
            Preview::Error(format!("built for {key}"))
        }

        #[test]
        fn the_cache_is_reused_while_the_key_is_unchanged() {
            let mut pane = pane();
            let mut builds = 0;

            for _ in 0..3 {
                pane.ensure(Some(7), |k, t| {
                    builds += 1;
                    build(k, t)
                });
            }

            assert_eq!(builds, 1, "the same key must not rebuild");
            assert_eq!(pane.built_for(), Some(&7));
        }

        #[test]
        fn a_new_key_rebuilds_and_drops_a_manual_scroll() {
            let mut pane = pane();
            pane.ensure(Some(1), build);
            pane.scroll_by(10);
            assert_eq!(pane.scroll(), 10);

            pane.ensure(Some(2), build);
            assert_eq!(pane.built_for(), Some(&2));
            assert_eq!(pane.scroll(), 0, "the offset belonged to the old content");
        }

        #[test]
        fn no_selection_clears_the_cache() {
            let mut pane = pane();
            pane.ensure(Some(1), build);

            pane.ensure(None, build);
            assert_eq!(pane.built_for(), None);
        }

        #[test]
        fn invalidate_drops_the_cache_and_the_scroll() {
            let mut pane = pane();
            pane.ensure(Some(1), build);
            pane.scroll_by(5);

            pane.invalidate();
            assert_eq!(pane.built_for(), None);
            assert_eq!(pane.scroll(), 0);
        }

        #[test]
        fn scrolling_accumulates_in_both_directions() {
            let mut pane = pane();
            pane.scroll_by(10);
            pane.scroll_by(-3);
            assert_eq!(pane.scroll(), 7);
        }
    }

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

    /// Past `HIGHLIGHT_FROM_START_LIMIT` the window is highlighted on its own
    /// rather than from the top of the file, so `window.from` stops being 1
    /// and the gutter arithmetic changes. Pin it against a real file.
    #[test]
    fn a_deep_anchor_still_numbers_its_lines_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep.txt");
        let anchor = HIGHLIGHT_FROM_START_LIMIT + 500;
        let body: String = (1..=anchor + 200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let Preview::Lines { lines, first_line } =
            build_preview(&path, anchor, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };

        assert_eq!(first_line, anchor - PREVIEW_CONTEXT);
        assert_eq!(lines.len(), 2 * PREVIEW_CONTEXT + 1);

        let text_at = |num: usize| -> String {
            lines[num - first_line]
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect()
        };
        assert_eq!(text_at(anchor), format!("line {anchor}"));
        assert_eq!(text_at(first_line), format!("line {first_line}"));
        assert_eq!(
            text_at(anchor + PREVIEW_CONTEXT),
            format!("line {}", anchor + PREVIEW_CONTEXT)
        );
    }

    /// CRLF files must number and strip exactly as LF ones do.
    #[test]
    fn crlf_line_endings_are_stripped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dos.txt");
        std::fs::write(&path, "alpha\r\nbeta\r\ngamma\r\n").unwrap();

        let Preview::Lines { lines, first_line } =
            build_preview(&path, 2, &highlighting::Theme::default())
        else {
            panic!("expected preview lines");
        };

        assert_eq!(first_line, 1);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect())
            .collect();
        assert_eq!(text, vec!["alpha", "beta", "gamma"]);
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
    fn a_file_with_no_newline_is_not_read_past_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob.bin");
        // One "line" larger than the cap: the old `read_to_string` would have
        // pulled all of it into memory.
        let body = "x".repeat(MAX_PREVIEW_BYTES as usize + 4096);
        std::fs::write(&path, &body).unwrap();

        let window = read_window(&path, 1, 1 + PREVIEW_CONTEXT).unwrap();
        assert_eq!(window.lines.len(), 1);
        assert!(
            window.lines[0].len() <= MAX_PREVIEW_BYTES as usize,
            "the read must stop at the cap, got {} bytes",
            window.lines[0].len()
        );
    }

    #[test]
    fn lines_before_the_window_are_walked_past_but_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.txt");
        let body: String = (1..=1_000).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let window = read_window(&path, 900, 950).unwrap();
        assert_eq!(window.from, 900);
        assert_eq!(window.lines.len(), 51, "only the window is held in memory");
        assert_eq!(window.lines[0], "line 900");
        assert_eq!(window.lines[50], "line 950");
        assert!(!window.truncated);
    }

    #[test]
    fn an_anchor_past_the_cap_says_the_file_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.log");
        // Enough short lines to run past the byte cap well before the anchor.
        let line_len = 64;
        let count = (MAX_PREVIEW_BYTES as usize / line_len) + 1_000;
        let mut body = String::with_capacity(count * line_len);
        for _ in 0..count {
            body.push_str(&"y".repeat(line_len - 1));
            body.push('\n');
        }
        std::fs::write(&path, body).unwrap();

        let anchor = count + 500;
        let preview = build_preview(&path, anchor, &highlighting::Theme::default());
        let Preview::Error(msg) = preview else {
            panic!("expected an error for an anchor past the cap");
        };
        assert!(msg.contains("too large"), "unexpected message: {msg}");
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
