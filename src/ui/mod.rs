use crate::{
    Config,
    ui::{
        footer::Footer,
        panes::{MoveDirection, OpenAction},
        theme::Theme,
    },
};
use crossterm::event::{
    self,
    Event::{self},
    KeyCode, KeyEventKind, KeyModifiers,
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
    pub fn new(theme: Theme, config: Config) -> Self {
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header: Header::new("~info one", "~/current/directory/foo", "git:master +2 ~1"),
            footer: Footer::default(),
            panes: Panes::new(config),
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_input()?;
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
    }

    fn handle_input(&mut self) -> std::io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                // if popup is active, only handle esc to close it
                if self.is_popup_active() {
                    if key_event.code == KeyCode::Esc {
                        self.ui_config.active_keybind_popup = false;
                        self.ui_config.active_about_popup = false;
                    }
                    return Ok(());
                }

                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    match key_event.code {
                        KeyCode::Char('h') => {
                            self.ui_config.show_hidden_entries =
                                !self.ui_config.show_hidden_entries;
                            self.panes.reload(self.ui_config.show_hidden_entries);
                        }
                        _ => todo!("no action defined!"),
                    }
                }

                match key_event.code {
                    KeyCode::Enter => {
                        match self.panes.get_active_pane_mut().open() {
                            OpenAction::Reload => {
                                self.panes.reload(self.ui_config.show_hidden_entries);
                            }
                            OpenAction::FileOpened(_entry) => {
                                // future: spawn editor/viewer
                            }
                            OpenAction::Nothing => {}
                        }
                    }
                    KeyCode::Char('x') => self.panes.get_active_pane_mut().toggle_select(),
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('h') => self.panes.set_active_pane(ActivePane::Left),
                    KeyCode::Char('l') => self.panes.set_active_pane(ActivePane::Right),
                    KeyCode::Tab => self.panes.toggle_active_pane(),
                    // KeyCode::Char('k')
                    //     if key_event
                    //         .modifiers
                    //         .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    // {
                    //     self.ui_config.active_about_popup = false;
                    //     self.ui_config.active_keybind_popup = !self.ui_config.active_keybind_popup
                    // }
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
                    // up/down j/k
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.panes.goto_next(MoveDirection::Down);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.panes.goto_next(MoveDirection::Up);
                    }
                    _ => {}
                }
            }
            _ => {}
        };
        Ok(())
    }

    fn is_popup_active(&self) -> bool {
        self.ui_config.active_keybind_popup || self.ui_config.active_about_popup
    }
}
