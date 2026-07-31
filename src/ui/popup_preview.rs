use ratatui_image::{Image, Resize, picker::Picker};
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Instant;
use syntect::{
    easy::HighlightLines,
    highlighting::{self, Style as SynStyle},
    parsing::SyntaxSet,
};

use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::fs::ops;
use crate::ui::{
    component::{Component, centered_popup},
    panes::{Entry, EntryKind, format_date, format_size},
    theme::Theme,
    uiconfig::UiConfig,
};

/// At most this many entries are listed in archive/directory previews.
const LISTING_LIMIT: usize = 1000;
/// Bytes shown in the binary hex dump.
const HEX_DUMP_BYTES: usize = 256;

/// Upper bounds for the popup, in cells. Code and text stay readable at about
/// a hundred columns; beyond that the popup just swallows the screen.
const MAX_POPUP_WIDTH: u16 = 110;
const MAX_POPUP_HEIGHT: u16 = 50;

enum PreviewContent {
    Text(Text<'static>),
    Image(String),
    Error(String),
    /// Background thread is computing the content; spinner shown meanwhile.
    Loading,
}

pub struct PopupPreview {
    selected: Option<Entry>,
    title: Option<String>,
    row: u16,
    viewport_height: u16,
    /// Syntax colours, built once per theme by the app and shared with the
    /// background loader. `None` for previews that are not file content.
    syn_theme: Option<Arc<highlighting::Theme>>,
    content: Option<PreviewContent>,
    /// Receives content from a background thread for slow previews (e.g. PDF).
    loading_rx: Option<mpsc::Receiver<PreviewContent>>,
    /// When the background load started (drives spinner animation).
    loading_started: Option<Instant>,
    /// Wrap long lines to the popup width. On by default: without it every
    /// line longer than the popup is simply cut off at the right edge, which
    /// hides most of a prose document (README lines run past 300 columns).
    wrap: bool,
}

#[derive(PartialEq, Debug)]
pub enum FileType {
    Unknown(String),
    Binary,
    Ascii,
    Image,
    Archive,
    Pdf,
    Symlink,
    Directory,
}

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

fn syntect_style_to_ratatui(style: SynStyle) -> Style {
    let mut s = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(highlighting::FontStyle::BOLD) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(highlighting::FontStyle::ITALIC) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style
        .font_style
        .contains(highlighting::FontStyle::UNDERLINE)
    {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

impl PopupPreview {
    pub fn new(entry: Option<Entry>, syn_theme: Arc<highlighting::Theme>) -> Self {
        // Parent/unknown entries never reach the filesystem — showing a
        // message instead of a real preview. Critically, this also prevents
        // sizing up the parent directory when navigation wraps to `..` with
        // the preview open.
        let content = match &entry {
            Some(e) if e.kind == EntryKind::Parent => Some(PreviewContent::Text(Text::from(
                "Cannot preview parent directory.",
            ))),
            Some(e) if e.kind == EntryKind::Unknown => Some(PreviewContent::Text(Text::from(
                "Unknown file type — cannot preview.",
            ))),
            _ => None,
        };

        Self {
            selected: entry,
            title: None,
            row: 0,
            viewport_height: 1,
            syn_theme: Some(syn_theme),
            content,
            loading_rx: None,
            loading_started: None,
            wrap: true,
        }
    }

    /// A preview showing arbitrary text (e.g., `:!` shell output) instead of
    /// an entry's content. Not bound to the selection: moving the cursor does
    /// not replace it.
    pub fn from_text(title: String, text: Text<'static>) -> Self {
        Self {
            selected: None,
            title: Some(title),
            row: 0,
            viewport_height: 1,
            syn_theme: None,
            content: Some(PreviewContent::Text(text)),
            loading_rx: None,
            loading_started: None,
            wrap: true,
        }
    }

    /// Returns `true` while a background thread is computing preview content.
    /// The `App` run loop uses this to keep re-rendering at ~50 ms so the
    /// spinner animates without waiting for a keypress.
    pub fn is_loading(&self) -> bool {
        matches!(self.content, Some(PreviewContent::Loading))
    }

    /// Toggles line wrapping; code is sometimes easier to read unwrapped.
    pub fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.row = 0;
    }

    /// Builds the scrollable paragraph for `text`, clamping the scroll offset
    /// to the number of rows the text actually renders to (which depends on
    /// wrapping).
    fn text_paragraph(&mut self, text: &Text<'static>, area: Rect) -> Paragraph<'static> {
        let mut paragraph = Paragraph::new(text.clone());
        if self.wrap {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }

        let rendered_rows = paragraph.line_count(area.width) as u16;
        let max_row = rendered_rows.saturating_sub(area.height);
        self.row = self.row.min(max_row);

        paragraph.scroll((self.row, 0))
    }

    pub fn row_next(&mut self) {
        self.row = self.row.saturating_add(10);
    }

    pub fn row_prev(&mut self) {
        self.row = self.row.saturating_sub(10);
    }

    pub fn page_down(&mut self) {
        self.row = self.row.saturating_add(self.viewport_height);
    }

    pub fn page_up(&mut self) {
        self.row = self.row.saturating_sub(self.viewport_height);
    }

    pub fn half_page_down(&mut self) {
        self.row = self.row.saturating_add(self.viewport_height / 2);
    }

    pub fn half_page_up(&mut self) {
        self.row = self.row.saturating_sub(self.viewport_height / 2);
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.selected.as_ref()
    }

    fn get_file_type(path: &str) -> FileType {
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.is_dir() {
                return FileType::Directory;
            }
            if meta.is_symlink() {
                return FileType::Symlink;
            }
        }

        let lower = path.to_lowercase();
        if lower.ends_with(".pdf") {
            return FileType::Pdf;
        }
        if lower.ends_with(".zip")
            || lower.ends_with(".tar")
            || lower.ends_with(".tar.gz")
            || lower.ends_with(".tgz")
        {
            return FileType::Archive;
        }

        let mut buf = vec![0u8; 8192];
        let n = match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
            Ok(n) => n,
            Err(_) => return FileType::Unknown(String::new()),
        };
        buf.truncate(n);

        match infer::get(&buf).map(|t| t.matcher_type()) {
            Some(infer::MatcherType::Image) => FileType::Image,
            Some(infer::MatcherType::Archive) => FileType::Archive,
            Some(infer::MatcherType::Text) => FileType::Ascii,
            Some(_) => FileType::Binary,
            None => {
                if std::str::from_utf8(&buf).is_ok() {
                    FileType::Ascii
                } else {
                    FileType::Binary
                }
            }
        }
    }

    fn get_file_content(path: &str, syn_theme: &highlighting::Theme) -> PreviewContent {
        match Self::get_file_type(path) {
            FileType::Ascii => Self::text_preview(path, syn_theme),
            FileType::Image => PreviewContent::Image(path.to_string()),
            FileType::Directory => PopupPreview::directory_preview(path),
            FileType::Archive => Self::archive_preview(path),
            FileType::Pdf => Self::pdf_preview(path),
            FileType::Binary => Self::binary_preview(path),
            FileType::Symlink => Self::symlink_preview(path),
            FileType::Unknown(msg) => PreviewContent::Error(format!("Cannot read file. {msg}")),
        }
    }

    fn text_preview(path: &str, syn_theme: &highlighting::Theme) -> PreviewContent {
        let ss = SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines);
        let syntax = ss
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, syn_theme);

        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => return PreviewContent::Error(format!("Cannot open file: {e}")),
        };

        let reader = io::BufReader::new(file);
        let mut lines: Vec<Line> = Vec::new();

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            let regions = match highlighter.highlight_line(&line, ss) {
                Ok(r) => r,
                Err(_) => {
                    lines.push(Line::from(Span::raw(line)));
                    continue;
                }
            };
            let spans: Vec<Span> = regions
                .iter()
                .map(|(style, text)| {
                    Span::styled(text.to_string(), syntect_style_to_ratatui(*style))
                })
                .collect();
            lines.push(Line::from(spans));
        }

        PreviewContent::Text(Text::from(lines))
    }

    fn directory_preview(path: &str) -> PreviewContent {
        let dir = Path::new(path);
        let mut files = 0usize;
        let mut dirs = 0usize;
        let mut children: Vec<(String, bool)> = Vec::new();

        match std::fs::read_dir(dir) {
            Ok(rd) => {
                for entry in rd.filter_map(|e| e.ok()) {
                    let is_dir = entry.path().is_dir();
                    if is_dir {
                        dirs += 1;
                    } else {
                        files += 1;
                    }
                    if children.len() < LISTING_LIMIT {
                        children.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
                    }
                }
            }
            Err(e) => {
                return PreviewContent::Error(format!("Cannot read directory: {e}"));
            }
        }

        children.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        // Capped walk: a huge tree must not block the UI. Transfers use the
        // exact ops::total_size instead.
        const SIZE_WALK_LIMIT: u64 = 50_000;
        let estimate = ops::total_size_capped(&[dir.to_path_buf()], SIZE_WALK_LIMIT);
        let size_text = if estimate.truncated {
            format!("≥ {} (partial)", format_size(estimate.bytes))
        } else {
            format_size(estimate.bytes)
        };

        let mut lines = vec![
            Line::from(format!("Total size: {size_text}")),
            Line::from(format!("{files} files, {dirs} directories")),
            Line::from(""),
        ];
        for (name, is_dir) in children {
            lines.push(Line::from(if is_dir { format!("{name}/") } else { name }));
        }
        if files + dirs > LISTING_LIMIT {
            lines.push(Line::from(format!(
                "… ({} more entries)",
                files + dirs - LISTING_LIMIT
            )));
        }

        PreviewContent::Text(Text::from(lines))
    }

    fn archive_preview(path: &str) -> PreviewContent {
        let lower = path.to_lowercase();
        let entries = if lower.ends_with(".zip") {
            PopupPreview::zip_listing(path)
        } else if lower.ends_with(".tar") {
            PopupPreview::tar_listing(path, false)
        } else {
            PopupPreview::tar_listing(path, true) // .tar.gz / .tgz
        };

        match entries {
            Ok(list) => {
                let mut lines: Vec<Line> = list
                    .iter()
                    .take(LISTING_LIMIT)
                    .map(|(name, size)| {
                        Line::from(format!("{:<60} {:>10}", name, format_size(*size)))
                    })
                    .collect();
                if list.len() > LISTING_LIMIT {
                    lines.push(Line::from(format!(
                        "… ({} more entries)",
                        list.len() - LISTING_LIMIT
                    )));
                }
                if lines.is_empty() {
                    lines.push(Line::from("(empty archive)"));
                }
                PreviewContent::Text(Text::from(lines))
            }
            Err(e) => PreviewContent::Error(format!("Cannot read archive: {e}")),
        }
    }

    fn zip_listing(path: &str) -> io::Result<Vec<(String, u64)>> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            entries.push((entry.name().to_string(), entry.size()));
        }
        Ok(entries)
    }

    fn tar_listing(path: &str, gzipped: bool) -> io::Result<Vec<(String, u64)>> {
        let file = std::fs::File::open(path)?;
        let reader: Box<dyn io::Read> = if gzipped {
            Box::new(flate2::read::GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        let mut archive = tar::Archive::new(reader);
        let mut entries = Vec::new();
        for entry in archive.entries()? {
            let entry = entry?;
            let name = entry.path()?.to_string_lossy().into_owned();
            entries.push((name, entry.header().size()?));
        }
        Ok(entries)
    }

    fn pdf_preview(path: &str) -> PreviewContent {
        match pdf_extract::extract_text(path) {
            Ok(text) => {
                let lines: Vec<Line> = text.lines().map(|l| Line::from(l.to_string())).collect();
                if lines.is_empty() {
                    PreviewContent::Text(Text::from("(no extractable text)"))
                } else {
                    PreviewContent::Text(Text::from(lines))
                }
            }
            Err(e) => PreviewContent::Error(format!("Cannot extract PDF text: {e}")),
        }
    }

    fn binary_preview(path: &str) -> PreviewContent {
        let mut lines: Vec<Line> = Vec::new();

        if let Ok(meta) = std::fs::metadata(path) {
            lines.push(Line::from(format!("Size: {}", format_size(meta.len()))));
            lines.push(Line::from(format!(
                "Modified: {}",
                meta.modified()
                    .map(format_date)
                    .unwrap_or_else(|_| "-".to_string())
            )));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                lines.push(Line::from(format!(
                    "Permissions: {:04o}",
                    meta.permissions().mode() & 0o7777
                )));
            }
        }

        if let Ok(Some(kind)) = infer::get_from_path(path) {
            lines.push(Line::from(format!("MIME: {}", kind.mime_type())));
        }

        match std::fs::File::open(path) {
            Ok(mut f) => {
                let mut buf = vec![0u8; HEX_DUMP_BYTES];
                match f.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        lines.push(Line::from(""));
                        lines.push(Line::from(format!("Hex dump (first {n} bytes):")));
                        lines.extend(hex_dump(&buf));
                    }
                    Err(e) => lines.push(Line::from(format!("Cannot read: {e}"))),
                }
            }
            Err(e) => lines.push(Line::from(format!("Cannot open: {e}"))),
        }

        PreviewContent::Text(Text::from(lines))
    }

    fn symlink_preview(path: &str) -> PreviewContent {
        let mut lines: Vec<Line> = Vec::new();
        match std::fs::read_link(path) {
            Ok(target) => {
                lines.push(Line::from(format!("Symlink to: {}", target.display())));
                let exists = Path::new(path).exists(); // follows the link
                lines.push(Line::from(if exists {
                    "Target exists."
                } else {
                    "Target is MISSING (broken link)."
                }));
            }
            Err(e) => lines.push(Line::from(format!("Cannot read link: {e}"))),
        }
        PreviewContent::Text(Text::from(lines))
    }
}

