use crate::{
    Config,
    ui::{
        footer::Footer,
        panes::{MoveDirection, OpenAction, SortOrder, SortType},
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
    config: Config,
}

impl App {
    pub fn new(theme: Theme, config: Config) -> Self {
        let panes = Panes::new(&config);
        let mut header = Header::new("~info one", "~/current/directory/foo", "git:master +2 ~1");
        header.update(config.initial_dir().to_string());
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header,
            footer: Footer::default(),
            panes,
            config,
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

                // CTRL
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    match key_event.code {
                        KeyCode::Char('h') => {
                            self.config.show_hidden = !self.config.show_hidden;
                            self.panes.reload(&self.config);
                        }
                        _ => todo!("no action defined while pressing CTRL"),
                    }
                }

                // SHIFT
                if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                    match key_event.code {
                        KeyCode::Right => {
                            self.config.sort_type = match self.config.sort_type {
                                SortType::Flagged => SortType::Name,
                                SortType::Name => SortType::Size,
                                SortType::Size => SortType::Time,
                                SortType::Time => SortType::Flagged,
                            };
                            self.panes.reload(&self.config);
                        }
                        KeyCode::Left => {
                            self.config.sort_type = match self.config.sort_type {
                                SortType::Flagged => SortType::Time,
                                SortType::Time => SortType::Size,
                                SortType::Size => SortType::Name,
                                SortType::Name => SortType::Flagged,
                            };
                            self.panes.reload(&self.config);
                        }
                        KeyCode::Char('O') => {
                            self.config.sort_order = match self.config.sort_order {
                                SortOrder::Ascending => SortOrder::Descending,
                                SortOrder::Descending => SortOrder::Ascending,
                            };
                            self.panes.reload(&self.config)
                        }
                        _ => todo!("SHIFT key {} not yet implemented.", key_event.code),
                    }
                }

                match key_event.code {
                    KeyCode::Enter => {
                        match self.panes.get_active_pane_mut().open() {
                            OpenAction::Reload => {
                                self.panes.reload(&self.config);
                                self.header
                                    .update(self.panes.get_active_pane().path.to_string());
                            }
                            OpenAction::FileOpened(_entry) => {
                                // future: spawn editor/viewer
                            }
                            OpenAction::Nothing => {}
                        }
                    }
                    KeyCode::Backspace => {
                        let path = self.panes.get_active_pane_mut().path.to_string();

                        match self.panes.get_active_pane_mut().to_parent(path) {
                            OpenAction::Reload => {
                                self.panes.reload(&self.config);
                                self.header
                                    .update(self.panes.get_active_pane().path.to_string());
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Char('x') => self.panes.get_active_pane_mut().toggle_select(),
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('h') => self.panes.set_active_pane(ActivePane::Left),
                    KeyCode::Char('l') => self.panes.set_active_pane(ActivePane::Right),
                    KeyCode::Tab => {
                        self.panes.toggle_active_pane();
                        self.header
                            .update(self.panes.get_active_pane().path.to_string());
                    }
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
