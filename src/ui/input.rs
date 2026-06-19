use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    panes::{EntryKind, MoveDirection, OpenAction, SortOrder, SortType},
    popup_preview::PopupPreview,
    uiconfig::ActivePane,
    App,
};

impl App {
    pub(crate) fn handle_input(&mut self) -> std::io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if self.is_popup_active() && self.handle_popup_key(&key_event) {
                    return Ok(());
                }

                if self.handle_ctrl_key(&key_event) {
                    return Ok(());
                }
                if self.handle_shift_key(&key_event) {
                    return Ok(());
                }

                self.handle_main_key(&key_event);
            }
            _ => {}
        }
        Ok(())
    }

    fn is_popup_active(&self) -> bool {
        self.ui_config.active_keybind_popup
            || self.ui_config.active_about_popup
            || self.ui_config.active_preview_popup
    }

    fn handle_popup_key(&mut self, key: &KeyEvent) -> bool {
        if key.code == KeyCode::Esc {
            self.ui_config.active_keybind_popup = false;
            self.ui_config.active_about_popup = false;
            self.ui_config.active_preview_popup = false;
            return true;
        }

        if self.ui_config.active_preview_popup && key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.row_next();
                    }
                    return true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.row_prev();
                    }
                    return true;
                }
                _ => {}
            }
        }

        true
    }

    fn handle_ctrl_key(&mut self, key: &KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    self.config.show_hidden = !self.config.show_hidden;
                    self.panes.reload(&self.config, false);
                    return true;
                }
                _ => todo!("no action defined while pressing CTRL"),
            }
        }
        false
    }

    fn handle_shift_key(&mut self, key: &KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Right => {
                    self.config.sort_type = match self.config.sort_type {
                        SortType::Flagged => SortType::Name,
                        SortType::Name => SortType::Size,
                        SortType::Size => SortType::Time,
                        SortType::Time => SortType::Flagged,
                    };
                    self.panes.reload(&self.config, false);
                    return true;
                }
                KeyCode::Left => {
                    self.config.sort_type = match self.config.sort_type {
                        SortType::Flagged => SortType::Time,
                        SortType::Time => SortType::Size,
                        SortType::Size => SortType::Name,
                        SortType::Name => SortType::Flagged,
                    };
                    self.panes.reload(&self.config, false);
                    return true;
                }
                KeyCode::Char('O') => {
                    self.config.sort_order = match self.config.sort_order {
                        SortOrder::Ascending => SortOrder::Descending,
                        SortOrder::Descending => SortOrder::Ascending,
                    };
                    self.panes.reload(&self.config, false);
                    return true;
                }
                _ => unimplemented!("SHIFT key {} not yet implemented.", key.code),
            }
        }
        false
    }

    fn handle_main_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                match self.panes.get_active_pane_mut().open() {
                    OpenAction::DirectoryOpened | OpenAction::Reload => {
                        self.panes.reload(&self.config, true);
                        self.header.update(self.panes.get_active_pane().path.to_string());
                    }
                    OpenAction::FileOpened(_entry) => {}
                    OpenAction::Nothing => {}
                }
            }
            KeyCode::Backspace => {
                let path = self.panes.get_active_pane_mut().path.to_string();
                match self.panes.get_active_pane_mut().to_parent(path) {
                    OpenAction::DirectoryOpened => {
                        self.panes.reload(&self.config, true);
                        self.header.update(self.panes.get_active_pane().path.to_string());
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
                self.header.update(self.panes.get_active_pane().path.to_string());
            }
            KeyCode::Char('?') => {
                self.ui_config.active_keybind_popup = false;
                self.ui_config.active_about_popup = !self.ui_config.active_about_popup;
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
            KeyCode::Char(' ') => {
                if let Some(e) = self.panes.get_active_pane().get_selected_entry() {
                    match e.kind {
                        EntryKind::File | EntryKind::Directory | EntryKind::Symlink => {
                            self.ui_config.active_keybind_popup = false;
                            self.ui_config.active_about_popup = false;
                            self.ui_config.active_preview_popup = !self.ui_config.active_preview_popup;
                            self.preview = Some(PopupPreview::new(Some(e)));
                        }
                        _ => todo!("Preview of non-files not implemented yet"),
                    }
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.panes.goto_next(MoveDirection::Down);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.panes.goto_next(MoveDirection::Up);
            }
            _ => {}
        }
    }
}
