use ansi_to_tui::IntoText as _;
use ratatui_image::{Image, Resize, picker::Picker};
use std::process::Command;

use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::Style,
    text::Text,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::ui::{component::Component, panes::Entry, theme::Theme, uiconfig::UiConfig};

enum PreviewContent {
    Text(Text<'static>),
    Image(String),
    NotImplemented(String),
}

#[derive(Debug)]
pub struct PopupPreview {
    entry: Entry,
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
    pub fn new(entry: Entry) -> Self {
        Self { entry: entry }
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

    fn get_file_content(path: &str) -> PreviewContent {
        match Self::get_file_type(path) {
            FileType::ASCII => {
                let cmd = Command::new("bat")
                    .args(["--plain", "--color=always", "--theme=ansi", path])
                    .output()
                    .unwrap();

                if cmd.status.success() {
                    PreviewContent::Text(cmd.stdout.into_text().unwrap())
                } else {
                    PreviewContent::Text(cmd.stderr.into_text().unwrap())
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

        let block = Block::default()
            .title(format!("Preview {}", self.entry.name))
            .borders(Borders::ALL)
            .border_style(Style::new().bg(theme.colors.background()))
            .style(Style::default().fg(theme.colors.foreground()));

        let path = self.entry.path.as_os_str().to_string_lossy();

        let inner_area = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let content = Self::get_file_content(&path);

        if !matches!(content, PreviewContent::Image(_)) {
            let bg_fill = Block::default().style(Style::default().bg(theme.colors.background()));
            frame.render_widget(bg_fill, inner_area);
        }

        match content {
            PreviewContent::Text(text) => {
                frame.render_widget(Paragraph::new(text), inner_area);
            }
            PreviewContent::Image(path) => {
                let dyn_img = image::ImageReader::open(path).unwrap().decode().unwrap();

                let picker = Picker::halfblocks();

                let font_size = picker.font_size();
                let size = Size::new(
                    dyn_img.width().div_ceil(font_size.width as u32) as u16,
                    dyn_img.height().div_ceil(font_size.height as u32) as u16,
                );

                let image = picker
                    .new_protocol(dyn_img, size, Resize::Fit(None))
                    .unwrap();

                let image = Image::new(&image);

                frame.render_widget(image, inner_area);
            }
            PreviewContent::NotImplemented(msg) => {
                frame.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), inner_area);
            }
        }
    }
}
