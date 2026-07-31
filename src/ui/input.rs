//! Key handling.
//!
//! Every key press runs through one chain, most specific handler first:
//! dialogs, the command bar, search, the popups, then Ctrl-, Shift- and
//! finally unmodified keys resolved through the configurable keymap.

use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    App,
    completion::Completion,
    dialog::{Dialog, DialogAction, DialogResult},
    keymap::Action,
    panes::{EntryKind, MoveDirection, OpenAction, SortOrder, SortType},
    popup_bulkrename::BulkRename,
    popup_findinfiles::FindInFiles,
    popup_preview::PopupPreview,
    popup_trash::TrashView,
    search::{FilterSpec, Search, SearchKind},
    textinput::TextInput,
    uiconfig::ActivePane,
};
use crate::config::Config;
use crate::fs::ops;
use crate::ui::theme::Theme;

impl App {
    pub(crate) fn handle_input(&mut self) -> std::io::Result<()> {
        if let Event::Key(key_event) = event::read()?
            && key_event.kind == KeyEventKind::Press
        {
            self.dispatch_key(&key_event);
        }
        Ok(())
    }

    /// Routes one key press through the handler chain. Split out of
    /// [`Self::handle_input`] so tests can drive the app without a terminal.
    pub fn dispatch_key(&mut self, key_event: &KeyEvent) {
        // A pending `d` (awaiting the second key of `dd`) is cancelled
        // by any other key.
        if self.pending_d && !matches!(key_event.code, KeyCode::Char('d')) {
            self.pending_d = false;
        }

        // Dialogs take priority over all other key handling.
        if self.dialog.is_some() {
            self.handle_dialog_key(key_event);
            return;
        }

        // The command bar consumes keys while it is open.
        if self.command.is_some() {
            self.handle_command_key(key_event);
            return;
        }

        // The search bar consumes keys while it is being edited.
        if self.search.is_some() {
            self.handle_search_key(key_event);
            return;
        }

        // The find-in-files popup consumes keys while it is open.
        if self.find_in_files.is_some() {
            self.handle_find_in_files_key(key_event);
            return;
        }

        // Bulk rename popup consumes keys while it is open.
        if self.bulk_rename.is_some() {
            self.handle_bulk_rename_key(key_event);
            return;
        }

        // Trash view consumes keys while it is open.
        if self.trash_view.is_some() {
            self.handle_trash_key(key_event);
            return;
        }

        if self.is_popup_active() && self.handle_popup_key(key_event) {
            return;
        }

        if self.handle_ctrl_key(key_event) {
            return;
        }
        if self.handle_shift_key(key_event) {
            return;
        }

        self.handle_main_key(key_event);
    }

