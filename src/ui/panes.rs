use std::{
    fs::{self},
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Row, Table, TableState},
};

use crate::{
    config::Config,
    ui::{
        component::Component,
        theme::Theme,
        uiconfig::{ActivePane, UiConfig},
    },
};

fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

fn format_date(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

#[derive(PartialEq, Debug, Clone)]
pub enum EntryKind {
    Parent,
    Directory,
    Symlink,
    File,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: EntryKind,
    pub path: PathBuf,
    pub name: String,
    pub size: String,
    pub modified: String,
    pub selected: bool,
}

impl Entry {
    pub fn new(path: PathBuf) -> Self {
        let kind = if path.is_file() {
            EntryKind::File
        } else if path.file_name().is_some_and(|name| name == "..") {
            EntryKind::Parent
        } else if path.is_dir() {
            EntryKind::Directory
        } else if path.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Unknown
        };

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "-".to_string());

        let (size, modified) = path
            .metadata()
            .ok()
            .map(|meta| {
                (
                    format_size(meta.len()),
                    meta.modified()
                        .ok()
                        .map(format_date)
                        .unwrap_or_else(|| "-".to_string()),
                )
            })
            .unwrap_or_else(|| ("-".to_string(), "-".to_string()));

        Self {
            kind,
            path,
            name,
            size,
            modified,
            selected: false,
        }
    }

    pub fn toggle_selected(&mut self) {
        self.selected = !self.selected;
    }
}

#[derive(Debug)]
pub struct Pane {
    pub state: TableState,
    pub path: String,
    paths: Vec<Entry>,
    constraints: [Constraint; 4],
}

impl Pane {
    pub fn new(path: String, show_hidden: bool) -> Self {
        let paths = read_entries(&path, show_hidden);
        Self {
            state: TableState::default(),
            path,
            paths,
            constraints: [
                Constraint::Max(1),
                Constraint::Fill(1),
                Constraint::Percentage(20),
                Constraint::Percentage(30),
            ],
        }
    }

    pub fn clear_selections(&mut self) {
        for path in self.paths.iter_mut() {
            path.selected = false;
        }
    }
    // .iter_mut(|entry| entry.selected = false);

    pub fn entries_to_rows(&self) -> Vec<[String; 4]> {
        self.paths
            .iter()
            .map(|p| {
                let marker = if p.selected { "x" } else { "" };

                [
                    marker.to_string(),
                    p.name.clone(),
                    p.size.clone(),
                    p.modified.clone(),
                ]
            })
            .collect()
    }

    pub fn toggle_select(&mut self) {
        let Some(i) = self.state.selected() else {
            return;
        };
        let Some(path) = self.paths.get_mut(i) else {
            return;
        };

        if path.kind != EntryKind::File {
            return;
        }

        path.toggle_selected();
    }

    pub fn reload(&mut self, show_hidden: bool) {
        self.clear_selections();
        self.paths = read_entries(&self.path, show_hidden);
        self.state = TableState::default();
        self.state.select(Some(0));
    }

    pub fn open(&mut self) -> OpenAction {
        let Some(i) = self.state.selected() else {
            return OpenAction::Nothing;
        };

        let Some(entry) = self.paths.get(i) else {
            return OpenAction::Nothing;
        };

        match entry.kind {
            EntryKind::File => OpenAction::FileOpened(entry.clone()),
            EntryKind::Parent => {
                let previous = self.path.clone();

                if let Some(parent) = Path::new(&previous).parent() {
                    self.path = parent.to_string_lossy().to_string();

                    return OpenAction::Reload;
                } else {
                    return OpenAction::Nothing;
                }
            }
            EntryKind::Directory => {
                self.path = entry.path.to_str().unwrap().to_string();
                return OpenAction::Reload;
            }
            _ => OpenAction::Nothing,
        };
        OpenAction::Nothing
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        active: bool,
        theme: &Theme,
        _ui: &UiConfig,
    ) {
        let entries = self.entries_to_rows();

        let color_border = if active {
            theme.colors.secondary()
        } else {
            theme.colors.border()
        };

        let header = Row::new(["", "Name", "Size", "Modify Time"])
            .style(Style::new().fg(theme.colors.highlight()).bold());

        let rows: Vec<Row> = entries
            .iter()
            .map(|e| Row::new([e[0].as_str(), e[1].as_str(), e[2].as_str(), e[3].as_str()]))
            .collect();

        let table = Table::new(rows, self.constraints)
            .header(header)
            .column_spacing(1)
            .style(Style::new().fg(theme.colors.primary()))
            .row_highlight_style(
                Style::new()
                    .fg(theme.colors.highlight())
                    .bg(theme.colors.surface())
                    .bold(),
            )
            .cell_highlight_style(Style::new().reversed().yellow())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("~")
                    .border_style(color_border)
                    .title_style(Style::new().fg(theme.colors.primary())),
            );

