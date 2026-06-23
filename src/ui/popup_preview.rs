use ansi_to_tui::IntoText;
use ratatui_image::{Image, Resize, picker::Picker};
use std::io;
use std::io::BufRead;
use std::ops::Add;
use std::process::Command;
use syntect::highlighting;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;
use syntect::{easy::HighlightFile, highlighting::Highlighter};

use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::Style,
    text::{Line, Text},
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
}

#[derive(PartialEq, Debug)]
pub enum FileType {
    Unknown(String),
    Binary,
    ASCII,
    Image,
    Archive,
    Symlink,
    Directory,
}

impl PopupPreview {
    pub fn new(entry: Option<Entry>) -> Self {
        Self {
            selected: entry,
            row: 0,
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
        let Ok(output) = Command::new("file").arg(path).output() else {
            return FileType::Unknown(String::new());
        };

        let Ok(stdout) = String::from_utf8(output.stdout) else {
            return FileType::Binary;
        };

        if stdout.contains("text") {
            FileType::ASCII
        } else if stdout.contains("directory") {
            FileType::Directory
        } else if stdout.contains("archive") {
            FileType::Archive
        } else if stdout.contains("image data") || stdout.to_ascii_lowercase().contains("web/p") {
            FileType::Image
        } else if stdout.contains("symbolic link") {
            FileType::Symlink
        } else {
            FileType::Unknown(stdout)
            // todo!("Not yet implemented. file cmd output: {}", stdout);
        }
    }

    fn syntect_style_to_ratatui(s: highlighting::Style) -> ratatui::style::Style {
        use ratatui::style::{Color, Modifier, Style};
        let mut style = Style::default()
            .fg(Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b))
            .bg(Color::Rgb(s.background.r, s.background.g, s.background.b));
        if s.font_style.contains(highlighting::FontStyle::BOLD) {
            style = style.add_modifier(Modifier::BOLD);
        }
        if s.font_style.contains(highlighting::FontStyle::ITALIC) {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if s.font_style.contains(highlighting::FontStyle::UNDERLINE) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }
    fn get_file_content(path: &str) -> PreviewContent {
        match Self::get_file_type(path) {
            FileType::ASCII => {
                let ss = SyntaxSet::load_defaults_newlines();
                let ts = highlighting::ThemeSet::load_defaults();
                let mut highlighter =
                    HighlightFile::new(path, &ss, &ts.themes["base16-ocean.dark"]).unwrap();
                let mut line = String::new();
                let mut lines: Vec<Line> = Vec::new();
                let mut ansi_string = String::new();

                loop {
                    match highlighter.reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }

                    let regions = match highlighter.highlight_lines.highlight_line(&line, &ss) {
                        Ok(r) => r,
                        // cannot parse the line, fallback to non-parsed line
                        Err(_) => {
                            lines.push(Line::from(line.clone()));
                            line.clear();
                            continue;
                        }
                    };

                    ansi_string.push_str(&as_24_bit_terminal_escaped(&regions[..], true));
                    line.clear();
                }

                // while highlighter.reader.read_line(&mut line) > 0 {
                //     {
                //         let regions: Vec<(highlighting::Style, &str)> = highlighter
                //             .highlight_lines
                //             .highlight_line(&line, &ss)
                //             .unwrap();
                //         ansi_string.push_str(&as_24_bit_terminal_escaped(&regions[..], true));
                //         // print!("{}", as_24_bit_terminal_escaped(&regions[..], true));
                //     } // until NLL this scope is needed so we can clear the buffer after
                //     line.clear(); // read_line appends so we need to clear between lines
                // }

                match ansi_string.into_text() {
                    Ok(t) => PreviewContent::Text(t),
                    Err(_) => PreviewContent::Error("cannot convert string to tui text".into()),
                }
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

        let content = Self::get_file_content(&path);

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
