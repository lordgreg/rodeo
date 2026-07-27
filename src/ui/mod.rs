use std::{path::PathBuf, process::Command};

use crate::{
    Config,
    ui::{footer::Footer, theme::Theme},
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Block,
};

pub mod component;
pub mod dialog;
pub mod footer;
pub mod git;
pub mod header;
pub mod input;
pub mod panes;
pub mod popup_about;
pub mod popup_keybinds;
pub mod popup_preview;
pub mod search;
pub mod textinput;
pub mod theme;
pub mod uiconfig;

use component::Component;
use dialog::Dialog;
use header::Header;
use panes::Panes;
use popup_about::PopupAbout;
use popup_keybinds::PopupKeybinds;
use popup_preview::PopupPreview;
use search::{FilterSpec, Search, SearchKind};
use uiconfig::UiConfig;

#[derive(Debug)]
pub struct App {
    exit: bool,
    theme: Theme,
    ui_config: UiConfig,
    header: Header,
    footer: Footer,
    panes: Panes,
    config: Config,
    preview: Option<PopupPreview>,
    dialog: Option<Dialog>,
    search: Option<Search>,
    pending_editor_file: Option<PathBuf>,
}

impl App {
    pub fn new(theme: Theme, config: Config) -> Self {
        let panes = Panes::new(&config);

        let current_directory = config.get_initial_dir();

        let header = Header::new(current_directory);
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header,
            footer: Footer::default(),
            panes,
            config,
            preview: None,
            dialog: None,
            search: None,
            pending_editor_file: None,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_input()?;
            if let Some(path) = self.pending_editor_file.take() {
                terminal.clear()?;
                Command::new(&self.config.editor).arg(&path).status()?;
                terminal.clear()?;
                self.panes.reload(&self.config, true);
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        let background = Block::default().style(Style::new().bg(self.theme.colors.background()));

        frame.render_widget(background, frame.area());

        // The search bar is visible while editing a search or while a regex
        // filter is active on the active pane.
        let regex_filter_active = matches!(
            self.panes.get_active_pane().filter(),
            Some(FilterSpec::Regex(_))
        );
        let show_search_bar = self.search.is_some() || regex_filter_active;

        let outer_layout = if show_search_bar {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Max(1),
                    Constraint::Fill(1),
                    Constraint::Max(1),
                    Constraint::Max(1),
                ])
                .split(frame.area())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Max(1), Constraint::Fill(1), Constraint::Max(1)])
                .split(frame.area())
        };

        let stats = self.panes.get_active_pane().stats();
        self.header.set_stats(stats);
        self.footer.set_stats(stats);
        self.header
            .render(frame, &self.theme, &self.ui_config, outer_layout[0]);
        self.panes
            .render(frame, &self.theme, &self.ui_config, outer_layout[1]);
        let footer_idx = outer_layout.len() - 1;
        self.footer.render(
            frame,
            &self.theme,
            &self.ui_config,
            outer_layout[footer_idx],
        );

        if show_search_bar {
            self.render_search_bar(frame, outer_layout[2]);
        }

        if self.ui_config.active_keybind_popup {
            PopupKeybinds::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_about_popup {
            PopupAbout::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_preview_popup {
            let current = self.panes.get_active_pane().get_selected_entry();
            if self.preview.as_ref().and_then(|p| p.selected()) != current.as_ref() {
                self.preview = current.map(|e| PopupPreview::new(Some(e)));
            }
            if let Some(preview) = self.preview.as_mut() {
                preview.render(frame, &self.theme, &self.ui_config, frame.area());
            }
        }

        // Dialogs render on top of everything.
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.render(frame, &self.theme, &self.ui_config, frame.area());
        }
    }

    fn render_search_bar(&mut self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        use ratatui::widgets::Paragraph;

        let (text, style, cursor_offset): (String, Style, Option<u16>) = match &self.search {
            Some(s) => match s.kind {
                SearchKind::Fuzzy => (
                    format!("/{}", s.input.value),
                    Style::new().fg(self.theme.colors.foreground()),
                    Some(1 + s.input.cursor as u16),
                ),
                SearchKind::Regex => {
                    let style = if s.regex_invalid {
                        Style::new().fg(self.theme.colors.error())
                    } else {
                        Style::new().fg(self.theme.colors.foreground())
                    };
                    (
                        format!("regex: {}", s.input.value),
                        style,
                        Some(7 + s.input.cursor as u16),
                    )
                }
            },
            None => match self.panes.get_active_pane().filter() {
                Some(FilterSpec::Regex(pattern)) => (
                    format!("regex filter: {pattern}  (Ctrl+f to edit, Esc to clear)"),
                    Style::new().fg(self.theme.colors.muted()),
                    None,
                ),
                _ => (String::new(), Style::new(), None),
            },
        };

        frame.render_widget(
            Paragraph::new(text).style(style.bg(self.theme.colors.surface())),
            area,
        );

        if let Some(offset) = cursor_offset {
            frame.set_cursor_position((area.x + offset, area.y));
        }
    }
}
