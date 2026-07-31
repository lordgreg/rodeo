use std::process::Command;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{
    component::Component,
    panes::{PaneStats, format_size},
    theme::Theme,
    uiconfig::UiConfig,
};

/// Ellipsis used when the breadcrumb does not fit.
const ELLIPSIS: &str = "…/";

#[derive(Default, Debug, Clone)]
struct GitStatus {
    pub branch: String,
    pub modified: i32,
    pub untracked: i32,
}

impl GitStatus {
    pub fn try_from_path(path: &str) -> Option<Self> {
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
    pub directory: String,
    stats: Option<PaneStats>,
    git_status: Option<GitStatus>,
    /// Bytes available on the filesystem holding `directory`. Sampled when the
    /// directory changes, not per frame.
    free_space: Option<u64>,
}

impl Header {
    pub fn new(directory: impl Into<String>) -> Self {
        let mut header = Self {
            directory: directory.into(),
            stats: None,
            git_status: None,
            free_space: None,
        };
        header.free_space = free_space(&header.directory);
        header.git_status = GitStatus::try_from_path(&header.directory);
        header
    }

    pub fn set_stats(&mut self, stats: PaneStats) {
        self.stats = Some(stats);
    }

    fn stats_to_line(&self, theme: &Theme) -> Line<'static> {
        let Some(s) = &self.stats else {
            return Line::from(vec![]);
        };

        let mut spans: Vec<Span<'static>> = vec![];

        if s.selected > 0 {
            spans.push(Span::from(format!("●{}  ", s.selected)).style(theme.colors.primary()));
        }

        spans.push(Span::from(format!("{} files  ", s.files)).style(theme.colors.muted()));
        spans.push(Span::from(format!("{} dirs", s.dirs)).style(theme.colors.muted()));

        if s.hidden > 0 {
            spans.push(Span::from(format!("  {} hidden", s.hidden)).style(theme.colors.warning()));
        }

        Line::from(spans)
    }

    pub fn update(&mut self, directory: String) {
        self.directory = directory;

        self.git_status = self.update_git(&self.directory);
        self.free_space = free_space(&self.directory);
    }

    /// The active path as a breadcrumb: separators and parents muted, the
    /// directory you are actually in emphasised. Truncated from the left,
    /// because the tail is the part that matters.
    fn breadcrumb(&self, theme: &Theme, width: u16) -> Line<'static> {
        let segments: Vec<&str> = self
            .directory
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let Some((last, parents)) = segments.split_last() else {
            return Line::from(Span::styled(
                "/",
                Style::default().fg(theme.colors.primary()),
            ));
        };

        // Drop leading parents until the whole thing fits.
        let width = width as usize;
        let last_width = last.chars().count();
        let mut skip = 0;
        loop {
            let shown = &parents[skip..];
            let parents_width: usize =
                shown.iter().map(|p| p.chars().count() + 1).sum::<usize>() + 1;
            let prefix_width = if skip > 0 {
                ELLIPSIS.chars().count()
            } else {
                0
            };
            if prefix_width + parents_width + last_width <= width || skip == parents.len() {
                break;
            }
            skip += 1;
        }

        let mut spans = Vec::new();
        if skip > 0 {
            spans.push(Span::styled(
                ELLIPSIS.to_string(),
                Style::default().fg(theme.colors.muted()),
            ));
        } else {
            spans.push(Span::styled(
                "/".to_string(),
                Style::default().fg(theme.colors.muted()),
            ));
        }
        for parent in &parents[skip..] {
            spans.push(Span::styled(
                format!("{parent}/"),
                Style::default().fg(theme.colors.muted()),
            ));
        }
        spans.push(Span::styled(
            (*last).to_string(),
            Style::default().fg(theme.colors.primary()).bold(),
        ));

