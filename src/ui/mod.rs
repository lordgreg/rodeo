use crossterm::event::{
    self,
    Event::{self},
    KeyCode, KeyEventKind,
};
use log::info;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::ui::{footer::Footer, theme::Theme};

pub mod component;
pub mod events;
pub mod footer;
pub mod header;
pub mod panes;
pub mod popup_about;
pub mod popup_keybinds;
pub mod theme;
pub mod uiconfig;

use component::Component;
use header::Header;
use panes::Panes;
use popup_about::PopupAbout;
use popup_keybinds::PopupKeybinds;
use uiconfig::{ActivePane, UiConfig};

#[derive(Debug)]
pub struct App {
    exit: bool,
    theme: Theme,
    ui_config: UiConfig,
    header: Header,
    footer: Footer,
    panes: Panes,
}

impl App {
    pub fn new(theme: Theme) -> Self {
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header: Header::new("~info one", "~/current/directory/foo", "git:master +2 ~1"),
            footer: Footer::default(),
            panes: Panes::new("Left Pane", "Right Pane"),
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_input()?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame<'_>) {
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
    }

    fn handle_input(&mut self) -> std::io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match key_event.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('h') => self.toggle_active_pane(ActivePane::Left),
                    KeyCode::Char('l') => self.toggle_active_pane(ActivePane::Right),
                    KeyCode::Char('k')
                        if key_event
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        self.ui_config.active_about_popup = false;
                        self.ui_config.active_keybind_popup = !self.ui_config.active_keybind_popup
                    }
                    KeyCode::Char('?') => {
                        self.ui_config.active_keybind_popup = false;
                        self.ui_config.active_about_popup = !self.ui_config.active_about_popup
                    }
                    KeyCode::Esc => {
                        if self.ui_config.active_keybind_popup {
                            self.ui_config.active_keybind_popup = false;
                        } else if self.ui_config.active_about_popup {
                            self.ui_config.active_about_popup = false;
                        } else {
                            self.exit = true;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        };
        Ok(())
    }

    fn toggle_active_pane(&mut self, pane: ActivePane) {
        info!("Switching active pane to {:?}", pane);
        self.ui_config.active_pane = pane;
    }

    fn _is_popup_active(&self) -> bool {
        self.ui_config.active_keybind_popup || self.ui_config.active_about_popup
    }
}