        frame.render_stateful_widget(table, area, &mut self.state);
    }
}

fn read_entries(dir: &str, show_hidden: bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            show_hidden
                || !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .map(|p| Entry::new(p))
        .collect();
    entries.insert(0, Entry::new(Path::new(dir).join("..")));
    entries
}

#[derive(PartialEq)]
pub enum MoveDirection {
    Up,
    Down,
}

pub enum OpenAction {
    Reload,
    FileOpened(Entry),
    Nothing,
}

#[derive(Debug)]
pub struct Panes {
    pub pane_left: Pane,
    pub pane_right: Pane,
    active_pane: ActivePane,
}

impl Panes {
    pub fn new(config: Config) -> Self {
        Self {
            pane_left: Pane::new(config.initial_dir().to_string(), false),
            pane_right: Pane::new(config.initial_dir().to_string(), false),
            active_pane: ActivePane::Left,
        }
    }

    pub fn set_active_pane(&mut self, pane: ActivePane) {
        self.active_pane = pane;
    }

    pub fn get_active_pane(&self) -> &Pane {
        if self.active_pane == ActivePane::Left {
            &self.pane_left
        } else {
            &self.pane_right
        }
    }

    pub fn get_active_pane_mut(&mut self) -> &mut Pane {
        if self.active_pane == ActivePane::Left {
            &mut self.pane_left
        } else {
            &mut self.pane_right
        }
    }

    pub fn toggle_active_pane(&mut self) {
        self.active_pane = if self.active_pane == ActivePane::Left {
            ActivePane::Right
        } else {
            ActivePane::Left
        }
    }

    pub fn reload(&mut self, show_hidden: bool) {
        self.pane_left.reload(show_hidden);
        self.pane_right.reload(show_hidden);
    }

    pub fn next_index(
        row_count: &usize,
        current: Option<usize>,
        direction: MoveDirection,
    ) -> usize {
        let max = row_count.saturating_sub(1);
        match direction {
            MoveDirection::Down => match current {
                Some(i) if i >= max => 0,
                Some(i) => i + 1,
                None => 0,
            },
            MoveDirection::Up => match current {
                Some(0) => max,
                Some(i) => i - 1,
                None => 0,
            },
        }
    }

    pub fn goto_next(&mut self, direction: MoveDirection) {
        if self.active_pane == ActivePane::Left {
            let next = Self::next_index(
                &self.pane_left.paths.len(),
                self.pane_left.state.selected(),
                direction,
            );
            self.pane_left.state.select(Some(next));
        } else {
            let next = Self::next_index(
                &self.pane_right.paths.len(),
                self.pane_right.state.selected(),
                direction,
            );
            self.pane_right.state.select(Some(next));
        }
    }
}

impl Component for Panes {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, ui: &UiConfig, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        self.pane_left.render(
            frame,
            layout[0],
            self.active_pane == ActivePane::Left,
            theme,
            ui,
        );
        self.pane_right.render(
            frame,
            layout[1],
            self.active_pane == ActivePane::Right,
            theme,
            ui,
        );
    }
}