fn hex_dump(bytes: &[u8]) -> Vec<Line<'static>> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let offset = i * 16;
            let hex = chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii: String = chunk
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() || *b == b' ' {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            Line::from(format!("{offset:08x}  {hex:<48}  |{ascii}|"))
        })
        .collect()
}

impl Component for PopupPreview {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        // 60% of the screen, but never wider than a comfortable reading
        // measure — on an ultrawide that would otherwise be 120+ columns.
        let popup_area = centered_popup(
            area,
            (area.width * 3 / 5, area.height * 9 / 10),
            (30, 5),
            (MAX_POPUP_WIDTH, MAX_POPUP_HEIGHT),
        );

        frame.render_widget(Clear, popup_area);

        let title = match (&self.title, &self.selected) {
            (Some(t), _) => t.clone(),
            (None, Some(entry)) => format!("Preview {}", entry.name),
            (None, None) => return,
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().bg(theme.colors.background()))
            .style(Style::default().fg(theme.colors.foreground()));

        let inner_area = block.inner(popup_area);
        self.viewport_height = inner_area.height.max(1);
        frame.render_widget(block, popup_area);

        // Text previews from from_text are complete; entry previews resolve
        // their content lazily from the filesystem.
        if self.selected.is_none() {
            if let Some(PreviewContent::Text(text)) = self.content.as_ref() {
                let paragraph = self.text_paragraph(&text.clone(), inner_area);
                frame.render_widget(paragraph, inner_area);
            }
            return;
        }

