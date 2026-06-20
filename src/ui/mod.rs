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
pub mod footer;
pub mod header;
pub mod input;
pub mod panes;
pub mod popup_about;
pub mod popup_keybinds;
pub mod popup_preview;
pub mod theme;
pub mod uiconfig;

use component::Component;
use header::Header;
use panes::Panes;
use popup_about::PopupAbout;
use popup_keybinds::PopupKeybinds;
use popup_preview::PopupPreview;
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
    pending_editor_file: Option<PathBuf>,
}

impl App {
    pub fn new(theme: Theme, config: Config) -> Self {
        let panes = Panes::new(&config);

        let current_directory = config.get_initial_dir();

        let header = Header::new("~info one", current_directory);
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header,
            footer: Footer::default(),
            panes,
            config,
            preview: None,
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

        let outer_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Max(1), Constraint::Fill(1), Constraint::Max(1)])
            .split(frame.area());

        self.header
            .render(frame, &self.theme, &self.ui_config, outer_layout[0]);
        self.panes
            .render(frame, &self.theme, &self.ui_config, outer_layout[1]);
        self.footer
            .render(frame, &self.theme, &self.ui_config, outer_layout[2]);

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
    }
}
