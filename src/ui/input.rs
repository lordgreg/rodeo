use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    App,
    dialog::{Dialog, DialogAction, DialogResult},
    panes::{EntryKind, MoveDirection, OpenAction, SortOrder, SortType},
    popup_preview::PopupPreview,
    search::{FilterSpec, Search, SearchKind},
    uiconfig::ActivePane,
};

impl App {
    pub(crate) fn handle_input(&mut self) -> std::io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                // Dialogs take priority over all other key handling.
                if self.dialog.is_some() {
                    self.handle_dialog_key(&key_event);
                    return Ok(());
                }

                // The search bar consumes keys while it is being edited.
                if self.search.is_some() {
                    self.handle_search_key(&key_event);
                    return Ok(());
                }

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

    fn handle_dialog_key(&mut self, key: &KeyEvent) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };

        if let Some(result) = dialog.handle_key(key)
            && let Some(dialog) = self.dialog.take()
        {
            self.dispatch_dialog(dialog, result);
        }
    }

    fn handle_search_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => self.confirm_search(),
            KeyCode::Esc => self.cancel_search(),
            KeyCode::Backspace => {
                if let Some(s) = self.search.as_mut() {
                    s.input.backspace();
                }
                self.apply_search();
            }
            KeyCode::Down => self.panes.goto_next(MoveDirection::Down),
            KeyCode::Up => self.panes.goto_next(MoveDirection::Up),
            KeyCode::Left => {
                if let Some(s) = self.search.as_mut() {
                    s.input.left();
                }
            }
            KeyCode::Right => {
                if let Some(s) = self.search.as_mut() {
                    s.input.right();
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(s) = self.search.as_mut() {
                    s.input.insert(c);
                }
                self.apply_search();
            }
            _ => {}
        }
    }

    fn apply_search(&mut self) {
        let Some(s) = self.search.as_ref() else {
            return;
        };
        let (kind, input) = (s.kind, s.input.value.clone());
        let pane = self.panes.get_active_pane_mut();

        match kind {
            SearchKind::Fuzzy => {
                let _ = pane.set_filter(FilterSpec::Fuzzy(input));
            }
            SearchKind::Regex => {
                let result = if input.is_empty() {
                    pane.clear_filter();
                    Ok(())
                } else {
                    pane.set_filter(FilterSpec::Regex(input))
                };
                if let Some(s) = self.search.as_mut() {
                    s.regex_invalid = result.is_err();
                }
            }
        }
    }

    fn confirm_search(&mut self) {
        let Some(s) = self.search.take() else {
            return;
        };

        if s.kind == SearchKind::Fuzzy {
            // Jump to the top match: the cursor already sits on the best match
            // in the filtered list — keep it there after dropping the filter.
            let pane = self.panes.get_active_pane_mut();
            let selected = pane.get_selected_entry().map(|e| e.path);
            pane.clear_filter();
            if let Some(path) = selected {
                pane.select_by_path(&path);
            }
        }
        // Regex: the filter stays active after the bar closes (see Esc to clear).
    }

    fn cancel_search(&mut self) {
        self.search = None;
        self.panes.get_active_pane_mut().clear_filter();
    }

    fn dispatch_dialog(&mut self, dialog: Dialog, result: DialogResult) {
        match (dialog.action, result) {
            (DialogAction::Mkdir { parent }, DialogResult::Submitted(name)) => {
                self.mkdir(parent, name);
            }
            (DialogAction::Touch { parent }, DialogResult::Submitted(name)) => {
                self.touch(parent, name);
            }
            (DialogAction::TouchOverwrite { path }, DialogResult::Confirmed) => {
                self.create_file(&path);
            }
            (DialogAction::Rename { from }, DialogResult::Submitted(name)) => {
                self.rename(from, name);
            }
            (DialogAction::RenameOverwrite { from, to }, DialogResult::Confirmed) => {
                self.rename_path(&from, &to);
            }
            (DialogAction::Trash { paths }, DialogResult::Confirmed) => {
                self.trash_entries(paths);
            }
            (DialogAction::DeletePermanent { paths }, DialogResult::Confirmed) => {
                self.delete_permanent(paths);
            }
            (DialogAction::Copy { sources, dest_dir }, DialogResult::Confirmed) => {
                self.copy_entries(sources, dest_dir);
            }
            (DialogAction::Move { sources, dest_dir }, DialogResult::Confirmed) => {
                self.move_entries(sources, dest_dir);
            }
            _ => {}
        }
    }

    fn mkdir(&mut self, parent: PathBuf, name: String) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        match std::fs::create_dir(parent.join(name)) {
            Ok(()) => self.panes.reload(&self.config, false),
            Err(e) => {
                self.dialog = Some(Dialog::message(
                    "Error",
                    format!("Cannot create directory '{name}': {e}"),
                ));
            }
        }
    }

    fn touch(&mut self, parent: PathBuf, name: String) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        let path = parent.join(name);
        if path.exists() {
            self.dialog = Some(Dialog::confirm(
                "Overwrite?",
                format!("'{name}' already exists. Overwrite?"),
                DialogAction::TouchOverwrite { path },
            ));
        } else {
            self.create_file(&path);
        }
    }

    fn create_file(&mut self, path: &Path) {
        match std::fs::File::create(path) {
            Ok(_) => self.panes.reload(&self.config, false),
            Err(e) => {
                self.dialog = Some(Dialog::message(
                    "Error",
                    format!("Cannot create file '{}': {e}", path.display()),
                ));
            }
        }
    }

    fn start_rename(&mut self) {
        let Some(entry) = self.panes.get_active_pane().get_selected_entry() else {
            return;
        };
        if !matches!(
            entry.kind,
            EntryKind::File | EntryKind::Directory | EntryKind::Symlink
        ) {
            log::warn!("cannot rename entry of kind {:?}", entry.kind);
            return;
        }

        self.dialog = Some(Dialog::input(
            "Rename",
            "New name:",
            &entry.name,
            DialogAction::Rename { from: entry.path },
        ));
    }

    fn rename(&mut self, from: PathBuf, name: String) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        let Some(parent) = from.parent() else {
            return;
        };
        let to = parent.join(name);

        if to == from {
            return; // name unchanged
        }

        if to.exists() {
            self.dialog = Some(Dialog::confirm(
                "Overwrite?",
                format!("'{name}' already exists. Overwrite?"),
                DialogAction::RenameOverwrite { from, to },
            ));
        } else {
            self.rename_path(&from, &to);
        }
    }

    fn rename_path(&mut self, from: &Path, to: &Path) {
        match std::fs::rename(from, to) {
            Ok(()) => self.panes.reload(&self.config, false),
            Err(e) => {
                self.dialog = Some(Dialog::message(
                    "Error",
                    format!("Cannot rename '{}': {e}", from.display()),
                ));
            }
        }
    }

    /// Entries an operation should apply to: all selected entries, or the
    /// highlighted entry when nothing is selected.
    fn op_targets(&mut self) -> Vec<PathBuf> {
        let pane = self.panes.get_active_pane();

        let selected: Vec<PathBuf> = pane
            .selected_entries()
            .into_iter()
            .map(|e| e.path)
            .collect();
        if !selected.is_empty() {
            return selected;
        }

        if let Some(entry) = pane.get_selected_entry()
            && matches!(
                entry.kind,
                EntryKind::File | EntryKind::Directory | EntryKind::Symlink
            )
        {
            return vec![entry.path];
        }

        vec![]
    }

    fn start_delete(&mut self) {
        let targets = self.op_targets();
        if targets.is_empty() {
            return;
        }

        let message = if targets.len() == 1 {
            format!("Move '{}' to trash?", file_name_of(&targets[0]))
        } else {
            format!("Move {} items to trash?", targets.len())
        };

        self.dialog = Some(Dialog::confirm(
            "Delete",
            message,
            DialogAction::Trash { paths: targets },
        ));
    }

    fn trash_entries(&mut self, paths: Vec<PathBuf>) {
        for path in &paths {
            if let Err(e) = trash::delete(path) {
                self.dialog = Some(Dialog::confirm(
                    "Trash unavailable",
                    format!(
                        "Cannot trash '{path}' ({e}). Permanently delete?",
                        path = path.display()
                    ),
                    DialogAction::DeletePermanent {
                        paths: paths.clone(),
                    },
                ));
                return;
            }
        }
        self.panes.reload(&self.config, false);
    }

    fn delete_permanent(&mut self, paths: Vec<PathBuf>) {
        for path in &paths {
            let result = if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
            if let Err(e) = result {
                self.dialog = Some(Dialog::message(
                    "Error",
                    format!("Cannot delete '{}': {e}", path.display()),
                ));
                return;
            }
        }
        self.panes.reload(&self.config, false);
    }

    fn start_copy(&mut self) {
        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let dest_dir = PathBuf::from(&self.panes.get_inactive_pane().path);
        for src in &sources {
            if let Err(msg) = check_transfer_paths(src, &dest_dir) {
                self.dialog = Some(Dialog::message("Error", msg));
                return;
            }
        }

        let conflicts = sources
            .iter()
            .filter(|s| dest_dir.join(file_name_of(s)).exists())
            .count();

        if conflicts > 0 {
            self.dialog = Some(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist in the other pane. Overwrite?"),
                DialogAction::Copy { sources, dest_dir },
            ));
        } else {
            self.copy_entries(sources, dest_dir);
        }
    }

    fn copy_entries(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf) {
        for src in &sources {
            let dst = dest_dir.join(file_name_of(src));
            let result = if src.is_dir() {
                copy_dir_recursive(src, &dst)
            } else {
                std::fs::copy(src, &dst).map(|_| ())
            };
            if let Err(e) = result {
                self.dialog = Some(Dialog::message(
                    "Error",
                    format!("Cannot copy '{}': {e}", src.display()),
                ));
                return;
            }
        }
        self.panes.reload(&self.config, false);
    }

    fn start_move(&mut self) {
        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let dest_dir = PathBuf::from(&self.panes.get_inactive_pane().path);
        for src in &sources {
            if let Err(msg) = check_transfer_paths(src, &dest_dir) {
                self.dialog = Some(Dialog::message("Error", msg));
                return;
            }
        }

        let conflicts = sources
            .iter()
            .filter(|s| dest_dir.join(file_name_of(s)).exists())
            .count();

        if conflicts > 0 {
            self.dialog = Some(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist in the other pane. Overwrite?"),
                DialogAction::Move { sources, dest_dir },
            ));
        } else {
            self.move_entries(sources, dest_dir);
        }
    }

    fn move_entries(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf) {
        for src in &sources {
            let dst = dest_dir.join(file_name_of(src));

            if std::fs::rename(src, &dst).is_err() {
                // Cross-device move (EXDEV) or similar: fall back to copy + delete.
                let copied = if src.is_dir() {
                    copy_dir_recursive(src, &dst)
                } else {
                    std::fs::copy(src, &dst).map(|_| ())
                };
                if let Err(e) = copied {
                    self.dialog = Some(Dialog::message(
                        "Error",
                        format!("Cannot move '{}': {e}", src.display()),
                    ));
                    return;
                }

                let removed = if src.is_dir() {
                    std::fs::remove_dir_all(src)
                } else {
                    std::fs::remove_file(src)
                };
                if let Err(e) = removed {
                    self.dialog = Some(Dialog::message(
                        "Error",
                        format!("Copied but cannot remove source '{}': {e}", src.display()),
                    ));
                    return;
                }
            }
        }
        self.panes.reload(&self.config, false);
    }
    // Keys checked in order: popup-specific → Ctrl-modified → Shift-modified → unmodified.
    // Popup handler takes priority when any popup is active.
    fn handle_popup_key(&mut self, key: &KeyEvent) -> bool {
        if key.code == KeyCode::Esc
            || key.code == KeyCode::Char(' ')
            || key.code == KeyCode::Char('q')
        {
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
                _ => return false,
            }
        } else if self.ui_config.active_preview_popup && key.modifiers.is_empty() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.panes.goto_next(MoveDirection::Up);
                    return true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.panes.goto_next(MoveDirection::Down);
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    fn handle_ctrl_key(&mut self, key: &KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h') => {
                    self.config.show_hidden = !self.config.show_hidden;
                    self.panes.reload(&self.config, false);
                    return true;
                }
                KeyCode::Char('l') => {
                    self.panes.reload(&self.config, false);
                    return true;
                }
                KeyCode::Char('t') => {
                    let parent = PathBuf::from(&self.panes.get_active_pane().path);
                    self.dialog = Some(Dialog::input(
                        "touch",
                        "File name:",
                        "",
                        DialogAction::Touch { parent },
                    ));
                    return true;
                }
                KeyCode::Char('f') => {
                    let initial = match self.panes.get_active_pane().filter() {
                        Some(FilterSpec::Regex(pattern)) => pattern.clone(),
                        _ => String::new(),
                    };
                    self.search = Some(Search::regex(initial));
                    return true;
                }
                _ => {
                    log::debug!("unhandled Ctrl+{:?}", key.code);
                    return false;
                }
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
                KeyCode::Char('G') => self.panes.goto_last(),
                _ => {
                    log::debug!("unhandled Shift+{:?}", key.code);
                    return false;
                }
            }
        }
        false
    }

    fn handle_main_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::F(4) => match self.panes.get_active_pane_mut().open() {
                OpenAction::DirectoryOpened | OpenAction::Reload => {
                    self.panes.reload(&self.config, true);
                    self.header
                        .update(self.panes.get_active_pane().path.to_string());
                }
                OpenAction::FileOpened(_entry) => {
                    self.pending_editor_file = Some(_entry.path);
                }
                OpenAction::Nothing => {}
            },
            KeyCode::Backspace => {
                let path = self.panes.get_active_pane().path.clone();
                if let OpenAction::DirectoryOpened =
                    self.panes.get_active_pane_mut().go_to_parent(&path)
                {
                    self.panes.reload(&self.config, true);
                    self.header
                        .update(self.panes.get_active_pane().path.to_string());
                }
            }
            KeyCode::F(7) => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.dialog = Some(Dialog::input(
                    "mkdir",
                    "Directory name:",
                    "",
                    DialogAction::Mkdir { parent },
                ));
            }
            KeyCode::Char('g') => self.panes.goto_first(),
            KeyCode::Char('x') => self.panes.get_active_pane_mut().toggle_select(),
            KeyCode::Char('q') | KeyCode::F(10) => self.exit = true,
            KeyCode::Char('h') => self.panes.set_active_pane(ActivePane::Left),
            KeyCode::Char('l') => self.panes.set_active_pane(ActivePane::Right),
            KeyCode::Tab => {
                self.panes.toggle_active_pane();
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }
            KeyCode::Char('?') => {
                self.ui_config.active_keybind_popup = false;
                self.ui_config.active_about_popup = !self.ui_config.active_about_popup;
            }
            KeyCode::F(1) => {
                self.ui_config.active_about_popup = false;
                self.ui_config.active_keybind_popup = !self.ui_config.active_keybind_popup;
            }
            KeyCode::Esc => {
                if self.ui_config.active_keybind_popup {
                    self.ui_config.active_keybind_popup = false;
                } else if self.ui_config.active_about_popup {
                    self.ui_config.active_about_popup = false;
                } else if self.panes.get_active_pane().filter().is_some() {
                    self.panes.get_active_pane_mut().clear_filter();
                } else if self.panes.get_active_pane().has_selections() {
                    self.panes.get_active_pane_mut().clear_selections();
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
                            self.ui_config.active_preview_popup =
                                !self.ui_config.active_preview_popup;
                            self.preview = Some(PopupPreview::new(Some(e)));
                        }
                        EntryKind::Parent => log::warn!("Cannot preview parent directory."),
                        EntryKind::Unknown => log::warn!("Unknown file type - cannot preview"),
                    }
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.panes.goto_next(MoveDirection::Down);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.panes.goto_next(MoveDirection::Up);
            }
            KeyCode::Char('r') | KeyCode::F(2) => self.start_rename(),
            KeyCode::Char('/') | KeyCode::F(3) => {
                self.panes.get_active_pane_mut().clear_filter();
                self.search = Some(Search::fuzzy());
            }
            KeyCode::F(5) => self.start_copy(),
            KeyCode::F(6) => self.start_move(),
            KeyCode::Delete | KeyCode::F(8) => self.start_delete(),
            _ => {}
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Rejects transfer when the destination is the source itself (would truncate
/// the file on copy) or lies inside the source directory (infinite recursion).
fn check_transfer_paths(src: &Path, dest_dir: &Path) -> Result<(), String> {
    let (Ok(src_c), Ok(dest_c)) = (src.canonicalize(), dest_dir.canonicalize()) else {
        return Ok(()); // cannot verify — let the fs operation surface any error
    };

    if src_c.parent() == Some(dest_c.as_path()) {
        return Err("Source and destination are the same.".to_string());
    }

    if src.is_dir() && dest_c.starts_with(&src_c) {
        return Err("Cannot copy a directory into itself.".to_string());
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn copy_dir_recursive_copies_nested_tree() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();

        std::fs::create_dir(src_dir.path().join("sub")).unwrap();
        let mut f = std::fs::File::create(src_dir.path().join("top.txt")).unwrap();
        write!(f, "top").unwrap();
        let mut f = std::fs::File::create(src_dir.path().join("sub").join("nested.txt")).unwrap();
        write!(f, "nested").unwrap();

        let dst = dst_root.path().join("copy");
        copy_dir_recursive(src_dir.path(), &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub").join("nested.txt")).unwrap(),
            "nested"
        );
    }

    #[test]
    fn check_transfer_rejects_same_source_and_dest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::File::create(&file).unwrap();

        assert!(check_transfer_paths(&file, dir.path()).is_err());
    }

    #[test]
    fn check_transfer_rejects_dest_inside_source() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        assert!(check_transfer_paths(dir.path(), &sub).is_err());
    }

    #[test]
    fn check_transfer_allows_normal_transfer() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let file = src_root.path().join("a.txt");
        std::fs::File::create(&file).unwrap();

        assert!(check_transfer_paths(&file, dst_root.path()).is_ok());
    }
}
