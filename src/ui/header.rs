use std::process::Command;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{component::Component, theme::Theme, uiconfig::UiConfig};

#[derive(Default, Debug, Clone)]
struct GitStatus {
    pub branch: String,
    pub modified: i32,
    pub untracked: i32,
}

impl GitStatus {
    pub fn try_from_path(path: &String) -> Option<Self> {
        Command::new("git")
            .args(["-C", path, "rev-parse", "--is-inside-work-tree"])
            .output()
            .ok()?;

        let branch = Command::new("git")
            .args(["-C", path, "branch", "--show-current"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|f| !f.is_empty())?;

        let status = Command::new("git")
            .args(["-C", path, "status", "--porcelain"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;

        let mut modified = 0;
        let mut untracked = 0;
        for line in status.lines() {
            if line.is_empty() {
                continue;
            }
            if line.starts_with("??") {
                untracked += 1;
            } else {
                modified += 1;
            }
        }

        Some(Self {
            branch,
            modified,
            untracked,
        })
    }
}

#[derive(Debug, Default)]
pub struct Header {
    pub info: String,
    pub directory: String,
    git_status: Option<GitStatus>,
}

impl Header {
    pub fn new(info: impl Into<String>, directory: impl Into<String>) -> Self {
        Self {
            info: info.into(),
            directory: directory.into(),
            git_status: None,
        }
    }

    pub fn update(&mut self, directory: String) {
        self.directory = directory;

        self.git_status = self.update_git(&self.directory);
    }

    fn update_git(&self, path: &String) -> Option<GitStatus> {
        GitStatus::try_from_path(path)
    }

    fn git_to_line(&self, theme: &Theme) -> Line<'static> {
        match &self.git_status {
            Some(g) => {
                let branch = Span::from(g.branch.clone()).style(theme.colors.primary());
                let mut spans: Vec<Span<'static>> =
                    vec![Span::from("git:").style(theme.colors.muted()), branch];

                if g.modified > 0 {
                    spans.push(
                        Span::from(format!(" !{}", g.modified)).style(theme.colors.warning()),
                    );
                }

                if g.untracked > 0 {
                    spans.push(Span::from(format!(" ?{}", g.untracked)).style(theme.colors.info()))
                }

                Line::from(spans)
            }
            None => Line::from(vec![]),
        }
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
            Paragraph::new("")
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Center)
                .style(Style::default().fg(theme.colors.foreground())),
            layout[1],
        );

        frame.render_widget(
            Paragraph::new(self.git_to_line(theme))
                .alignment(HorizontalAlignment::Right)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.muted())),
            layout[2],
        );
    }
}