        let Some(entry) = self.selected.as_ref() else {
            return;
        };
        let path = entry.path.as_os_str().to_string_lossy();

        // First time: decide how to load content.
        if self.content.is_none() {
            let file_type = Self::get_file_type(&path);
            match file_type {
                FileType::Image => {
                    // ratatui-image decodes the file lazily during render —
                    // just storing the path is instant.
                    self.content = Some(PreviewContent::Image(path.to_string()));
                }
                FileType::Binary | FileType::Symlink => {
                    // These are always fast (256-byte hex dump / symlink read)
                    // so there is no perceptible delay worth a spinner.
                    let syn_theme = self.syn_theme.clone().unwrap_or_default();
                    self.content = Some(Self::get_file_content(&path, &syn_theme));
                }
                _ => {
                    // Text (syntax highlighting), archive listing, directory
                    // size walk, and PDF extraction can all take noticeable
                    // time on large inputs — offload every one of them so the
                    // popup opens instantly with a spinner.
                    let path_owned = path.to_string();
                    let syn_theme = self.syn_theme.clone().unwrap_or_default();
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(Self::get_file_content(&path_owned, &syn_theme));
                    });
                    self.loading_rx = Some(rx);
                    self.loading_started = Some(Instant::now());
                    self.content = Some(PreviewContent::Loading);
                }
            }
        }

        // If a background load is in progress, poll for completion.
        if matches!(self.content, Some(PreviewContent::Loading))
            && let Some(rx) = &self.loading_rx
            && let Ok(ready) = rx.try_recv()
        {
            self.content = Some(ready);
            self.loading_rx = None;
            self.loading_started = None;
        }

        // Render spinner while loading.
        if matches!(self.content, Some(PreviewContent::Loading)) {
            const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let elapsed = self
                .loading_started
                .map(|s| s.elapsed().as_millis())
                .unwrap_or(0);
            let spin_frame = ((elapsed / 80) as usize) % SPINNER.len();
            let spinner = SPINNER[spin_frame];
            let bg_fill = Block::default().style(Style::default().bg(theme.colors.background()));
            frame.render_widget(bg_fill, inner_area);
            let msg = Paragraph::new(format!("{spinner} Loading preview…"))
                .style(Style::default().fg(theme.colors.muted()));
            frame.render_widget(msg, inner_area);
            return;
        }

        let Some(content) = self.content.as_ref() else {
            return;
        };

        if !matches!(content, PreviewContent::Image(_)) {
            let bg_fill = Block::default().style(Style::default().bg(theme.colors.background()));
            frame.render_widget(bg_fill, inner_area);
        }

        match content {
            PreviewContent::Text(text) => {
                let paragraph = self.text_paragraph(&text.clone(), inner_area);
                frame.render_widget(paragraph, inner_area);
            }
            PreviewContent::Image(path) => {
                let dyn_img = match image::ImageReader::open(path) {
                    Ok(reader) => match reader.decode() {
                        Ok(img) => img,
                        Err(e) => {
                            frame.render_widget(
                                Paragraph::new(format!("Image decode error: {e}")),
                                inner_area,
                            );
                            return;
                        }
                    },
                    Err(e) => {
                        frame.render_widget(
                            Paragraph::new(format!("Cannot open image: {e}")),
                            inner_area,
                        );
                        return;
                    }
                };

                let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
                let size = Size::from(inner_area);
                let image = match picker.new_protocol(dyn_img, size, Resize::Fit(None)) {
                    Ok(protocol) => protocol,
                    Err(e) => {
                        frame.render_widget(
                            Paragraph::new(format!("protocol error: {e}")),
                            inner_area,
                        );
                        return;
                    }
                };
                let image = Image::new(&image);
                frame.render_widget(image, inner_area);
            }
            PreviewContent::Error(msg) => {
                frame.render_widget(
                    Paragraph::new(msg.as_str()).wrap(Wrap { trim: true }),
                    inner_area,
                );
            }
            // Loading is handled above with an early return; unreachable here.
            PreviewContent::Loading => {}
        }
    }
}

