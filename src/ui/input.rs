use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    App,
    dialog::{Dialog, DialogAction, DialogResult},
    panes::{EntryKind, MoveDirection, OpenAction, SortOrder, SortType},
    popup_preview::PopupPreview,
    search::{FilterSpec, Search, SearchKind},
    textinput::TextInput,
    uiconfig::ActivePane,
};
use crate::config::Config;
use crate::fs::ops;
use crate::ui::theme::Theme;

impl App {
    pub(crate) fn handle_input(&mut self) -> std::io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                // A pending `d` (awaiting the second key of `dd`) is cancelled
                // by any other key.
                if self.pending_d && !matches!(key_event.code, KeyCode::Char('d')) {
                    self.pending_d = false;
                }

                // Dialogs take priority over all other key handling.
                if self.dialog.is_some() {
                    self.handle_dialog_key(&key_event);
                    return Ok(());
                }

                // The command bar consumes keys while it is open.
                if self.command.is_some() {
                    self.handle_command_key(&key_event);
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
                    self.run_command(&input.value.clone());
                }
            }
            KeyCode::Esc => {
                self.command = None;
            }
            KeyCode::Tab => self.complete_command(),
            KeyCode::Backspace => input.backspace(),
            KeyCode::Left => input.left(),
            KeyCode::Right => input.right(),
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                input.insert(c);
            }
            _ => {}
        }
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
            _ => {
                self.err_status(format!("Unknown command: {cmd}  (try :help)"));
            }
        }
    }

    fn complete_command(&mut self) {
        const COMMANDS: &[&str] = &[
            "q", "quit", "w", "write", "e", "cd", "mkdir", "touch", "delete", "rename", "theme",
            "help", "shell",
        ];

        let Some(input) = self.command.as_mut() else {
            return;
        };
        let text = input.value.clone();

        // Argument completion: only theme names for now.
        if let Some((first, rest)) = text.split_once(char::is_whitespace) {
            if first == "theme" {
                let rest = rest.trim_start();
                let matches: Vec<String> = Theme::get_theme_list()
                    .into_iter()
                    .filter(|t| t.starts_with(rest))
                    .collect();
                if let Some(completion) = common_prefix(&matches)
                    && completion.len() > rest.len()
                {
                    input.value = format!("{first} {completion}");
                    input.cursor = input.value.chars().count();
                }
            }
            return;
        }

        let matches: Vec<String> = COMMANDS
            .iter()
            .filter(|c| c.starts_with(text.as_str()))
            .map(|s| s.to_string())
            .collect();
        if let Some(completion) = common_prefix(&matches)
            && completion.len() > text.len()
        {
            input.value = completion;
        }
        if matches.len() == 1 {
            input.value = format!("{} ", matches[0]);
        }
        input.cursor = input.value.chars().count();
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

        // Guard: load_from_file exits the process on unreadable files.
        let is_path = name.ends_with(".yaml");
        if !is_path && !known.iter().any(|t| t == name) {
            self.err_status(format!("Unknown theme '{name}'. Available: {}", known.join(", ")));
            return;
        }
        if is_path && !Path::new(name).exists() {
            self.err_status(format!("Theme file not found: {name}"));
            return;
        }

        match Theme::load_theme(Some(name)) {
            Ok(theme) => {
                self.theme = theme;
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

                let lines: Vec<&str> = text.lines().collect();
                let text = if lines.len() > 30 {
                    format!(
                        "{}\n… ({} more lines)",
                        lines[..30].join("\n"),
                        lines.len() - 30
                    )
                } else {
                    text
                };

                self.dialog = Some(Dialog::message(format!(":!{cmd}"), text));
            }
            Err(e) => {
                self.err_status(format!("Cannot run shell: {e}"));
            }
        }
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
            KeyCode::Char(':') => {
                self.command = Some(TextInput::default());
            }
            KeyCode::Char('a') => {
                let parent = PathBuf::from(&self.panes.get_active_pane().path);
                self.dialog = Some(Dialog::input(
                    "Create",
                    "File name  (end with / for a directory):",
                    "",
                    DialogAction::Create { parent },
                ));
            }
            KeyCode::Char('y') => {
                self.yank();
            }
            KeyCode::Char('p') => self.paste(false),
            KeyCode::Char('P') => self.paste(true),
            KeyCode::Char('d') => {
                if self.pending_d {
                    self.pending_d = false;
                    self.start_delete();
                } else {
                    self.pending_d = true;
                }
            }
            KeyCode::F(5) => self.start_copy(),
            KeyCode::F(6) => self.start_move(),
            KeyCode::Delete | KeyCode::F(8) => self.start_delete(),
            _ => {}
        }
    }
}

/// Longest common prefix of the given strings, or `None` when empty.
fn common_prefix(strings: &[String]) -> Option<String> {
    let first = strings.first()?;
    let mut prefix = first.clone();
    for s in &strings[1..] {
        while !s.starts_with(&prefix) {
            prefix.pop();
        }
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_prefix_of_similar_strings() {
        let strings = vec!["quit".to_string(), "quark".to_string()];
        assert_eq!(common_prefix(&strings), Some("qu".to_string()));
    }

    #[test]
    fn common_prefix_single_string_is_itself() {
        let strings = vec!["theme".to_string()];
        assert_eq!(common_prefix(&strings), Some("theme".to_string()));
    }

    #[test]
    fn common_prefix_none_for_empty() {
        assert_eq!(common_prefix(&[]), None);
    }

    #[test]
    fn common_prefix_empty_when_no_overlap() {
        let strings = vec!["abc".to_string(), "xyz".to_string()];
        assert_eq!(common_prefix(&strings), Some(String::new()));
    }
}