        Line::from(spans)
    }

    /// Free space on the device, so a copy that cannot fit is obvious before
    /// starting it.
    fn free_space_to_line(&self, theme: &Theme) -> Option<Span<'static>> {
        let free = self.free_space?;
        Some(Span::styled(
            format!("{} free", format_size(free)),
            Style::default().fg(theme.colors.muted()),
        ))
    }

    fn update_git(&self, path: &str) -> Option<GitStatus> {
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
            Paragraph::new(self.stats_to_line(theme))
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.foreground())),
            layout[0],
        );
        // The middle third used to render an empty string; it now says where
        // you actually are.
        frame.render_widget(
            Paragraph::new(self.breadcrumb(theme, layout[1].width.saturating_sub(2)))
                .block(Block::default().padding(Padding::horizontal(1)))
                .alignment(HorizontalAlignment::Center)
                .style(Style::default().fg(theme.colors.foreground())),
            layout[1],
        );

        let mut right = self.git_to_line(theme);
        if let Some(free) = self.free_space_to_line(theme) {
            if !right.spans.is_empty() {
                right.spans.push(Span::styled(
                    "  ",
                    Style::default().fg(theme.colors.muted()),
                ));
            }
            right.spans.push(free);
        }

        frame.render_widget(
            Paragraph::new(right)
                .alignment(HorizontalAlignment::Right)
                .block(Block::default().padding(Padding::horizontal(1)))
                .style(Style::default().fg(theme.colors.muted())),
            layout[2],
        );
    }
}

/// Bytes available to unprivileged users on the filesystem holding `path`.
///
/// Uses `statvfs` directly: `libc` is already in the dependency tree, and the
/// alternative (spawning `df`) would cost a process per navigation.
#[cfg(unix)]
fn free_space(path: &str) -> Option<u64> {
    use std::ffi::CString;

    let c_path = CString::new(path).ok()?;
    // SAFETY: c_path is a valid NUL-terminated string for the duration of the
    // call, stat is a properly sized zeroed statvfs, and the return code is
    // checked before any field is read.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }

    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space(_path: &str) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn header_at(path: &str) -> Header {
        Header {
            directory: path.to_string(),
            stats: None,
            git_status: None,
            free_space: None,
        }
    }

    #[test]
    fn breadcrumb_shows_the_whole_path_when_it_fits() {
        let theme = Theme::builtin().unwrap();
        let header = header_at("/home/user/projects/rodeo");

        assert_eq!(
            line_text(&header.breadcrumb(&theme, 80)),
            "/home/user/projects/rodeo"
        );
    }

    #[test]
    fn breadcrumb_drops_leading_segments_when_narrow() {
        let theme = Theme::builtin().unwrap();
        let header = header_at("/a/very/deeply/nested/directory/tree/leaf");

        let text = line_text(&header.breadcrumb(&theme, 20));
        assert!(text.starts_with('…'), "{text}");
        // The directory you are in is what matters, so it always survives.
        assert!(text.ends_with("leaf"), "{text}");
        assert!(text.chars().count() <= 20, "{text}");
    }

    #[test]
    fn breadcrumb_keeps_the_leaf_even_when_it_cannot_fit() {
        let theme = Theme::builtin().unwrap();
        let header = header_at("/some/extremely-long-directory-name-that-cannot-fit");

        let text = line_text(&header.breadcrumb(&theme, 10));
        assert!(text.ends_with("extremely-long-directory-name-that-cannot-fit"));
    }

    #[test]
    fn breadcrumb_of_root_is_a_slash() {
        let theme = Theme::builtin().unwrap();
        assert_eq!(line_text(&header_at("/").breadcrumb(&theme, 40)), "/");
    }

    #[test]
    fn emphasises_the_current_directory() {
        let theme = Theme::builtin().unwrap();
        let line = header_at("/home/user/rodeo").breadcrumb(&theme, 80);

        let last = line.spans.last().unwrap();
        assert_eq!(last.content.as_ref(), "rodeo");
        assert_ne!(last.style, line.spans[0].style);
    }
}