impl std::fmt::Debug for PopupPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content_label = match &self.content {
            None => "None",
            Some(PreviewContent::Text(_)) => "Some(Text)",
            Some(PreviewContent::Image(_)) => "Some(Image)",
            Some(PreviewContent::Error(_)) => "Some(Error)",
            Some(PreviewContent::Loading) => "Some(Loading)",
        };
        f.debug_struct("PopupPreview")
            .field("selected", &self.selected)
            .field("title", &self.title)
            .field("row", &self.row)
            .field("content", &content_label)
            .field("is_loading", &self.is_loading())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Syntax colours for tests that never actually highlight anything.
    fn test_syn_theme() -> Arc<highlighting::Theme> {
        Arc::new(highlighting::Theme::default())
    }
    use std::io::Write;

    #[test]
    fn hex_dump_formats_offset_hex_and_ascii() {
        let lines = hex_dump(b"Hello\0World");
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.starts_with("00000000"));
        assert!(text.contains("48 65 6c 6c 6f 00 57 6f 72 6c 64"));
        assert!(text.ends_with("|Hello.World|"));
    }

    #[test]
    fn hex_dump_splits_into_16_byte_rows() {
        let lines = hex_dump(&[0u8; 32]);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn directory_preview_shows_size_and_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("a.txt")).unwrap();
        write!(f, "12345").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let PreviewContent::Text(text) =
            PopupPreview::directory_preview(dir.path().to_str().unwrap())
        else {
            panic!("expected text content");
        };

        let all: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("Total size: 5 B"));
        assert!(all.contains("1 files, 1 directories"));
        assert!(all.contains("a.txt"));
        assert!(all.contains("sub/"));
    }

    #[test]
    fn zip_listing_reads_entry_names_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("hello.txt", options).unwrap();
            zip.write_all(b"hello world").unwrap();
            zip.finish().unwrap();
        }

        let entries = PopupPreview::zip_listing(zip_path.to_str().unwrap()).unwrap();
        assert_eq!(entries, vec![("hello.txt".to_string(), 11)]);
    }

    #[test]
    fn tar_listing_reads_entry_names_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("test.tar");
        let src_file = dir.path().join("data.txt");
        let mut f = std::fs::File::create(&src_file).unwrap();
        write!(f, "123456").unwrap();
        {
            let file = std::fs::File::create(&tar_path).unwrap();
            let mut builder = tar::Builder::new(file);
            builder
                .append_path_with_name(&src_file, "data.txt")
                .unwrap();
            builder.finish().unwrap();
        }

        let entries = PopupPreview::tar_listing(tar_path.to_str().unwrap(), false).unwrap();
        assert_eq!(entries, vec![("data.txt".to_string(), 6)]);
    }

    #[test]
    fn parent_entry_gets_message_content_without_fs_access() {
        let dir = tempfile::tempdir().unwrap();
        let entry = Entry::parent(dir.path().to_str().unwrap());
        let preview = PopupPreview::new(Some(entry), test_syn_theme());

        // Content is pre-set (message), never computed from the filesystem.
        assert!(preview.content.is_some());
        let Some(PreviewContent::Text(text)) = preview.content.as_ref() else {
            panic!("expected text content");
        };
        let all: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(all.contains("Cannot preview parent directory."));
    }

    #[test]
    fn regular_entry_has_lazy_content() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let preview =
            PopupPreview::new(Some(Entry::new(tmp.path().to_path_buf())), test_syn_theme());

        assert!(preview.content.is_none());
    }

    #[test]
    fn wrapping_bounds_the_scroll_by_rendered_rows() {
        // One very long logical line: unwrapped it is a single row and there is
        // nothing to scroll, wrapped it is many rows.
        let text = Text::from(vec![Line::from("word ".repeat(40))]);
        let area = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 5,
        };

        let mut preview = PopupPreview::from_text("t".to_string(), text.clone());
        preview.row = 500;
        let _ = preview.text_paragraph(&text, area);
        let wrapped_max = preview.row;
        assert!(
            wrapped_max > 0,
            "wrapped text must be scrollable past the first screen"
        );
        assert!(wrapped_max < 500, "scroll must be clamped to the content");

        // Without wrapping the same text fits on one row: nothing to scroll.
        preview.wrap = false;
        preview.row = 500;
        let _ = preview.text_paragraph(&text, area);
        assert_eq!(preview.row, 0);
    }

    #[test]
    fn toggling_wrap_returns_to_the_top() {
        let mut preview = PopupPreview::from_text("t".to_string(), Text::from("x"));
        preview.row = 12;
        preview.toggle_wrap();
        assert!(!preview.wrap);
        assert_eq!(preview.row, 0);
    }

    #[test]
    fn paging_moves_by_viewport() {
        let mut preview = PopupPreview::new(None, test_syn_theme());
        preview.viewport_height = 20;

        preview.page_down();
        assert_eq!(preview.row, 20);
        preview.half_page_down();
        assert_eq!(preview.row, 30);
        preview.half_page_up();
        assert_eq!(preview.row, 20);
        preview.page_up();
        assert_eq!(preview.row, 0);
    }
}
