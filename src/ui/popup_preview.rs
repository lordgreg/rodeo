use ratatui_image::{Image, Resize, picker::Picker};
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::sync::OnceLock;
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

use crate::ui::{component::Component, panes::Entry, theme::Theme, uiconfig::UiConfig};

enum PreviewContent {
    Text(Text<'static>),
    Image(String),
    Error(String),
    NotImplemented(String),
}

#[derive(Debug)]
pub struct PopupPreview {
    selected: Option<Entry>,
    row: u16,
    syn_theme: Option<highlighting::Theme>,
}

#[derive(PartialEq, Debug)]
pub enum FileType {
    Unknown(String),
    Binary,
    Ascii,
    Image,
    Archive,
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
    pub fn new(entry: Option<Entry>) -> Self {
        Self {
            selected: entry,
            row: 0,
            syn_theme: None,
        }
    }

    pub fn row_next(&mut self) {
        self.row = self.row.saturating_add(10);
    }

    pub fn row_prev(&mut self) {
        self.row = self.row.saturating_sub(10);
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
            FileType::Ascii => {
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
            FileType::Binary => PreviewContent::NotImplemented("BINARY".to_string()),
            FileType::Image => PreviewContent::Image(path.to_string()),
            default => {
                PreviewContent::NotImplemented(format!("not implemented yet. {:?}", default))
            }
        }
    }
}

impl Component for PopupPreview {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let width = area.width * 3 / 5;
        let height = area.height * 9 / 10;
        let popup_area = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };

        frame.render_widget(Clear, popup_area);

        let Some(entry) = self.selected.as_ref() else {
            return;
        };

        let block = Block::default()
            .title(format!("Preview {}", entry.name))
            .borders(Borders::ALL)
            .border_style(Style::new().bg(theme.colors.background()))
            .style(Style::default().fg(theme.colors.foreground()));

        let path = entry.path.as_os_str().to_string_lossy();

        let inner_area = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let syn_theme = self
            .syn_theme
            .get_or_insert_with(|| theme.to_syntect_theme());
        let content = Self::get_file_content(&path, syn_theme);

        if !matches!(content, PreviewContent::Image(_)) {
            let bg_fill = Block::default().style(Style::default().bg(theme.colors.background()));
            frame.render_widget(bg_fill, inner_area);
        }

        match content {
            PreviewContent::Text(text) => {
                let line_count = text.lines.len() as u16;
                let max_row = line_count.saturating_sub(inner_area.height);
                self.row = self.row.min(max_row);
                frame.render_widget(Paragraph::new(text).scroll((self.row, 0)), inner_area);
            }
            PreviewContent::Image(path) => {
                let dyn_img = match image::ImageReader::open(&path) {
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
            PreviewContent::NotImplemented(msg) | PreviewContent::Error(msg) => {
                frame.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), inner_area);
            }
        }
    }
}