    /// `true` for keys that may trigger a keymap action: plain presses and
    /// Shift'ed characters (`G`, `*`, `?`, `:` arrive with SHIFT on most
    /// layouts). Ctrl/Alt/Super combos are never single-key bindings and must
    /// not fall through to the keymap.
    fn is_plain_key(key: &KeyEvent) -> bool {
        (key.modifiers - KeyModifiers::SHIFT).is_empty()
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

    fn handle_find_in_files_key(&mut self, key: &KeyEvent) {
        let Some(find) = self.find_in_files.as_mut() else {
            return;
        };

        // If currently searching, only allow Esc to cancel
        if find.searching {
            if matches!(key.code, KeyCode::Esc) {
                self.find_in_files = None;
            }
            return;
        }

        match key.code {
            KeyCode::Enter => {
                if find.results.is_empty() {
                    // Start a new search
                    let pattern = find.input.value.clone();
                    if !pattern.is_empty() {
                        self.start_find_in_files(pattern);
                    }
                } else {
                    // Open the selected file
                    if let Some(m) = find.selected_match() {
                        self.pending_editor_file = Some(m.path.clone());
                        self.find_in_files = None;
                    }
                }
            }
            KeyCode::Esc => {
                self.find_in_files = None;
            }
            KeyCode::Backspace => {
                find.input.backspace();
            }
            KeyCode::Down => {
                find.move_down();
            }
            KeyCode::Up => {
                find.move_up();
            }
            KeyCode::Left => {
                find.input.left();
            }
            KeyCode::Right => {
                find.input.right();
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                find.input.insert(c);
            }
            _ => {}
        }
    }

    fn handle_trash_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.trash_view = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(tv) = self.trash_view.as_mut() {
                    tv.move_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(tv) = self.trash_view.as_mut() {
                    tv.move_down();
                }
            }
            KeyCode::Char('x') => {
                if let Some(tv) = self.trash_view.as_mut() {
                    tv.toggle_select();
                    tv.move_down();
                }
            }
            // Restore selected/highlighted item(s).
            KeyCode::Char('r') => {
                let Some(tv) = self.trash_view.as_ref() else {
                    return;
                };
                match tv.restore_targets() {
                    Ok(n) => {
                        self.ok_status(format!("{n} item(s) restored"));
                        self.trash_view = Some(TrashView::load()); // refresh
                        self.panes.reload(&self.config, true);
                    }
                    Err(e) => self.err_status(e),
                }
            }
            // Permanently delete selected/highlighted item(s).
            KeyCode::Char('D') => {
                let Some(tv) = self.trash_view.as_ref() else {
                    return;
                };
                match tv.purge_targets() {
                    Ok(n) => {
                        self.ok_status(format!("{n} item(s) permanently deleted"));
                        self.trash_view = Some(TrashView::load()); // refresh
                    }
                    Err(e) => self.err_status(e),
                }
            }
            _ => {}
        }
    }

    fn handle_bulk_rename_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.bulk_rename = None;
            }
            KeyCode::Enter => {
                let Some(br) = self.bulk_rename.as_ref() else {
                    return;
                };
                if !br.is_valid() {
                    return;
                }
                let pairs = br.rename_pairs();
                self.bulk_rename = None;
                let mut errors = 0usize;
                for (old, new) in &pairs {
                    if let Err(e) = std::fs::rename(old, new) {
                        log::error!("bulk rename {old:?} → {new:?}: {e}");
                        errors += 1;
                    }
                }
                self.panes.reload(&self.config, true);
                if errors == 0 {
                    self.ok_status(format!("{} file(s) renamed", pairs.len()));
                } else {
                    self.err_status(format!("{errors} rename(s) failed"));
                }
            }
            KeyCode::Backspace => {
                if let Some(br) = self.bulk_rename.as_mut() {
                    br.pattern.backspace();
                    br.update_preview();
                }
            }
            KeyCode::Left => {
                if let Some(br) = self.bulk_rename.as_mut() {
                    br.pattern.left();
                }
            }
            KeyCode::Right => {
                if let Some(br) = self.bulk_rename.as_mut() {
                    br.pattern.right();
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(br) = self.bulk_rename.as_mut() {
                    br.pattern.insert(c);
                    br.update_preview();
                }
            }
            _ => {}
        }
    }

    fn start_find_in_files(&mut self, pattern: String) {
        let Some(find) = self.find_in_files.as_mut() else {
            return;
        };

        find.start_search();

        // Get the current directory
        let search_dir = PathBuf::from(&self.panes.get_active_pane().path);

        // Compile the regex pattern
        let re = match regex::Regex::new(&pattern) {
            Ok(re) => re,
            Err(_) => {
                find.finish_search();
                self.err_status("Invalid regex pattern".to_string());
                return;
            }
        };

        // Walk the directory tree and search file contents
        let walker = ignore::WalkBuilder::new(&search_dir)
            .hidden(false)
            .git_ignore(true)
            .build();

        let mut match_count = 0;
        let mut matches = Vec::new();
        for entry in walker {
            let Ok(entry) = entry else { continue };
            let path = entry.path();

            // Only search files, not directories
            if !path.is_file() {
                continue;
            }

            // Read file contents
            let Ok(contents) = std::fs::read_to_string(path) else {
                continue;
            };

            // Search each line
            for (line_num, line) in contents.lines().enumerate() {
                if re.is_match(line) {
                    matches.push(super::popup_findinfiles::FindMatch {
                        path: path.to_path_buf(),
                        line_num: line_num + 1,
                        line_content: line.to_string(),
                    });
                    match_count += 1;

                    // Limit results to avoid excessive memory use
                    if match_count >= 1000 {
                        break;
                    }
                }
            }

            if match_count >= 1000 {
                break;
            }
        }

        // Results are gathered locally and handed over in one go: borrowing
        // self inside the walk would need an unwrap per match.
        let Some(find) = self.find_in_files.as_mut() else {
            return;
        };
        for m in matches {
            find.add_result(m);
        }
        find.finish_search();

        if match_count == 0 {
            self.ok_status("No matches found".to_string());
        } else {
            self.ok_status(format!("Found {} matches", match_count));
        }
    }

    /// Swaps the active theme and rebuilds everything derived from it.
    pub(crate) fn set_theme(&mut self, theme: Theme) {
        self.syn_theme = std::sync::Arc::new(theme.to_syntect_theme());
        self.theme = theme;
        // Drop the cached preview so it is rebuilt with the new colours.
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.selected().is_some())
        {
            self.preview = None;
        }
    }

    /// Transient footer notice for successful operations.
    pub(crate) fn ok_status(&mut self, msg: String) {
        self.footer.set_status(msg, false);
    }

    /// Transient footer notice for non-fatal errors.
    pub(crate) fn err_status(&mut self, msg: String) {
        self.footer.set_status(msg, true);
    }

    fn dispatch_dialog(&mut self, dialog: Dialog, result: DialogResult) {
        match (dialog.action, result) {
            (DialogAction::Mkdir { parent }, DialogResult::Submitted(name)) => {
                self.mkdir(parent, name);
            }
            (DialogAction::Touch { parent }, DialogResult::Submitted(name)) => {
                self.touch(parent, name);
            }
            (DialogAction::Create { parent }, DialogResult::Submitted(name)) => {
                let name = name.trim();
                if let Some(dir) = name.strip_suffix('/') {
                    self.mkdir(parent, dir.to_string());
                } else {
                    self.touch(parent, name.to_string());
                }
            }
            (DialogAction::SelectGlob, DialogResult::Submitted(pattern)) => {
                let count = self.panes.get_active_pane_mut().select_matching(&pattern);
                self.ok_status(format!("{count} selected"));
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
            (DialogAction::PasteMove { sources, dest_dir }, DialogResult::Confirmed) => {
                self.move_entries(sources, dest_dir);
                self.clipboard.clear();
                self.clipboard_cut = false;
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
            Ok(()) => {
                self.panes.reload(&self.config, false);
                self.ok_status(format!("Created directory '{name}'"));
            }
            Err(e) => {
                self.err_status(format!("Cannot create directory '{name}': {e}"));
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
            Ok(_) => {
                self.panes.reload(&self.config, false);
                self.ok_status(format!("Created file '{}'", path.display()));
            }
            Err(e) => {
                self.err_status(format!("Cannot create file '{}': {e}", path.display()));
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
            Ok(()) => {
                self.panes.reload(&self.config, false);
                self.ok_status(format!("Renamed to '{}'", to.display()));
            }
            Err(e) => {
                self.err_status(format!("Cannot rename '{}': {e}", from.display()));
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
            format!("Move '{}' to trash?", ops::file_name_of(&targets[0]))
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
        self.ok_status("Moved to trash".to_string());
    }

    fn delete_permanent(&mut self, paths: Vec<PathBuf>) {
        for path in &paths {
            if let Err(e) = ops::delete_entry(path) {
                self.err_status(format!("Cannot delete '{}': {e}", path.display()));
                return;
            }
        }
        self.panes.reload(&self.config, false);
        self.ok_status("Deleted permanently".to_string());
    }

    fn start_copy(&mut self) {
        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let dest_dir = PathBuf::from(&self.panes.get_inactive_pane().path);
        for src in &sources {
            if let Err(msg) = ops::check_transfer_paths(src, &dest_dir) {
                self.err_status(msg);
                return;
            }
        }

        let conflicts = sources
            .iter()
            .filter(|s| dest_dir.join(ops::file_name_of(s)).exists())
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

    /// Transfers larger than this run in the background with a progress gauge.
    const ASYNC_THRESHOLD_BYTES: u64 = 10 * 1024 * 1024;

    fn copy_entries(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf) {
        if ops::total_size(&sources) > Self::ASYNC_THRESHOLD_BYTES {
            self.start_transfer(sources, dest_dir, false);
            return;
        }
        for src in &sources {
            if let Err(e) = ops::copy_entry(src, &dest_dir) {
                self.err_status(format!("Cannot copy '{}': {e}", src.display()));
                return;
            }
        }
        self.panes.reload(&self.config, false);
        self.ok_status("Copied".to_string());
    }

    fn start_move(&mut self) {
        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let dest_dir = PathBuf::from(&self.panes.get_inactive_pane().path);
        for src in &sources {
            if let Err(msg) = ops::check_transfer_paths(src, &dest_dir) {
                self.err_status(msg);
                return;
            }
        }

        let conflicts = sources
            .iter()
            .filter(|s| dest_dir.join(ops::file_name_of(s)).exists())
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
        if ops::total_size(&sources) > Self::ASYNC_THRESHOLD_BYTES {
            self.start_transfer(sources, dest_dir, true);
            return;
        }
        for src in &sources {
            if let Err(e) = ops::move_entry(src, &dest_dir) {
                self.err_status(format!("Cannot move '{}': {e}", src.display()));
                return;
            }
        }
        self.panes.reload(&self.config, false);
        self.ok_status("Moved".to_string());
    }

    /// Starts a background copy (cut=false) or move (cut=true) with a progress
    /// gauge. The transfer is cancellable with Esc.
    fn start_transfer(&mut self, sources: Vec<PathBuf>, dest_dir: PathBuf, is_cut: bool) {
        let total = ops::total_size(&sources);
        let (rx, cancel) = ops::spawn_transfer(sources, dest_dir, is_cut);
        self.progress = Some(super::Progress {
            title: if is_cut {
                "Moving…".to_string()
            } else {
                "Copying…".to_string()
            },
            total_bytes: total,
            done_bytes: 0,
            rx,
            cancel,
            is_cut,
        });
    }

    /// Yanks the operation targets (selection or highlighted entry) into the
    /// internal clipboard as a copy.
    fn yank(&mut self) {
        let targets = self.op_targets();
        if targets.is_empty() {
            return;
        }
        self.clipboard = targets;
        self.clipboard_cut = false;
        self.ok_status(format!("{} yanked", self.clipboard.len()));
    }

    /// Pastes the clipboard into the active pane's directory. `cut` moves
    /// instead of copying and clears the clipboard afterwards.
    fn paste(&mut self, cut: bool) {
        if self.clipboard.is_empty() {
            return;
        }

        let sources = self.clipboard.clone();
        let dest_dir = PathBuf::from(&self.panes.get_active_pane().path);

        for src in &sources {
            if let Err(msg) = ops::check_transfer_paths(src, &dest_dir) {
                self.err_status(msg);
                return;
            }
        }

        let conflicts = sources
            .iter()
            .filter(|s| dest_dir.join(ops::file_name_of(s)).exists())
            .count();

        if conflicts > 0 {
            let action = if cut {
                DialogAction::PasteMove { sources, dest_dir }
            } else {
                DialogAction::Copy { sources, dest_dir }
            };
            self.dialog = Some(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist here. Overwrite?"),
                action,
            ));
            return;
        }

        if cut {
            self.move_entries(sources, dest_dir);
            self.clipboard.clear();
            self.clipboard_cut = false;
        } else {
            self.copy_entries(sources, dest_dir);
        }
    }

    fn handle_command_key(&mut self, key: &KeyEvent) {
        let Some(input) = self.command.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Enter => {
                if let Some(input) = self.command.take() {
                    self.completion = Completion::default();
                    self.run_command(&input.value.clone());
                }
                return;
            }
            KeyCode::Esc => {
                self.command = None;
                self.completion = Completion::default();
                return;
            }
            // Tab walks the menu; the line follows the highlighted candidate.
            KeyCode::Tab | KeyCode::BackTab => {
                let forward = key.code == KeyCode::Tab;
                let line = input.value.clone();
                if let Some(completed) = self.completion.cycle(&line, forward)
                    && let Some(input) = self.command.as_mut()
                {
                    *input = TextInput::new(completed);
                }
                return;
            }
            KeyCode::Backspace => input.backspace(),
            KeyCode::Left => input.left(),
            KeyCode::Right => input.right(),
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                input.insert(c);
            }
            _ => return,
        }

        // Editing the line invalidates the menu: rebuild it from scratch.
        self.refresh_completion();
    }

    /// Recomputes the completion menu for the current command line.
    pub(crate) fn refresh_completion(&mut self) {
        let Some(input) = self.command.as_ref() else {
            self.completion = Completion::default();
            return;
        };
        let line = input.value.clone();
        let pane_dir = PathBuf::from(&self.panes.get_active_pane().path);
        self.completion = Completion::compute(&line, &pane_dir);
    }

    fn run_command(&mut self, cmdline: &str) {
        let trimmed = cmdline.trim();
        if trimmed.is_empty() {
            return;
        }

        if let Some(shell_cmd) = trimmed.strip_prefix('!') {
            self.run_shell_capture(shell_cmd.trim());
            return;
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or_default();
        let arg = parts.next().unwrap_or("").trim();

        match cmd {
            "q" | "quit" => self.exit = true,
            "w" | "write" => match Config::save_config(&self.config, None) {
                Ok(()) => self.ok_status("Configuration saved".to_string()),
                Err(e) => self.err_status(format!("Cannot save config: {e}")),
            },
            "so" | "source" => self.reload_config(),
            "e" | "cd" => self.navigate_to(arg),
            "mkdir" => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.mkdir(parent, arg.to_string());
            }
            "touch" => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.touch(parent, arg.to_string());
            }
            "delete" => self.start_delete(),
            "rename" => {
                if let Some(entry) = self.panes.get_active_pane().get_selected_entry()
                    && matches!(
                        entry.kind,
                        EntryKind::File | EntryKind::Directory | EntryKind::Symlink
                    )
                {
                    self.rename(entry.path, arg.to_string());
                }
            }
            "theme" => self.switch_theme(arg),
            "help" => self.ui_config.active_keybind_popup = true,
            "shell" => self.pending_shell = true,
            "trash" => {
                self.trash_view = Some(TrashView::load());
            }
            _ => {
                self.err_status(format!("Unknown command: {cmd}  (try :help)"));
            }
        }
    }

    /// `:source` — reload the config file at runtime and apply it (theme
    /// included when it changed).
    fn reload_config(&mut self) {
        match Config::load_config(None) {
            Ok(config) => {
                let theme_name = config.theme.clone();
                let theme_changed = theme_name != self.config.theme;
                self.config = config;
                if theme_changed {
                    match Theme::load_theme(Some(&theme_name)) {
                        Ok(theme) => self.set_theme(theme),
                        Err(e) => {
                            self.err_status(format!("Cannot load theme '{theme_name}': {e}"));
                        }
                    }
                }
                self.panes.reload(&self.config, false);
                self.ok_status("Config reloaded".to_string());
            }
            Err(e) => self.err_status(format!("Cannot reload config: {e}")),
        }
    }

    fn navigate_to(&mut self, arg: &str) {
        if arg.is_empty() {
            return;
        }

        match PathBuf::from(arg).canonicalize() {
            Ok(p) if p.is_dir() => {
                self.panes.get_active_pane_mut().path = p.to_string_lossy().to_string();
                self.panes.reload(&self.config, true);
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }
            _ => {
                self.err_status(format!("Not a directory: {arg}"));
            }
        }
    }

    fn switch_theme(&mut self, name: &str) {
        let known = Theme::get_theme_list();

        if name.is_empty() {
            self.dialog = Some(Dialog::message(
                "Themes",
                format!("Available themes:\n{}", known.join("\n")),
            ));
            return;
        }

        // Unknown names would silently fall back to the default theme, so
        // reject them here and tell the user what is available instead.
        let is_path = name.ends_with(".toml");
        if !is_path && !known.iter().any(|t| t == name) {
            self.err_status(format!(
                "Unknown theme '{name}'. Available: {}",
                known.join(", ")
            ));
            return;
        }
        if is_path && !Path::new(name).exists() {
            self.err_status(format!("Theme file not found: {name}"));
            return;
        }

        match Theme::load_theme(Some(name)) {
            Ok(theme) => {
                self.set_theme(theme);
                self.config.theme = name.to_string();
                self.ok_status(format!("Theme: {name}"));
            }
            Err(e) => {
                self.err_status(format!("Cannot load theme '{name}': {e}"));
            }
        }
    }

    fn run_shell_capture(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }

        // Expand %f → space-separated shell-quoted paths of selected (or
        // highlighted) entries so `:!wc -l %f` works naturally.
        let cmd = if cmd.contains("%f") {
            let targets = self.op_targets();
            let quoted = targets
                .iter()
                .map(|p| {
                    let s = p.to_string_lossy();
                    format!("'{}'", s.replace('\'', "'\\''"))
                })
                .collect::<Vec<_>>()
                .join(" ");
            cmd.replace("%f", &quoted)
        } else {
            cmd.to_string()
        };
        let cmd = cmd.as_str();

        match std::process::Command::new("sh").args(["-c", cmd]).output() {
            Ok(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                if !out.stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!(
                        "[stderr]\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                if text.trim().is_empty() {
                    text = "(no output)".to_string();
                }

                let lines: Vec<ratatui::text::Line> = text
                    .lines()
                    .map(|l| ratatui::text::Line::from(l.to_string()))
                    .collect();

                // Output opens as a scrollable, selection-independent preview.
                self.preview = Some(PopupPreview::from_text(
                    format!(":!{cmd}"),
                    ratatui::text::Text::from(lines),
                ));
                self.ui_config.active_preview_popup = true;
            }
            Err(e) => {
                self.err_status(format!("Cannot run shell: {e}"));
            }
        }
    }
    // Keys checked in order: popup-specific → Ctrl-modified → Shift-modified → unmodified.
    // Popup handler takes priority when any popup is active.
    fn handle_popup_key(&mut self, key: &KeyEvent) -> bool {
        // Dismiss keys are plain presses only — Ctrl+Space/Ctrl+q must not
        // close a popup.
        if Self::is_plain_key(key)
            && matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q')
            )
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
                KeyCode::Char('f') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.page_down();
                    }
                    return true;
                }
                KeyCode::Char('b') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.page_up();
                    }
                    return true;
                }
                KeyCode::Char('d') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.half_page_down();
                    }
                    return true;
                }
                KeyCode::Char('u') => {
                    if let Some(ref mut preview) = self.preview {
                        preview.half_page_up();
                    }
                    return true;
                }
                _ => return false,
            }
        } else if self.ui_config.active_preview_popup && Self::is_plain_key(key) {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.panes.goto_next(MoveDirection::Up);
                    return true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.panes.goto_next(MoveDirection::Down);
                    return true;
                }
                KeyCode::Char('w') => {
                    if let Some(preview) = self.preview.as_mut() {
                        preview.toggle_wrap();
                    }
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
                KeyCode::Char('a') => {
                    let count = self.panes.get_active_pane_mut().select_all();
                    self.ok_status(format!("{count} selected"));
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
                KeyCode::Char('g') => {
                    self.find_in_files = Some(FindInFiles::new());
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
                KeyCode::Char('G') => {
                    self.panes.goto_last();
                    return true;
                }
                _ => {
                    log::debug!("unhandled Shift+{:?}", key.code);
                    return false;
                }
            }
        }
        false
    }

    fn handle_main_key(&mut self, key: &KeyEvent) {
        // Esc is hardcoded: it drives the universal dismiss chain.
        if key.code == KeyCode::Esc {
            self.handle_esc();
            return;
        }

        // Keymap entries are single-key bindings: a modified key that no
        // Ctrl/Shift handler claimed is dropped rather than firing the
        // unmodified action (Ctrl+d must not start the `dd` chord).
        if !Self::is_plain_key(key) {
            log::debug!("unhandled {:?}+{:?}", key.modifiers, key.code);
            return;
        }

        let Some(action) = self
            .keymap
            .iter()
            .find(|(code, _)| *code == key.code)
            .map(|(_, action)| *action)
        else {
            return;
        };

        match action {
            Action::OpenEntry => match self.panes.get_active_pane_mut().open() {
                OpenAction::DirectoryOpened | OpenAction::Reload => {
                    self.panes.reload(&self.config, true);
                    self.header
                        .update(self.panes.get_active_pane().path.to_string());
                }
                OpenAction::FileOpened(path) => {
                    self.pending_editor_file = Some(path);
                }
                OpenAction::Nothing => {}
            },
            Action::ParentDir => {
                let path = self.panes.get_active_pane().path.clone();
                if let OpenAction::DirectoryOpened =
                    self.panes.get_active_pane_mut().go_to_parent(&path)
                {
                    self.panes.reload(&self.config, true);
                    self.header
                        .update(self.panes.get_active_pane().path.to_string());
                }
            }
            Action::Mkdir => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.dialog = Some(Dialog::input(
                    "mkdir",
                    "Directory name:",
                    "",
                    DialogAction::Mkdir { parent },
                ));
            }
            Action::GotoFirst => self.panes.goto_first(),
            Action::GotoLast => self.panes.goto_last(),
            Action::ToggleSelect => self.panes.get_active_pane_mut().toggle_select(),
            Action::SelectGlob => {
                self.dialog = Some(Dialog::input(
                    "Select",
                    "Wildcard pattern (* ?):",
                    "",
                    DialogAction::SelectGlob,
                ));
            }
            Action::DirSizes => {
                let count = self.panes.get_active_pane_mut().compute_dir_sizes();
                self.ok_status(format!("Sizes computed for {count} directories"));
            }
            Action::BulkRename => {
                let pane = self.panes.get_active_pane();
                let targets = if pane.has_selections() {
                    pane.selected_entries()
                        .into_iter()
                        .map(|e| e.path)
                        .collect()
                } else {
                    // Fall back to the highlighted entry.
                    pane.get_selected_entry()
                        .filter(|e| !matches!(e.kind, EntryKind::Parent))
                        .map(|e| vec![e.path])
                        .unwrap_or_default()
                };
                if targets.len() < 2 {
                    self.err_status("Select 2+ files with x before bulk rename".to_string());
                } else {
                    self.bulk_rename = Some(BulkRename::new(targets));
                }
            }
            Action::Quit => self.exit = true,
            Action::PaneLeft => self.panes.set_active_pane(ActivePane::Left),
            Action::PaneRight => self.panes.set_active_pane(ActivePane::Right),
            Action::PaneToggle => {
                self.panes.toggle_active_pane();
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }
            Action::About => {
                self.ui_config.active_keybind_popup = false;
                self.ui_config.active_about_popup = !self.ui_config.active_about_popup;
            }
            Action::Help => {
                self.ui_config.active_about_popup = false;
                self.ui_config.active_keybind_popup = !self.ui_config.active_keybind_popup;
            }
            Action::Preview => {
                if let Some(e) = self.panes.get_active_pane().get_selected_entry() {
                    match e.kind {
                        EntryKind::File | EntryKind::Directory | EntryKind::Symlink => {
                            self.ui_config.active_keybind_popup = false;
                            self.ui_config.active_about_popup = false;
                            self.ui_config.active_preview_popup =
                                !self.ui_config.active_preview_popup;
                            self.preview = Some(PopupPreview::new(Some(e), self.syn_theme.clone()));
                        }
                        EntryKind::Parent => log::warn!("Cannot preview parent directory."),
                        EntryKind::Unknown => log::warn!("Unknown file type - cannot preview"),
                    }
                }
            }
            Action::MoveDown => {
                self.panes.goto_next(MoveDirection::Down);
            }
            Action::MoveUp => {
                self.panes.goto_next(MoveDirection::Up);
            }
            Action::Rename => self.start_rename(),
            Action::Search => {
                self.panes.get_active_pane_mut().clear_filter();
                self.search = Some(Search::fuzzy());
            }
            Action::CommandPalette => {
                self.command = Some(TextInput::default());
                self.refresh_completion();
            }
            Action::Create => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.dialog = Some(Dialog::input(
                    "Create",
                    "File name  (end with / for a directory):",
                    "",
                    DialogAction::Create { parent },
                ));
            }
            Action::Yank => {
                self.yank();
            }
            Action::Paste => self.paste(false),
            Action::PasteMove => self.paste(true),
            Action::DeleteChord => {
                if self.pending_d {
                    self.pending_d = false;
                    self.start_delete();
                } else {
                    self.pending_d = true;
                }
            }
            Action::Copy => self.start_copy(),
            Action::Move => self.start_move(),
            Action::Delete => self.start_delete(),
        }
    }

    fn handle_esc(&mut self) {
        if let Some(p) = &self.progress {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        } else if self.ui_config.active_keybind_popup {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway app rooted in a temporary directory.
    fn test_app(dir: &Path) -> App {
        let config = Config {
            initial_directory_left: dir.to_string_lossy().to_string(),
            initial_directory_right: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let theme = Theme::load_theme(None).expect("default theme in themes/");
        App::new(theme, config)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn ctrl_modified_key_does_not_trigger_plain_action() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // Ctrl+d is not a binding: it must not start the `dd` delete chord.
        app.dispatch_key(&key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(!app.pending_d);

        // Plain d still does.
        app.dispatch_key(&key(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.pending_d);
    }

    #[test]
    fn ctrl_q_does_not_quit() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(!app.exit);

        app.dispatch_key(&key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.exit);
    }

    #[test]
    fn ctrl_space_does_not_dismiss_popup() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.ui_config.active_keybind_popup = true;

        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::CONTROL));
        assert!(app.ui_config.active_keybind_popup);

        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.ui_config.active_keybind_popup);
    }

    #[test]
    fn command_palette_offers_completions_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::NONE));

        assert!(app.command.is_some());
        assert!(
            app.completion.is_active(),
            "the menu should be offered without pressing Tab first"
        );
    }

    #[test]
    fn tab_cycles_the_completion_menu() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Char('q'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.command.as_ref().unwrap().value, "q");

        app.dispatch_key(&key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.command.as_ref().unwrap().value, "quit");

        // Shift+Tab walks back.
        app.dispatch_key(&key(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.command.as_ref().unwrap().value, "q");
    }

    #[test]
    fn typing_refilters_the_menu() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::NONE));
        let all = app.completion.candidates().len();

        app.dispatch_key(&key(KeyCode::Char('t'), KeyModifiers::NONE));
        let filtered = app.completion.candidates().len();

        assert!(filtered < all && filtered > 0, "{filtered} of {all}");
    }

    #[test]
    fn leaving_the_palette_clears_the_menu() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.command.is_none());
        assert!(!app.completion.is_active());
    }

    #[test]
    fn shifted_characters_still_reach_the_keymap() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // '?' arrives with SHIFT on most layouts — it must still toggle About.
        app.dispatch_key(&key(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert!(app.ui_config.active_about_popup);
    }
}
