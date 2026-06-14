use std::{path::Path, process::Command};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Debug, Default)]
pub struct Header {
    pub info: String,
    pub directory: String,
    pub git_status: String,
}

impl Header {
    pub fn new(
        info: impl Into<String>,
        directory: impl Into<String>,
        git_status: impl Into<String>,
    ) -> Self {
        Self {
            info: info.into(),
            directory: directory.into(),
            git_status: git_status.into(),
        }
    }

    pub fn update(&mut self, directory: String) {
        self.directory = directory;

        let path = Path::new(&self.directory);
        if !path.join(".git").exists() {
            self.git_status = String::new();
            return;
        }

        let branch = Command::new("git")
            .args(["-C", &self.directory, "branch", "--show-current"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            })
            .unwrap_or_else(|| String::new());

        let branch = if !branch.is_empty() {
            branch
        } else {
            Command::new("git")
                .args(["-C", &self.directory, "rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        };

        let status_output = match Command::new("git")
            .args(["-C", &self.directory, "status", "--porcelain"])
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => {
                self.git_status = String::new();
                return;
            }
        };

        let mut modified = 0u32;
        let mut untracked = 0u32;

        for line in status_output.lines() {
            if line.is_empty() {
                continue;
            }
            if line.starts_with("??") {
                untracked += 1;
            } else {
                modified += 1;
            }
        }

        let mut parts = Vec::new();
        parts.push(format!("git:{}", branch));
        if modified > 0 {
            parts.push(format!("!{}", modified));
        }
        if untracked > 0 {
            parts.push(format!("?{}", untracked));
        }

        self.git_status = parts.join(" ");
    }
}

impl Component for Header {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, _ui: &UiConfig, area: Rect) {
        let bg_block = Block::default().style(Style::default().bg(theme.colors.surface()));
        let inner_area = bg_block.inner(area);
        frame.render_widget(bg_block, area);

        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .flex(ratatui::layout::Flex::SpaceBetween)
            .constraints([
                Constraint::Fill(1),
                Constraint::Percentage(50),
                Constraint::Fill(1),
            ])
            .split(inner_area);

        frame.render_widget(
            Paragraph::new(&*self.info)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.foreground())),
            layout[0],
        );
        frame.render_widget(
            Paragraph::new(&*self.directory)
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Center)
                .style(Style::default().fg(theme.colors.foreground())),
            layout[1],
        );
        frame.render_widget(
            Paragraph::new(&*self.git_status)
                .alignment(HorizontalAlignment::Right)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.muted())),
            layout[2],
        );
    }
}
