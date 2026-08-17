//! Key handling.
//!
//! Every key press runs through one chain, most specific handler first:
//! dialogs, the input bar, the remaining overlays, then Ctrl-, Shift- and
//! finally unmodified keys resolved through the configurable keymap.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    App, EditorTarget, InputMode, Overlay, OverlayKind,
    completion::Completion,
    dialog::{Dialog, DialogAction, DialogResult},
    keymap::{Action, Binding},
    panes::{EntryKind, MoveDirection, OpenAction},
    popup_bookmarks::BookmarksView,
    popup_bulkrename::BulkRename,
    popup_findfiles::FileFinder,
    popup_findinfiles::FindInFiles,
    popup_permissions::{Field, PermissionsEditor},
    popup_preview::PopupPreview,
    popup_trash::TrashView,
    search::{FilterSpec, Search},
    textinput::{TextEdit, TextInput},
};
use crate::config::Config;
use crate::fs::{archive, filter::SearchFilter, ops};
use crate::types::{ActivePane, SortOrder, SortType};
use crate::ui::theme::Theme;

/// A copy or a move.
///
/// The two differ only in which `ops` call runs, what the footer says and
/// which dialog action carries the confirmation — they used to be four
/// functions, and `start_copy`/`start_move` differed by three lines out of
/// twenty-nine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

impl Transfer {
    /// Whether the source is removed afterwards.
    fn is_cut(self) -> bool {
        matches!(self, Self::Move)
    }

    /// Lower-case, for "Cannot copy '...'".
    fn verb(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
        }
    }

    /// What the footer says once it is done.
    fn done_label(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
        }
    }

    fn apply(self, src: &Path, dest_dir: &Path) -> std::io::Result<()> {
        match self {
            Self::Copy => ops::copy_entry(src, dest_dir),
            Self::Move => ops::move_entry(src, dest_dir),
        }
    }

    fn dialog_action(
        self,
        sources: Vec<PathBuf>,
        base: PathBuf,
        dest_dir: PathBuf,
    ) -> DialogAction {
        match self {
            Self::Copy => DialogAction::Copy {
                sources,
                base,
                dest_dir,
            },
            Self::Move => DialogAction::Move {
                sources,
                base,
                dest_dir,
            },
        }
    }
}

/// Validates every source against the destination it will really land in, and
/// counts the names already taken there.
///
/// Both checks have to use the per-source destination rather than the pane's
/// own directory: with the layout preserved, `X/a/b.txt` copied into a pane
/// rooted at `X` lands back on itself, which the pane-level check would miss.
fn check_and_count_conflicts(
    sources: &[PathBuf],
    base: &Path,
    dest_dir: &Path,
) -> Result<usize, String> {
    let mut conflicts = 0;

    for src in sources {
        let target = ops::dest_dir_for(src, base, dest_dir);
        ops::check_transfer_paths(src, &target)?;
        if target.join(ops::file_name_of(src)).exists() {
            conflicts += 1;
        }
    }

    Ok(conflicts)
}

/// Longest command echoed back in a footer message, so a long one-liner does
/// not push the key hints off the bar.
const SHELL_LABEL_WIDTH: usize = 24;

/// Lines the find-in-files preview moves per scroll key.
const PREVIEW_SCROLL_LINES: i32 = 10;

/// Shortens `text` to `width` characters, marking the cut with an ellipsis.
fn elide(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

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

        // Dialogs take priority over everything, including the input bar.
        if self.overlay_kind() == Some(OverlayKind::Dialog) {
            self.handle_dialog_key(key_event);
            return;
        }

        // The input bar consumes keys while it is being edited. It is checked
        // before the remaining overlays because it can sit underneath one:
        // `:` opens the command line with the preview popup up.
        if self.input_mode.is_some() {
            self.handle_input_mode_key(key_event);
            return;
        }

        match self.overlay_kind() {
            Some(OverlayKind::FindInFiles) => self.handle_find_in_files_key(key_event),
            Some(OverlayKind::FindFiles) => self.handle_find_files_key(key_event),
            Some(OverlayKind::BulkRename) => self.handle_bulk_rename_key(key_event),
            Some(OverlayKind::Trash) => self.handle_trash_key(key_event),
            Some(OverlayKind::Bookmarks) => self.handle_bookmarks_key(key_event),
            Some(OverlayKind::Permissions) => self.handle_permissions_key(key_event),
            // These two leave the panes usable underneath, so a key they do
            // not claim falls through to the normal bindings.
            Some(OverlayKind::Preview | OverlayKind::Keybinds) => {
                if !self.handle_popup_key(key_event) {
                    self.handle_main_key(key_event);
                }
            }
            Some(OverlayKind::Dialog) | None => self.handle_main_key(key_event),
        }
    }

    /// Routes a key to whichever half of the input bar is open.
    fn handle_input_mode_key(&mut self, key: &KeyEvent) {
        match self.input_mode {
            Some(InputMode::Filter(_)) => self.handle_search_key(key),
            Some(InputMode::Command(_)) => self.handle_command_key(key),
            None => {}
        }
    }

    /// `true` for keys that may trigger a keymap action: plain presses and
    /// Shift'ed characters (`G`, `*`, `?`, `:` arrive with SHIFT on most
    /// layouts). Ctrl/Alt/Super combos are never single-key bindings and must
    /// not fall through to the keymap.
    fn is_plain_key(key: &KeyEvent) -> bool {
        (key.modifiers - KeyModifiers::SHIFT).is_empty()
    }

    fn handle_dialog_key(&mut self, key: &KeyEvent) {
        let Some(Overlay::Dialog(dialog)) = &mut self.overlay else {
            return;
        };

        if let Some(result) = dialog.handle_key(key)
            && let Some(Overlay::Dialog(dialog)) = self.overlay.take()
        {
            self.dispatch_dialog(dialog, result);
        }
    }

    fn handle_search_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => self.confirm_search(),
            KeyCode::Esc => self.cancel_search(),
            KeyCode::Down => self.panes.goto_next(MoveDirection::Down),
            KeyCode::Up => self.panes.goto_next(MoveDirection::Up),
            _ => {
                let Some(s) = self.search_mut() else {
                    return;
                };
                if s.input.handle_key(key) == TextEdit::Changed {
                    self.apply_search();
                }
            }
        }
    }

    /// Applies what is in the filter bar to the active pane.
    ///
    /// There is only one kind of query: [`FilterSpec::detect`] decides whether
    /// it reads as a regular expression or as a fuzzy pattern, so the user
    /// never has to pick a mode before typing.
    fn apply_search(&mut self) {
        let Some(s) = self.search() else {
            return;
        };
        let input = s.input.value.clone();
        let pane = self.panes.get_active_pane_mut();

        if input.is_empty() {
            pane.clear_filter();
        } else {
            // detect() only hands back a regex it has already compiled, so the
            // only errors left are impossible — but a failure must still not
            // leave the pane showing a filter it is not applying.
            let _ = pane.set_filter(FilterSpec::detect(&input));
        }

        // An unfinished regex is matched fuzzily; colouring the bar is how the
        // user learns the pattern is not doing what it looks like yet.
        if let Some(s) = self.search_mut() {
            s.regex_invalid = FilterSpec::is_broken_regex(&input);
        }
    }

    /// Closes the filter bar, leaving the filter itself in place (Esc clears
    /// it, and the footer bar says so).
    fn confirm_search(&mut self) {
        self.input_mode = None;
    }

    fn cancel_search(&mut self) {
        self.input_mode = None;
        self.panes.get_active_pane_mut().clear_filter();
    }

    fn handle_find_in_files_key(&mut self, key: &KeyEvent) {
        let Some(find) = self.find_in_files_mut() else {
            return;
        };

        // If currently searching, only allow Esc to cancel
        if find.searching {
            if matches!(key.code, KeyCode::Esc) {
                self.overlay = None;
            }
            return;
        }

        match key.code {
            KeyCode::Enter => {
                // Enter searches whatever the box holds; it only opens a result
                // once the list on screen belongs to that exact pattern.
                // Otherwise editing a query and pressing Enter would launch the
                // editor on the previous search's selection.
                if find.results_are_current() {
                    if let Some(m) = find.selected_match() {
                        self.pending_editor_file =
                            Some(EditorTarget::at_line(m.path.clone(), m.line_num));
                        self.overlay = None;
                    }
                } else {
                    let pattern = find.input.value.clone();
                    if !pattern.is_empty() {
                        self.start_find_in_files(pattern);
                    }
                }
            }
            KeyCode::Esc => {
                self.overlay = None;
            }
            KeyCode::Down => {
                find.move_down();
            }
            KeyCode::Up => {
                find.move_up();
            }
            // Telescope-style aliases: the hands never leave the query box.
            KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
                find.move_down();
            }
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                find.move_up();
            }
            // Scroll the preview without moving the selection.
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                find.scroll_preview(PREVIEW_SCROLL_LINES);
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                find.scroll_preview(-PREVIEW_SCROLL_LINES);
            }
            KeyCode::PageDown => {
                find.scroll_preview(PREVIEW_SCROLL_LINES);
            }
            KeyCode::PageUp => {
                find.scroll_preview(-PREVIEW_SCROLL_LINES);
            }
            _ => {
                find.input.handle_key(key);
            }
        }
    }

    fn handle_find_files_key(&mut self, key: &KeyEvent) {
        let Some(finder) = self.find_files_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
            }
            KeyCode::Enter => {
                let Some(entry) = finder.selected().cloned() else {
                    return;
                };
                self.overlay = None;
                self.reveal(&entry.path, entry.is_dir);
            }
            // Straight into the editor, for when the file was the destination
            // rather than the pane it lives in.
            KeyCode::Char('e') if key.modifiers == KeyModifiers::CONTROL => {
                let Some(entry) = finder.selected().cloned() else {
                    return;
                };
                if entry.is_dir {
                    return;
                }
                self.overlay = None;
                self.pending_editor_file = Some(EditorTarget::new(entry.path));
            }
            KeyCode::Down => finder.move_down(),
            KeyCode::Up => finder.move_up(),
            KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => finder.move_down(),
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => finder.move_up(),
            KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                finder.scroll_preview(PREVIEW_SCROLL_LINES);
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                finder.scroll_preview(-PREVIEW_SCROLL_LINES);
            }
            KeyCode::PageDown => finder.scroll_preview(PREVIEW_SCROLL_LINES),
            KeyCode::PageUp => finder.scroll_preview(-PREVIEW_SCROLL_LINES),
            _ => {
                if finder.input.handle_key(key) == TextEdit::Changed {
                    finder.refilter();
                }
            }
        }
    }

    /// Points the active pane at `path`: into it when it is a directory,
    /// otherwise at its parent with the file under the cursor.
    pub(crate) fn reveal(&mut self, path: &Path, is_dir: bool) {
        let (dir, select) = if is_dir {
            (path.to_path_buf(), None)
        } else {
            match path.parent() {
                Some(parent) => (parent.to_path_buf(), Some(path.to_path_buf())),
                None => return,
            }
        };

        self.panes.get_active_pane_mut().path = dir.to_string_lossy().to_string();
        self.panes.reload(&self.config, false);
        if let Some(path) = select {
            self.panes.get_active_pane_mut().select_by_path(&path);
        }
        self.sync_header();
    }

    /// Paths the bookmark key applies to: the marked entries, or the one under
    /// the cursor when nothing is marked — the same rule as copy, move and
    /// delete ([`App::op_targets`]).
    ///
    /// A cursor sitting on `..` means the pane's own directory, so bookmarking
    /// the folder you are looking at needs no second key.
    fn bookmark_targets(&self) -> Vec<PathBuf> {
        let pane = self.panes.get_active_pane();

        let marked: Vec<PathBuf> = pane
            .selected_entries()
            .into_iter()
            .map(|e| e.path)
            .collect();
        let targets = if !marked.is_empty() {
            marked
        } else {
            match pane.get_selected_entry() {
                Some(entry) if entry.kind == EntryKind::Parent => vec![PathBuf::from(&pane.path)],
                Some(entry) => vec![entry.path],
                None => vec![PathBuf::from(&pane.path)],
            }
        };

        targets.iter().map(|p| Self::normalized(p)).collect()
    }

    /// An absolute, link-free form of `path`, falling back to `path` itself.
    ///
    /// A bookmark outlives the session that made it, so a relative path is no
    /// use: `rodeo --left .` would otherwise write `"."` into the file and
    /// resolve it against whatever directory the next run started in. It also
    /// makes the same directory reached two ways compare equal, so bookmarking
    /// `sub` and then bookmarking `..` from inside it cannot produce two
    /// entries for one folder. `Pane::open` canonicalizes for the same reason.
    fn normalized(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    /// Bookmarks the targets, or removes them when every one is already
    /// bookmarked.
    ///
    /// Toggling each target independently would make a part-bookmarked
    /// selection add some and drop others, which looks arbitrary. Adding wins
    /// unless there is nothing left to add.
    fn toggle_bookmark(&mut self) {
        let targets = self.bookmark_targets();
        if targets.is_empty() {
            return;
        }

        let all_known = targets.iter().all(|p| self.bookmarks.contains(p));

        // Paths actually changed, so the message can name the right one: with
        // three marked entries of which two were already bookmarked, `count`
        // is 1 and naming `targets[0]` would point at the wrong entry.
        let changed: Vec<PathBuf> = if all_known {
            targets
                .iter()
                .filter(|p| self.bookmarks.remove(p))
                .cloned()
                .collect()
        } else {
            // TOML cannot carry a path that is not valid UTF-8. Skipping it
            // here, with the path named, beats accepting the bookmark and then
            // failing to write the file every time any other one changes. Only
            // adding is affected — removing such a path must stay possible.
            let (ok, bad): (Vec<_>, Vec<_>) = targets.iter().partition(|p| p.to_str().is_some());
            for path in bad {
                self.err_status(format!(
                    "Cannot bookmark '{}': the path is not valid UTF-8",
                    path.display()
                ));
            }
            ok.into_iter()
                .filter(|p| self.bookmarks.add((*p).clone()))
                .cloned()
                .collect()
        };

        if changed.is_empty() {
            return;
        }
        if !self.save_bookmarks() {
            return;
        }

        let what = if changed.len() == 1 {
            ops::file_name_of(&changed[0])
        } else {
            format!("{} entries", changed.len())
        };
        if all_known {
            self.ok_status(format!("Bookmark removed: {what}"));
        } else {
            self.ok_status(format!("Bookmarked: {what}"));
        }
    }

    fn open_bookmarks(&mut self) {
        self.overlay = Some(Overlay::Bookmarks(BookmarksView::new(&self.bookmarks)));
    }

    fn handle_bookmarks_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(view) = self.bookmarks_view_mut() {
                    view.move_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(view) = self.bookmarks_view_mut() {
                    view.move_down();
                }
            }
            KeyCode::Enter => {
                let Some(index) = self.bookmarks_view().and_then(|v| v.selected_idx()) else {
                    return;
                };
                self.jump_to_bookmark(index);
            }
            // Position in the list, not on the screen: the number printed
            // beside a row is the key that goes there, and it stays that key
            // once the list scrolls.
            KeyCode::Char(c @ '1'..='9') => {
                let index = c as usize - '1' as usize;
                self.jump_to_bookmark(index);
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                self.remove_selected_bookmark();
            }
            KeyCode::Char('P') => self.prune_bookmarks(),
            _ => {}
        }
    }

    /// Takes the active pane to the nth bookmark and closes the popup.
    ///
    /// A bookmark whose target is gone is reported and the popup stays open, so
    /// it can be removed there and then rather than left to fail again.
    fn jump_to_bookmark(&mut self, index: usize) {
        let Some(row) = self.bookmarks_view().and_then(|v| v.row(index)).cloned() else {
            return;
        };

        // The row's state was settled when the popup opened, and the popup can
        // stay up for a long time. One `stat` on a keypress is cheap, and
        // trusting the cached answer would land the pane in a directory that
        // is no longer there — `reveal` assigns the path unconditionally and
        // the listing would just come back empty, with nothing said.
        let state = crate::bookmarks::PathState::of(&row.path);
        if state.is_missing() {
            self.refresh_bookmarks_view();
            self.err_status(format!("Bookmark is gone: {}", row.path.display()));
            return;
        }

        self.overlay = None;
        self.reveal(&row.path, state.is_dir());
    }

    /// Re-reads the store and re-checks every path, keeping the cursor where
    /// it was.
    fn refresh_bookmarks_view(&mut self) {
        let fresh = BookmarksView::new(&self.bookmarks);
        if let Some(view) = self.bookmarks_view_mut() {
            let selected = view.selected_idx();
            *view = fresh;
            if let Some(i) = selected {
                view.list_state.select(Some(i));
                view.clamp_selection();
            }
        }
    }

    fn remove_selected_bookmark(&mut self) {
        // By path, not by index: the view's row order only happens to match
        // the store's, and an index that drifted would silently remove the
        // wrong bookmark.
        let Some(path) = self
            .bookmarks_view()
            .and_then(|v| v.selected_idx().and_then(|i| v.row(i)))
            .map(|row| row.path.clone())
        else {
            return;
        };
        if !self.bookmarks.remove(&path) {
            return;
        }
        if !self.save_bookmarks() {
            return;
        }

        // Rebuilt from the store rather than patched in two places, so the
        // list and the file cannot disagree.
        self.refresh_bookmarks_view();
        self.ok_status(format!("Bookmark removed: {}", ops::file_name_of(&path)));
    }

    fn prune_bookmarks(&mut self) {
        let gone = self.bookmarks.prune_missing();

        // Refreshed even when nothing was pruned: the rows were checked when
        // the popup opened, so a bookmark whose target came back would
        // otherwise keep saying `(missing)` until the popup was reopened.
        if gone == 0 {
            self.refresh_bookmarks_view();
            self.ok_status("No missing bookmarks".to_string());
            return;
        }

        if !self.save_bookmarks() {
            return;
        }
        self.refresh_bookmarks_view();
        self.ok_status(format!("{gone} missing bookmark(s) removed"));
    }

    fn handle_trash_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(tv) = self.trash_view_mut() {
                    tv.move_up();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(tv) = self.trash_view_mut() {
                    tv.move_down();
                }
            }
            KeyCode::Char('x') => {
                if let Some(tv) = self.trash_view_mut() {
                    tv.toggle_select();
                    tv.move_down();
                }
            }
            // Restore selected/highlighted item(s).
            KeyCode::Char('r') => {
                let Some(tv) = self.trash_view() else {
                    return;
                };
                match tv.restore_targets() {
                    Ok(n) => {
                        self.ok_status(format!("{n} item(s) restored"));
                        self.overlay = Some(Overlay::Trash(TrashView::load())); // refresh
                        self.panes.reload(&self.config, true);
                    }
                    Err(e) => self.err_status(e),
                }
            }
            // Permanently delete selected/highlighted item(s).
            KeyCode::Char('D') => {
                let Some(tv) = self.trash_view() else {
                    return;
                };
                match tv.purge_targets() {
                    Ok(n) => {
                        self.ok_status(format!("{n} item(s) permanently deleted"));
                        self.overlay = Some(Overlay::Trash(TrashView::load())); // refresh
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
                self.overlay = None;
            }
            KeyCode::Enter => {
                let Some(br) = self.bulk_rename() else {
                    return;
                };
                if !br.is_valid() {
                    return;
                }
                let pairs = br.rename_pairs();
                self.overlay = None;
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
            _ => {
                if let Some(br) = self.bulk_rename_mut()
                    && br.pattern.handle_key(key) == TextEdit::Changed
                {
                    br.update_preview();
                }
            }
        }
    }

    fn handle_permissions_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Enter => self.apply_permissions(),
            KeyCode::Tab => {
                if let Some(pe) = self.permissions_editor_mut() {
                    pe.next_field();
                }
            }
            KeyCode::BackTab => {
                if let Some(pe) = self.permissions_editor_mut() {
                    pe.prev_field();
                }
            }
            _ => {
                let Some(pe) = self.permissions_editor_mut() else {
                    return;
                };
                match pe.focus {
                    // The grid owns Left/Right/Up/Down/Space/digits/Backspace
                    // here — there is no text cursor to move, only the
                    // highlighted cell.
                    Field::Mode => match key.code {
                        KeyCode::Left | KeyCode::Char('h') => pe.move_cursor(0, -1),
                        KeyCode::Right | KeyCode::Char('l') => pe.move_cursor(0, 1),
                        KeyCode::Up | KeyCode::Char('k') => pe.move_cursor(-1, 0),
                        KeyCode::Down | KeyCode::Char('j') => pe.move_cursor(1, 0),
                        KeyCode::Char(' ') => pe.toggle_bit(),
                        KeyCode::Backspace => pe.backspace_mode(),
                        KeyCode::Char(c @ '0'..='7') => pe.type_digit(c as u8 - b'0'),
                        _ => {}
                    },
                    Field::Owner => {
                        pe.owner.handle_key(key);
                    }
                    Field::Group => {
                        pe.group.handle_key(key);
                    }
                }
            }
        }
    }

    /// Applies the permissions popup: chmod, then chown (only the fields
    /// that were not left blank), across every target. Stops and reports
    /// rather than closing when a name does not resolve, so a typo can be
    /// fixed instead of silently doing nothing.
    fn apply_permissions(&mut self) {
        let Some(pe) = self.permissions_editor_mut() else {
            return;
        };

        let mode = pe.resolved_mode();
        let (owner, group) = match (pe.resolved_owner(), pe.resolved_group()) {
            (Ok(owner), Ok(group)) => (owner, group),
            (Err(e), _) | (_, Err(e)) => {
                pe.error = Some(e);
                return;
            }
        };
        let targets = pe.targets.clone();

        self.overlay = None;
        let mut errors = 0usize;
        for target in &targets {
            if let Err(e) = ops::chmod_entry(target, mode) {
                log::error!("chmod {}: {e}", target.display());
                errors += 1;
                continue;
            }
            if (owner.is_some() || group.is_some())
                && let Err(e) = ops::chown_entry(target, owner, group)
            {
                log::error!("chown {}: {e}", target.display());
                errors += 1;
            }
        }

        self.panes.reload(&self.config, false);
        if errors == 0 {
            self.ok_status(format!("{} file(s) updated", targets.len()));
        } else {
            self.err_status(format!("{errors} update(s) failed"));
        }
    }

    fn start_find_in_files(&mut self, pattern: String) {
        if self.find_in_files().is_none() {
            return;
        }

        // Compiled before the search is marked as started, so a bad pattern
        // leaves the popup exactly as it was rather than recording a query that
        // was never run.
        let re = match regex::Regex::new(&pattern) {
            Ok(re) => re,
            Err(_) => {
                self.err_status("Invalid regex pattern".to_string());
                return;
            }
        };

        // Get the current directory
        let search_dir = PathBuf::from(&self.panes.get_active_pane().path);
        // The same rules the file finder uses, so the two searches agree about
        // which files exist.
        let search_filter = SearchFilter::from_config(&self.config);

        let Some(find) = self.find_in_files_mut() else {
            return;
        };
        find.start_search(pattern);
        find.set_root(search_dir.clone());
        find.set_filter_label(search_filter.describe());

        // Walk the directory tree and search file contents
        let walker = search_filter.walk(&search_dir);

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
        let Some(find) = self.find_in_files_mut() else {
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

    /// Puts a dialog on screen, replacing whatever overlay was there.
    pub(crate) fn open_dialog(&mut self, dialog: Dialog) {
        self.overlay = Some(Overlay::Dialog(dialog));
    }

    /// Puts a preview on screen, replacing whatever overlay was there.
    fn open_preview(&mut self, preview: PopupPreview) {
        self.overlay = Some(Overlay::Preview(Box::new(preview)));
    }

    /// Swaps the active theme and rebuilds everything derived from it.
    pub(crate) fn set_theme(&mut self, theme: Theme) {
        self.syn_theme = std::sync::Arc::new(theme.to_syntect_theme());
        self.theme = theme;
        // Drop an entry-bound preview so it is rebuilt with the new colours.
        // Free-text previews (`:!` output) cannot be rebuilt, so they stay.
        if self.preview().is_some_and(|p| p.selected().is_some()) {
            self.overlay = None;
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
            (
                DialogAction::Copy {
                    sources,
                    base,
                    dest_dir,
                },
                DialogResult::Confirmed,
            ) => {
                self.transfer_entries(Transfer::Copy, sources, base, dest_dir);
            }
            (
                DialogAction::Move {
                    sources,
                    base,
                    dest_dir,
                },
                DialogResult::Confirmed,
            ) => {
                self.transfer_entries(Transfer::Move, sources, base, dest_dir);
            }
            (
                DialogAction::PasteMove {
                    sources,
                    base,
                    dest_dir,
                },
                DialogResult::Confirmed,
            ) => {
                self.transfer_entries(Transfer::Move, sources, base, dest_dir);
                self.clipboard.clear();
                self.clipboard_cut = false;
            }
            (
                DialogAction::ExtractArchive {
                    archive_path,
                    kind,
                    names,
                    dest_dir,
                    total,
                },
                DialogResult::Confirmed,
            ) => {
                self.run_archive_extract(archive_path, kind, names, dest_dir, total);
            }
            (DialogAction::CreateSymlink { pairs }, DialogResult::Confirmed) => {
                self.create_symlinks(pairs);
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
            self.open_dialog(Dialog::confirm(
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
        if self.archive_write_blocked() {
            return;
        }
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

        self.open_dialog(Dialog::input(
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
            self.open_dialog(Dialog::confirm(
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
        if self.archive_write_blocked() {
            return;
        }
        let targets = self.op_targets();
        if targets.is_empty() {
            return;
        }

        let message = if targets.len() == 1 {
            format!("Move '{}' to trash?", ops::file_name_of(&targets[0]))
        } else {
            format!("Move {} items to trash?", targets.len())
        };

        self.open_dialog(Dialog::confirm(
            "Delete",
            message,
            DialogAction::Trash { paths: targets },
        ));
    }

    fn trash_entries(&mut self, paths: Vec<PathBuf>) {
        for path in &paths {
            if let Err(e) = trash::delete(path) {
                self.open_dialog(Dialog::confirm(
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

    /// `true` (after leaving a footer error) when the active pane is
    /// browsing inside an archive, where nothing but navigating, selecting
    /// and extracting (`Copy`) is allowed.
    fn archive_write_blocked(&mut self) -> bool {
        if !self.panes.get_active_pane().is_writable() {
            self.err_status("Read-only inside an archive".to_string());
            true
        } else {
            false
        }
    }

    /// `true` (after leaving a footer error) when the inactive pane — the
    /// destination of a copy/move/paste — is browsing inside an archive.
    fn archive_paste_target_blocked(&mut self) -> bool {
        if self.panes.get_inactive_pane().is_archive() {
            self.err_status("Cannot paste into an archive".to_string());
            true
        } else {
            false
        }
    }

    fn start_copy(&mut self) {
        if self.panes.get_active_pane().is_archive() {
            self.start_archive_extract();
            return;
        }
        self.start_transfer_op(Transfer::Copy);
    }

    fn start_move(&mut self) {
        // Nothing can be removed from a read-only archive, so a "move" out of
        // one would really just be a copy — refused rather than silently
        // reinterpreted.
        if self.archive_write_blocked() {
            return;
        }
        self.start_transfer_op(Transfer::Move);
    }

    /// Checks the targets, then either asks about overwrites or gets on with
    /// it. Copy and move used to be two functions differing by three lines.
    fn start_transfer_op(&mut self, transfer: Transfer) {
        if self.archive_paste_target_blocked() {
            return;
        }

        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let base = PathBuf::from(&self.panes.get_active_pane().path);
        let dest_dir = self.panes.get_inactive_pane().cursor_dir();

        let conflicts = match check_and_count_conflicts(&sources, &base, &dest_dir) {
            Ok(conflicts) => conflicts,
            Err(msg) => return self.err_status(msg),
        };

        if conflicts > 0 {
            self.open_dialog(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist in the other pane. Overwrite?"),
                transfer.dialog_action(sources, base, dest_dir),
            ));
        } else {
            self.transfer_entries(transfer, sources, base, dest_dir);
        }
    }

    /// Transfers larger than this run in the background with a progress gauge.
    const ASYNC_THRESHOLD_BYTES: u64 = 10 * 1024 * 1024;

    fn transfer_entries(
        &mut self,
        transfer: Transfer,
        sources: Vec<PathBuf>,
        base: PathBuf,
        dest_dir: PathBuf,
    ) {
        // A move within one filesystem is a rename, so take those first: they
        // cost one syscall each and leave nothing to size up or copy. Only
        // what `rename` cannot do — a cross-device move, a directory to merge
        // into — reaches the walk below.
        let sources = match self.rename_movable_sources(transfer, sources, &base, &dest_dir) {
            Ok(remaining) => remaining,
            Err(()) => return,
        };

        if sources.is_empty() {
            self.panes.reload(&self.config, false);
            self.ok_status(transfer.done_label().to_string());
            return;
        }

        let total = ops::total_size(&sources);
        if total > Self::ASYNC_THRESHOLD_BYTES {
            self.start_transfer(sources, base, dest_dir, transfer.is_cut(), total);
            return;
        }

        for src in &sources {
            let result = ops::prepare_dest_dir(src, &base, &dest_dir)
                .and_then(|target| transfer.apply(src, &target));
            if let Err(e) = result {
                let verb = transfer.verb();
                self.err_status(format!("Cannot {verb} '{}': {e}", src.display()));
                return;
            }
        }

        self.panes.reload(&self.config, false);
        self.ok_status(transfer.done_label().to_string());
    }

    /// Settles a move's cheap half: everything `rename` can shift on its own
    /// goes now, and the sources that still need copying come back.
    ///
    /// A copy passes straight through — it has bytes to write either way.
    /// `Err(())` means the failure has already been reported to the footer.
    fn rename_movable_sources(
        &mut self,
        transfer: Transfer,
        sources: Vec<PathBuf>,
        base: &Path,
        dest_dir: &Path,
    ) -> Result<Vec<PathBuf>, ()> {
        if !transfer.is_cut() {
            return Ok(sources);
        }

        ops::rename_movable(&sources, base, dest_dir).map_err(|e| {
            // Earlier sources may already have moved, so show the real state.
            self.panes.reload(&self.config, false);
            self.err_status(format!("Cannot {}: {e}", transfer.verb()));
        })
    }

    /// Starts a background copy (cut=false) or move (cut=true) with a progress
    /// gauge. The transfer is cancellable with Esc.
    ///
    /// `total` is the caller's already-walked size — walking again here would
    /// mean a second full pass over the tree just to fill in the gauge.
    fn start_transfer(
        &mut self,
        sources: Vec<PathBuf>,
        base: PathBuf,
        dest_dir: PathBuf,
        is_cut: bool,
        total: u64,
    ) {
        let (rx, cancel) = ops::spawn_transfer(sources, base, dest_dir, is_cut);
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

    /// Symlinks the active pane's selection (or highlighted entry) into the
    /// other pane, each link pointing at the source's absolute path — same
    /// target/other-pane shape as `Copy`/`Move`, but nothing is read or
    /// written at the source, only a link created at the destination.
    fn start_create_symlink(&mut self) {
        if self.archive_write_blocked() || self.archive_paste_target_blocked() {
            return;
        }

        let sources = self.op_targets();
        if sources.is_empty() {
            return;
        }

        let dest_dir = self.panes.get_inactive_pane().cursor_dir();
        let pairs: Vec<(PathBuf, PathBuf)> = sources
            .iter()
            .map(|src| {
                // Absolute, but not otherwise resolved: `std::path::absolute`
                // only prepends the current directory when `src` is relative,
                // it does not follow symlinks the way `canonicalize` would —
                // linking to a symlink must not silently link to its target
                // instead.
                let target = std::path::absolute(src).unwrap_or_else(|_| src.clone());
                (target, dest_dir.join(ops::file_name_of(src)))
            })
            .collect();

        let conflicts = pairs.iter().filter(|(_, link)| link.exists()).count();
        if conflicts > 0 {
            self.open_dialog(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist in the other pane. Overwrite?"),
                DialogAction::CreateSymlink { pairs },
            ));
        } else {
            self.create_symlinks(pairs);
        }
    }

    fn create_symlinks(&mut self, pairs: Vec<(PathBuf, PathBuf)>) {
        let mut errors = 0usize;
        for (target, link) in &pairs {
            // `symlink` refuses to overwrite, unlike `fs::copy`/`rename` —
            // the conflicting name was already confirmed, so clear it first.
            // A real directory is left alone rather than deleted out from
            // under the user; `create_symlink` then fails on it naturally,
            // reported below same as any other error.
            if let Ok(meta) = std::fs::symlink_metadata(link)
                && !meta.is_dir()
            {
                let _ = std::fs::remove_file(link);
            }
            if let Err(e) = ops::create_symlink(target, link) {
                log::error!("symlink {} -> {}: {e}", link.display(), target.display());
                errors += 1;
            }
        }

        self.panes.reload(&self.config, false);
        if errors == 0 {
            self.ok_status(format!("{} symlink(s) created", pairs.len()));
        } else {
            self.err_status(format!("{errors} symlink(s) failed"));
        }
    }

    /// Extracts the archive pane's targets (selection, or the highlighted
    /// entry) into the other pane's real directory. The only write-shaped
    /// action a read-only archive pane allows.
    fn start_archive_extract(&mut self) {
        if self.archive_paste_target_blocked() {
            return;
        }

        let pane = self.panes.get_active_pane();
        let Some((archive_path, kind)) = pane.archive_source() else {
            return; // not actually in archive mode — nothing to do
        };
        let names: BTreeSet<String> = pane.archive_targets().into_iter().collect();
        if names.is_empty() {
            return;
        }
        let total = pane.archive_extract_size(&names);
        let dest_dir = self.panes.get_inactive_pane().cursor_dir();

        let conflicts = names
            .iter()
            .filter(|n| {
                let base = n.rsplit('/').next().unwrap_or(n);
                dest_dir.join(base).exists()
            })
            .count();

        if conflicts > 0 {
            self.open_dialog(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist in the other pane. Overwrite?"),
                DialogAction::ExtractArchive {
                    archive_path,
                    kind,
                    names,
                    dest_dir,
                    total,
                },
            ));
        } else {
            self.run_archive_extract(archive_path, kind, names, dest_dir, total);
        }
    }

    /// Starts the background extraction worker and shows the same progress
    /// gauge a regular transfer uses.
    fn run_archive_extract(
        &mut self,
        archive_path: PathBuf,
        kind: archive::ArchiveKind,
        names: BTreeSet<String>,
        dest_dir: PathBuf,
        total: u64,
    ) {
        let (rx, cancel) = archive::spawn_extract(archive_path, kind, names, dest_dir);
        self.progress = Some(super::Progress {
            title: "Extracting…".to_string(),
            total_bytes: total,
            done_bytes: 0,
            rx,
            cancel,
            is_cut: false,
        });
    }

    /// Yanks the operation targets (selection or highlighted entry) into the
    /// internal clipboard as a copy.
    fn yank(&mut self) {
        if self.archive_write_blocked() {
            return;
        }

        let targets = self.op_targets();
        if targets.is_empty() {
            return;
        }
        self.clipboard = targets;
        self.clipboard_base = PathBuf::from(&self.panes.get_active_pane().path);
        self.clipboard_cut = false;
        self.ok_status(format!("{} yanked", self.clipboard.len()));
    }

    /// Pastes the clipboard into the active pane's directory. `cut` moves
    /// instead of copying and clears the clipboard afterwards.
    fn paste(&mut self, cut: bool) {
        if self.clipboard.is_empty() || self.archive_write_blocked() {
            return;
        }

        let sources = self.clipboard.clone();
        // The layout is recreated relative to the pane the yank came from, not
        // the one pasting: that is where the sources' structure is meaningful.
        let base = self.clipboard_base.clone();
        let dest_dir = self.panes.get_active_pane().cursor_dir();

        let conflicts = match check_and_count_conflicts(&sources, &base, &dest_dir) {
            Ok(conflicts) => conflicts,
            Err(msg) => return self.err_status(msg),
        };

        if conflicts > 0 {
            let action = if cut {
                DialogAction::PasteMove {
                    sources,
                    base,
                    dest_dir,
                }
            } else {
                DialogAction::Copy {
                    sources,
                    base,
                    dest_dir,
                }
            };
            self.open_dialog(Dialog::confirm(
                "Overwrite?",
                format!("{conflicts} item(s) exist here. Overwrite?"),
                action,
            ));
            return;
        }

        if cut {
            self.transfer_entries(Transfer::Move, sources, base, dest_dir);
            self.clipboard.clear();
            self.clipboard_cut = false;
        } else {
            self.transfer_entries(Transfer::Copy, sources, base, dest_dir);
        }
    }

    fn handle_command_key(&mut self, key: &KeyEvent) {
        let Some(input) = self.command_mut() else {
            return;
        };

        match key.code {
            KeyCode::Enter => {
                if let Some(InputMode::Command(input)) = self.input_mode.take() {
                    self.completion = Completion::default();
                    self.run_command(&input.value.clone());
                }
                return;
            }
            KeyCode::Esc => {
                self.input_mode = None;
                self.completion = Completion::default();
                return;
            }
            // Tab walks the menu; the line follows the highlighted candidate.
            KeyCode::Tab | KeyCode::BackTab => {
                let forward = key.code == KeyCode::Tab;
                let line = input.value.clone();
                if let Some(completed) = self.completion.cycle(&line, forward)
                    && let Some(input) = self.command_mut()
                {
                    *input = TextInput::new(completed);
                }
                return;
            }
            // Note this rebuilds the menu on a cursor move too, not just on an
            // edit: that clears the Tab selection, so the next Tab starts
            // cycling from the top.
            _ => {
                if input.handle_key(key) == TextEdit::Ignored {
                    return;
                }
            }
        }

        // Editing the line invalidates the menu: rebuild it from scratch.
        self.refresh_completion();
    }

    /// Recomputes the completion menu for the current command line.
    pub(crate) fn refresh_completion(&mut self) {
        let Some(input) = self.command() else {
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
            "w" | "write" => match Config::save_config(&self.config, &self.config_path) {
                Ok(()) => self.ok_status("Configuration saved".to_string()),
                Err(e) => self.err_status(format!("Cannot save config: {e}")),
            },
            "so" | "source" => self.reload_config(),
            "e" | "cd" => self.navigate_to(arg),
            "mkdir" => {
                let parent = self.panes.get_active_pane().cursor_dir();
                self.mkdir(parent, arg.to_string());
            }
            "touch" => {
                let parent = self.panes.get_active_pane().cursor_dir();
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
            "help" => self.overlay = Some(Overlay::Keybinds),
            "term" => {
                if arg.is_empty() {
                    self.err_status("Usage: :term <command>".to_string());
                } else {
                    // Handled by the run loop, which can suspend the terminal.
                    self.pending_terminal_command = Some(self.expand_targets(arg));
                }
            }
            "trash" => {
                self.overlay = Some(Overlay::Trash(TrashView::load()));
            }
            "bookmarks" => self.open_bookmarks(),
            _ => {
                self.err_status(format!("Unknown command: {cmd}  (try :help)"));
            }
        }
    }

    /// `:source` — reload the config file at runtime and apply it (theme
    /// included when it changed).
    fn reload_config(&mut self) {
        // Cloned so the match does not hold a borrow of `self` across the arms.
        let path = self.config_path.clone();
        match Config::load_config_at(&path) {
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
                // Keybindings are rebuilt too, so `:so` is the way to iterate
                // on them without restarting.
                self.keymap = crate::ui::keymap::build_keymap(&self.config);
                self.footer.update_hints(&self.keymap);
                self.panes.reload(&self.config, false);
                self.ok_status("Config reloaded".to_string());
                self.report_keymap_warnings();
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
                self.sync_header();
            }
            _ => {
                self.err_status(format!("Not a directory: {arg}"));
            }
        }
    }

    fn switch_theme(&mut self, name: &str) {
        let known = Theme::get_theme_list();

        if name.is_empty() {
            self.open_dialog(Dialog::message(
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

    /// Expands `%f` to the shell-quoted paths of the selected (or highlighted)
    /// entries, so `:!wc -l %f` and `:term nvim %f` both work.
    pub(crate) fn expand_targets(&mut self, cmd: &str) -> String {
        if !cmd.contains("%f") {
            return cmd.to_string();
        }

        let quoted = self
            .op_targets()
            .iter()
            .map(|path| {
                let path = path.to_string_lossy();
                format!("'{}'", path.replace('\'', "'\\''"))
            })
            .collect::<Vec<_>>()
            .join(" ");

        cmd.replace("%f", &quoted)
    }

    fn run_shell_capture(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }

        let cmd = self.expand_targets(cmd);
        let cmd = cmd.as_str();

        // Whatever it does, assume it touched the screen: a program that wants
        // a terminal opens /dev/tty and draws there even though its stdout is
        // a pipe, which is why `:!lazygit` used to come back to a broken UI.
        self.pending_redraw = true;

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

                // Nothing to preview. Report it in the footer instead of
                // opening an empty popup: rodeo cannot tell "produced nothing"
                // from "wrote straight to the terminal", which is what every
                // interactive program does — it opens /dev/tty rather than
                // using the pipes captured here.
                if text.trim().is_empty() {
                    let label = elide(cmd, SHELL_LABEL_WIDTH);
                    match out.status.code() {
                        Some(0) | None => self
                            .ok_status(format!(":!{label} — no output (interactive? use :term)")),
                        Some(code) => self.err_status(format!(":!{label} — exit {code}")),
                    }
                    return;
                }

                let lines: Vec<ratatui::text::Line> = text
                    .lines()
                    .map(|l| ratatui::text::Line::from(l.to_string()))
                    .collect();

                // Output opens as a scrollable, selection-independent preview.
                self.open_preview(PopupPreview::from_text(
                    format!(":!{cmd}"),
                    ratatui::text::Text::from(lines),
                ));

                // A failing command still gets its output shown, but the
                // footer says it failed.
                if let Some(code) = out.status.code().filter(|c| *c != 0) {
                    self.err_status(format!(":!{} — exit {code}", elide(cmd, SHELL_LABEL_WIDTH)));
                }
            }
            Err(e) => {
                self.err_status(format!("Cannot run shell: {e}"));
            }
        }
    }

    // Keys checked in order: popup-specific → Ctrl-modified → Shift-modified → unmodified.
    /// Keys claimed by the preview and keybinds popups.
    ///
    /// `false` lets the key fall through to the normal bindings, which is what
    /// keeps the panes usable while these two are on screen.
    fn handle_popup_key(&mut self, key: &KeyEvent) -> bool {
        // Dismiss keys are plain presses only — Ctrl+Space/Ctrl+q must not
        // close a popup.
        if Self::is_plain_key(key)
            && matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q')
            )
        {
            self.overlay = None;
            return true;
        }

        // Everything below belongs to the preview; the keybinds popup only
        // knows how to close.
        if !self.preview_open() {
            return false;
        }

        // Ctrl scrolls the preview itself.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            let Some(preview) = self.preview_mut() else {
                return false;
            };
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => preview.row_next(),
                KeyCode::Up | KeyCode::Char('k') => preview.row_prev(),
                KeyCode::Char('f') => preview.page_down(),
                KeyCode::Char('b') => preview.page_up(),
                KeyCode::Char('d') => preview.half_page_down(),
                KeyCode::Char('u') => preview.half_page_up(),
                _ => return false,
            }
            return true;
        }

        if Self::is_plain_key(key) {
            match key.code {
                // Moving the pane selection; the preview follows it.
                KeyCode::Up | KeyCode::Char('k') => self.panes.goto_next(MoveDirection::Up),
                KeyCode::Down | KeyCode::Char('j') => self.panes.goto_next(MoveDirection::Down),
                KeyCode::Char('w') => {
                    if let Some(preview) = self.preview_mut() {
                        preview.toggle_wrap();
                    }
                }
                _ => return false,
            }
            return true;
        }

        false
    }

    fn handle_main_key(&mut self, key: &KeyEvent) {
        // Esc is hardcoded: it drives the universal dismiss chain.
        if key.code == KeyCode::Esc {
            self.handle_esc();
            return;
        }

        let Some(binding) = self.keymap.binding_for(key).cloned() else {
            log::debug!("unbound key {:?}+{:?}", key.modifiers, key.code);
            return;
        };

        let action = match binding {
            Binding::Action(action) => action,
            // A key bound to a command runs exactly what typing it after `:`
            // would have done, so `%f` and completion-era commands all work.
            Binding::Command(command) => {
                self.run_command(&command);
                return;
            }
        };

        // One line per action. Anything that needs more than a call belongs in
        // a named method below — this used to mix twenty one-line arms with
        // eight inline blocks, and the blocks were where the logic hid.
        match action {
            Action::OpenEntry => self.open_entry(),
            Action::ParentDir => self.go_to_parent(),
            Action::GotoFirst => self.panes.goto_first(),
            Action::GotoLast => self.panes.goto_last(),
            Action::ToggleSelect => self.panes.get_active_pane_mut().toggle_select(),
            Action::SelectGlob => self.prompt_select_glob(),
            Action::DirSizes => self.compute_dir_sizes(),
            Action::BulkRename => self.start_bulk_rename(),
            Action::Quit => self.exit = true,
            Action::PaneLeft => self.panes.set_active_pane(ActivePane::Left),
            Action::PaneRight => self.panes.set_active_pane(ActivePane::Right),
            Action::PaneToggle => self.toggle_pane(),
            Action::Help => self.toggle_keybinds(),
            Action::Preview => self.toggle_preview(),
            Action::MoveDown => self.panes.goto_next(MoveDirection::Down),
            Action::MoveUp => self.panes.goto_next(MoveDirection::Up),
            Action::Rename => self.start_rename(),
            Action::Search => self.open_file_finder(),
            Action::CommandPalette => self.open_command_line(),
            Action::Create => self.prompt_create(),
            Action::Yank => self.yank(),
            Action::Paste => self.paste(false),
            Action::PasteMove => self.paste(true),
            Action::DeleteChord => self.delete_chord(),
            Action::Copy => self.start_copy(),
            Action::Move => self.start_move(),
            Action::Delete => self.start_delete(),
            Action::SelectAll => self.select_all(),
            Action::FilterRegex => self.open_filter_bar(),
            Action::FindInFiles => self.open_find_in_files(),
            Action::ToggleHidden => self.toggle_hidden(),
            Action::Refresh => self.panes.reload(&self.config, false),
            Action::SortNext => self.set_sort_type(self.config.sort_type.next()),
            Action::SortPrev => self.set_sort_type(self.config.sort_type.prev()),
            Action::SortReverse => self.set_sort_order(self.config.sort_order.reversed()),
            Action::BookmarkToggle => self.toggle_bookmark(),
            Action::Bookmarks => self.open_bookmarks(),
            Action::Permissions => self.start_permissions_editor(),
            Action::CreateSymlink => self.start_create_symlink(),
            Action::ToggleTree => self.toggle_tree(),
            Action::TreeExpand => self.tree_step(true),
            Action::TreeCollapse => self.tree_step(false),
        }
    }

    /// Opens or closes a tree node, reloading only when the rows actually
    /// changed — stepping the cursor in or out needs no rebuild.
    fn tree_step(&mut self, expand: bool) {
        let pane = self.panes.get_active_pane_mut();
        let changed = if expand {
            pane.tree_expand()
        } else {
            pane.tree_collapse()
        };

        if changed {
            self.panes.reload(&self.config, false);
        }
    }

    /// Switches the active pane between the flat listing and the tree.
    fn toggle_tree(&mut self) {
        if !self.panes.get_active_pane_mut().toggle_tree() {
            self.err_status("No tree view inside an archive".to_string());
            return;
        }

        self.panes.reload(&self.config, true);
        self.sync_header();
    }

    /// Opens the highlighted entry: a directory in the pane, a file in
    /// `$EDITOR`.
    fn open_entry(&mut self) {
        match self.panes.get_active_pane_mut().open() {
            OpenAction::DirectoryOpened | OpenAction::Reload => {
                self.panes.reload(&self.config, true);
                self.sync_header();
            }
            // The pane has not moved, so the flagged selection is still about
            // entries the user can see and must survive the rebuild.
            OpenAction::TreeChanged => self.panes.reload(&self.config, false),
            OpenAction::FileOpened(path) => {
                self.pending_editor_file = Some(EditorTarget::new(path));
            }
            OpenAction::Nothing => {}
        }
    }

    fn go_to_parent(&mut self) {
        let path = self.panes.get_active_pane().path.clone();
        if let OpenAction::DirectoryOpened = self.panes.get_active_pane_mut().go_to_parent(&path) {
            self.panes.reload(&self.config, true);
            self.sync_header();
        }
    }

    fn prompt_select_glob(&mut self) {
        self.open_dialog(Dialog::input(
            "Select",
            "Wildcard pattern (* ?):",
            "",
            DialogAction::SelectGlob,
        ));
    }

    fn compute_dir_sizes(&mut self) {
        if self.archive_write_blocked() {
            return;
        }
        let count = self.panes.get_active_pane_mut().compute_dir_sizes();
        self.ok_status(format!("Sizes computed for {count} directories"));
    }

    /// Bulk rename works on the selection, or on the highlighted entry when
    /// nothing is selected. Fewer than two targets is not worth a popup.
    fn start_bulk_rename(&mut self) {
        if self.archive_write_blocked() {
            return;
        }
        let pane = self.panes.get_active_pane();
        let targets: Vec<PathBuf> = if pane.has_selections() {
            pane.selected_entries()
                .into_iter()
                .map(|e| e.path)
                .collect()
        } else {
            pane.get_selected_entry()
                .filter(|e| !matches!(e.kind, EntryKind::Parent))
                .map(|e| vec![e.path])
                .unwrap_or_default()
        };

        if targets.len() < 2 {
            self.err_status("Select 2+ files with x before bulk rename".to_string());
        } else {
            self.overlay = Some(Overlay::BulkRename(BulkRename::new(targets)));
        }
    }

    /// Opens the permissions/ownership popup on the selection, or the
    /// highlighted entry when nothing is selected — same shape as
    /// `op_targets`, but keeping the `Entry` around rather than just its
    /// path, since the popup seeds its fields from the first target's raw
    /// mode/uid/gid.
    fn start_permissions_editor(&mut self) {
        if self.archive_write_blocked() {
            return;
        }

        let pane = self.panes.get_active_pane();
        let selected = pane.selected_entries();
        let entries = if !selected.is_empty() {
            selected
        } else if let Some(entry) = pane.get_selected_entry()
            && matches!(
                entry.kind,
                EntryKind::File | EntryKind::Directory | EntryKind::Symlink
            )
        {
            vec![entry]
        } else {
            Vec::new()
        };

        let Some(first) = entries.first() else {
            return;
        };
        let (mode, uid, gid) = (first.raw_mode, first.raw_uid, first.raw_gid);
        let targets = entries.into_iter().map(|e| e.path).collect();

        self.overlay = Some(Overlay::Permissions(PermissionsEditor::new(
            targets, mode, uid, gid,
        )));
    }

    fn toggle_pane(&mut self) {
        self.panes.toggle_active_pane();
        self.sync_header();
    }

    fn toggle_keybinds(&mut self) {
        self.overlay = if self.keybinds_open() {
            None
        } else {
            Some(Overlay::Keybinds)
        };
    }

    fn toggle_preview(&mut self) {
        let Some(entry) = self.panes.get_active_pane().get_selected_entry() else {
            return;
        };

        match entry.kind {
            EntryKind::File | EntryKind::Directory | EntryKind::Symlink => {
                if self.preview_open() {
                    self.overlay = None;
                } else {
                    self.open_preview(PopupPreview::new(Some(entry), self.syn_theme.clone()));
                }
            }
            EntryKind::Parent => log::warn!("Cannot preview parent directory."),
            EntryKind::Unknown => log::warn!("Unknown file type - cannot preview"),
        }
    }

    fn open_file_finder(&mut self) {
        let root = PathBuf::from(&self.panes.get_active_pane().path);
        let filter = SearchFilter::from_config(&self.config);
        self.overlay = Some(Overlay::FindFiles(FileFinder::new(
            root,
            &filter,
            self.syn_theme.clone(),
        )));
    }

    fn open_command_line(&mut self) {
        self.input_mode = Some(InputMode::Command(TextInput::default()));
        self.refresh_completion();
    }

    fn prompt_create(&mut self) {
        if self.archive_write_blocked() {
            return;
        }
        let parent = self.panes.get_active_pane().cursor_dir();
        self.open_dialog(Dialog::input(
            "Create",
            "File name  (end with / for a directory):",
            "",
            DialogAction::Create { parent },
        ));
    }

    /// `dd` deletes; a lone `d` arms the chord and waits for the second key.
    fn delete_chord(&mut self) {
        if self.pending_d {
            self.pending_d = false;
            self.start_delete();
        } else {
            self.pending_d = true;
        }
    }

    fn select_all(&mut self) {
        let count = self.panes.get_active_pane_mut().select_all();
        self.ok_status(format!("{count} selected"));
    }

    /// Re-opening the bar keeps the pattern that is in force, so a filter can
    /// be corrected instead of retyped.
    fn open_filter_bar(&mut self) {
        let initial = self
            .panes
            .get_active_pane()
            .filter()
            .map(|f| f.pattern().to_string())
            .unwrap_or_default();
        self.input_mode = Some(InputMode::Filter(Search::new(initial)));
    }

    fn open_find_in_files(&mut self) {
        self.overlay = Some(Overlay::FindInFiles(FindInFiles::new(
            self.syn_theme.clone(),
        )));
    }

    fn toggle_hidden(&mut self) {
        self.config.show_hidden = !self.config.show_hidden;
        self.panes.reload(&self.config, false);
    }

    fn set_sort_type(&mut self, sort_type: SortType) {
        self.config.sort_type = sort_type;
        self.panes.reload(&self.config, false);
    }

    fn set_sort_order(&mut self, sort_order: SortOrder) {
        self.config.sort_order = sort_order;
        self.panes.reload(&self.config, false);
    }

    /// The universal dismiss chain: back out of the innermost thing first.
    ///
    /// Esc deliberately does *not* quit. It used to, once there was nothing
    /// left to dismiss, which made one keypress mean either "clear my filter"
    /// or "exit" depending on state the user cannot see — press it twice out
    /// of reflex and rodeo was gone. Quitting is `q` or `:q`.
    fn handle_esc(&mut self) {
        if let Some(p) = &self.progress {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        } else if self.overlay.is_some() {
            self.overlay = None;
        } else if self.panes.get_active_pane().filter().is_some() {
            self.panes.get_active_pane_mut().clear_filter();
        } else if self.panes.get_active_pane().has_selections() {
            self.panes.get_active_pane_mut().clear_selections();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway app rooted in a temporary directory.
    ///
    /// The config path points inside `dir` so bookmarks are read from and
    /// written to the temporary directory: tests must never touch the
    /// developer's real `~/.config/rodeo/bookmarks.toml`.
    fn test_app(dir: &Path) -> App {
        let config = Config {
            initial_directory_left: dir.to_string_lossy().to_string(),
            initial_directory_right: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        let theme = Theme::load_theme(None).expect("default theme in themes/");
        App::new(theme, config, &dir.join("config.toml"), None)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    mod tree_view {
        use super::*;

        /// `a/deep.txt` alongside `b.txt`, in both panes.
        fn scratch() -> tempfile::TempDir {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("a")).unwrap();
            std::fs::write(dir.path().join("a/deep.txt"), b"deep").unwrap();
            std::fs::write(dir.path().join("b.txt"), b"top").unwrap();
            dir
        }

        fn names(app: &App) -> Vec<String> {
            app.panes
                .get_active_pane()
                .visible_entries()
                .map(|e| e.name.clone())
                .collect()
        }

        /// The whole feature, through the real keymap: `t` for the tree, the
        /// cursor onto `a`, `Right` to open it.
        #[test]
        fn t_opens_a_tree_and_right_expands_a_directory() {
            let dir = scratch();
            let mut app = test_app(dir.path());

            app.dispatch_key(&key(KeyCode::Char('t'), KeyModifiers::NONE));
            assert!(app.panes.get_active_pane().is_tree());
            assert_eq!(names(&app), vec!["a", "b.txt"]);

            app.dispatch_key(&key(KeyCode::Right, KeyModifiers::NONE));
            assert_eq!(names(&app), vec!["a", "deep.txt", "b.txt"]);

            app.dispatch_key(&key(KeyCode::Left, KeyModifiers::NONE));
            assert_eq!(names(&app), vec!["a", "b.txt"]);

            app.dispatch_key(&key(KeyCode::Char('t'), KeyModifiers::NONE));
            assert!(!app.panes.get_active_pane().is_tree());
            assert!(names(&app).contains(&"..".to_string()));
        }

        /// The point of preserving the layout: a file copied from inside the
        /// tree keeps the directory it lived in, rather than being flattened
        /// into the destination where names from different directories could
        /// collide.
        #[test]
        fn copying_from_inside_a_tree_recreates_the_directory_it_came_from() {
            let dir = scratch();
            let dest = tempfile::tempdir().unwrap();
            let config = Config {
                initial_directory_left: dir.path().to_string_lossy().to_string(),
                initial_directory_right: dest.path().to_string_lossy().to_string(),
                ..Default::default()
            };
            let theme = Theme::load_theme(None).expect("default theme in themes/");
            let mut app = App::new(theme, config, &dir.path().join("config.toml"), None);

            app.dispatch_key(&key(KeyCode::Char('t'), KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Right, KeyModifiers::NONE));
            // Cursor onto the nested file, then copy to the other pane.
            app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));
            assert_eq!(
                app.panes
                    .get_active_pane()
                    .get_selected_entry()
                    .unwrap()
                    .name,
                "deep.txt"
            );
            app.dispatch_key(&key(KeyCode::Char('Y'), KeyModifiers::SHIFT));

            assert_eq!(
                std::fs::read_to_string(dest.path().join("a/deep.txt")).unwrap(),
                "deep"
            );
            assert!(
                !dest.path().join("deep.txt").exists(),
                "the file must not be flattened into the destination root"
            );
        }
    }

    /// Copy and move ran through two near-identical functions, so a fix to one
    /// could miss the other. They share one path now; these pin the parts that
    /// have to stay different.
    mod transfers {
        use super::*;

        #[test]
        fn only_a_move_removes_the_source() {
            assert!(!Transfer::Copy.is_cut());
            assert!(Transfer::Move.is_cut());
        }

        #[test]
        fn each_reports_itself_in_the_footer() {
            assert_eq!(Transfer::Copy.done_label(), "Copied");
            assert_eq!(Transfer::Move.done_label(), "Moved");
            assert_eq!(Transfer::Copy.verb(), "copy");
            assert_eq!(Transfer::Move.verb(), "move");
        }

        #[test]
        fn each_confirms_with_its_own_dialog_action() {
            let sources = vec![PathBuf::from("/a")];
            let base = PathBuf::from("/");
            let dest = PathBuf::from("/b");

            assert!(matches!(
                Transfer::Copy.dialog_action(sources.clone(), base.clone(), dest.clone()),
                DialogAction::Copy { .. }
            ));
            assert!(matches!(
                Transfer::Move.dialog_action(sources, base, dest),
                DialogAction::Move { .. }
            ));
        }

        #[test]
        fn a_copy_leaves_the_source_and_a_move_does_not() {
            let dir = tempfile::tempdir().unwrap();
            let dest = dir.path().join("dest");
            std::fs::create_dir(&dest).unwrap();

            for (transfer, name, source_survives) in [
                (Transfer::Copy, "c.txt", true),
                (Transfer::Move, "m.txt", false),
            ] {
                let src = dir.path().join(name);
                std::fs::write(&src, "x").unwrap();

                transfer.apply(&src, &dest).expect("transfer");

                assert!(dest.join(name).exists(), "{transfer:?} did not arrive");
                assert_eq!(src.exists(), source_survives, "{transfer:?} source");
            }
        }
    }

    /// The preview and the keybinds popup used to be independent booleans, so
    /// pressing `?` with the preview open set both and drew one over the other.
    #[test]
    fn opening_help_replaces_the_preview_instead_of_stacking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.preview_open(), "space opens the preview");

        app.dispatch_key(&key(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.keybinds_open());
        assert!(!app.preview_open(), "only one overlay at a time");
    }

    /// Esc backs out of one thing at a time and stops there. It used to quit
    /// once the chain ran dry, so a reflexive second press killed the app.
    #[test]
    fn esc_dismisses_but_never_quits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut app = test_app(dir.path());
        let esc = key(KeyCode::Esc, KeyModifiers::NONE);

        app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.preview_open());

        app.dispatch_key(&esc);
        assert!(app.overlay.is_none(), "first Esc closes the popup");
        assert!(!app.exit);

        // Nothing left to dismiss: Esc is now a no-op, however often it comes.
        for _ in 0..5 {
            app.dispatch_key(&esc);
        }
        assert!(!app.exit, "Esc must never quit");

        // `q` still does.
        app.dispatch_key(&key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.exit);
    }

    /// Reported from a real session: Space, `?`, Ctrl+g used to leave the
    /// preview, the help table and the find-in-files popup all open, drawn on
    /// top of one another, because each was tracked separately and nothing
    /// closed the previous one.
    #[test]
    fn opening_a_third_popup_still_leaves_exactly_one_open() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));

        for (code, modifiers, expected) in [
            (KeyCode::Char(' '), KeyModifiers::NONE, OverlayKind::Preview),
            (
                KeyCode::Char('?'),
                KeyModifiers::SHIFT,
                OverlayKind::Keybinds,
            ),
            (
                KeyCode::Char('g'),
                KeyModifiers::CONTROL,
                OverlayKind::FindInFiles,
            ),
        ] {
            app.dispatch_key(&key(code, modifiers));
            assert_eq!(
                app.overlay_kind(),
                Some(expected),
                "{code:?} should leave only {expected:?} open"
            );
        }
    }

    #[test]
    fn preview_toggles_off_on_a_second_press() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.preview_open());

        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.overlay.is_none(), "a second press closes it");
    }

    /// The input bar is not an overlay: it sits under a popup, and it is
    /// checked first so it keeps the keys while it is open.
    #[test]
    fn the_command_line_opens_over_the_preview_and_takes_the_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('j'), KeyModifiers::NONE));
        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.preview_open());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::SHIFT));
        assert!(app.command().is_some(), "`:` still works under a popup");
        assert!(app.preview_open(), "and leaves the popup alone");

        // `q` now types into the command line instead of closing the popup.
        app.dispatch_key(&key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(app.command().map(|c| c.value.as_str()), Some("q"));
        assert!(app.preview_open());
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
        app.overlay = Some(Overlay::Keybinds);

        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::CONTROL));
        assert!(app.keybinds_open());

        app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.keybinds_open());
    }

    #[test]
    fn silent_command_reports_in_the_footer_instead_of_an_empty_preview() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // `true` succeeds and prints nothing — exactly the lazygit case, where
        // the program wrote to /dev/tty and the capture saw nothing.
        app.run_command("!true");

        assert!(app.preview().is_none(), "no popup for an empty capture");
        assert!(!app.preview_open());
        let status = app.footer.status_text().expect("a footer message");
        assert!(status.contains("no output"), "{status}");
        assert!(
            status.contains(":term"),
            "should point at the fix: {status}"
        );
    }

    #[test]
    fn failing_silent_command_reports_its_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("!exit 3");

        assert!(app.preview().is_none());
        let status = app.footer.status_text().expect("a footer message");
        assert!(status.contains("exit 3"), "{status}");
    }

    #[test]
    fn command_with_output_still_opens_the_preview() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("!echo hello");

        assert!(app.preview_open());
        assert!(app.preview().is_some());
    }

    #[test]
    fn long_commands_are_elided_in_the_footer() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("!true # aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let status = app.footer.status_text().unwrap();
        assert!(status.contains('…'), "{status}");
        assert!(status.len() < 70, "footer message too long: {status}");
    }

    #[test]
    fn captured_command_forces_a_repaint() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("!true");

        // rodeo cannot tell whether the child drew on the terminal through
        // /dev/tty, so it must assume it did.
        assert!(app.pending_redraw);
    }

    #[test]
    fn shell_command_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("shell");

        assert!(
            app.footer
                .status_text()
                .unwrap()
                .contains("Unknown command"),
            "`:shell` was removed in favour of `:term $SHELL`"
        );
    }

    #[test]
    fn term_defers_to_the_run_loop() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("term lazygit");

        // The run loop owns the terminal, so the command is only queued here.
        assert_eq!(app.pending_terminal_command.as_deref(), Some("lazygit"));
        assert!(app.preview().is_none());
    }

    #[test]
    fn term_without_a_command_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.run_command("term");

        assert!(app.pending_terminal_command.is_none());
        assert!(app.footer.status_text().unwrap().contains("Usage"));
    }

    #[test]
    fn term_expands_the_selection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let mut app = test_app(dir.path());
        app.panes.goto_next(MoveDirection::Down);

        app.run_command("term nvim %f");

        let queued = app.pending_terminal_command.as_deref().unwrap();
        assert!(queued.starts_with("nvim '"), "{queued}");
        assert!(queued.contains("a.txt"), "{queued}");
    }

    fn app_with_bindings(dir: &Path, bindings: &[(&str, &str)]) -> App {
        let config = Config {
            initial_directory_left: dir.to_string_lossy().to_string(),
            initial_directory_right: dir.to_string_lossy().to_string(),
            keybindings: bindings
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        };
        App::new(
            Theme::builtin().expect("built-in theme"),
            config,
            &dir.join("config.toml"),
            None,
        )
    }

    #[test]
    fn a_key_can_be_bound_to_a_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_bindings(dir.path(), &[("z", ":!echo hi")]);

        app.dispatch_key(&key(KeyCode::Char('z'), KeyModifiers::NONE));

        assert!(app.preview_open(), "the command should have run");
    }

    #[test]
    fn modified_keys_are_bindable_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_bindings(dir.path(), &[("ctrl+r", ":!echo hi")]);

        app.dispatch_key(&key(KeyCode::Char('r'), KeyModifiers::CONTROL));

        assert!(app.preview_open());
    }

    #[test]
    fn built_in_modified_keys_still_work() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // Ctrl+g used to be hardcoded; it now comes from the same table.
        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert!(app.find_in_files().is_some());
    }

    /// `--config` has to hold for the whole session: `:w` and `:so` used to
    /// resolve the default location afresh, so a session started on one file
    /// saved to and reloaded from another.
    mod the_config_in_force {
        use super::*;

        #[test]
        fn write_saves_to_the_config_that_was_loaded() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rodeo.toml");
            let mut app = test_app(dir.path());
            app.config_path = path.clone();

            app.run_command("w");

            assert!(path.exists(), "`:w` wrote somewhere else");
            assert_eq!(app.footer.status_text(), Some("Configuration saved"));
        }

        #[test]
        fn source_reloads_the_config_that_was_loaded() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rodeo.toml");
            let mut app = test_app(dir.path());
            app.config_path = path.clone();

            assert!(!app.config.show_hidden);
            std::fs::write(&path, "show_hidden = true").unwrap();
            app.run_command("so");

            assert!(app.config.show_hidden, "`:so` read a different file");
        }

        /// `:so` rebuilds the keymap, which is the point of it.
        #[test]
        fn source_picks_up_a_rebound_key_from_that_file() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rodeo.toml");
            let mut app = test_app(dir.path());
            app.config_path = path.clone();

            std::fs::write(&path, "[keybindings]\n\"Z\" = \"help\"\n").unwrap();
            app.run_command("so");

            press(&mut app, 'Z');
            assert!(app.keybinds_open());
        }

        /// A round trip through the same path: what `:w` writes, `:so` reads.
        #[test]
        fn what_write_saves_source_reads_back() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rodeo.toml");
            let mut app = test_app(dir.path());
            app.config_path = path.clone();

            app.config.show_hidden = true;
            app.run_command("w");

            app.config.show_hidden = false;
            app.run_command("so");

            assert!(app.config.show_hidden);
        }
    }

    fn press(app: &mut App, c: char) {
        app.dispatch_key(&key(KeyCode::Char(c), KeyModifiers::NONE));
    }

    /// Bookmarks: `b` toggles the entry under the cursor, `B` lists them, and
    /// the list is written out the moment it changes.
    mod bookmarks {
        use super::*;

        /// Three files and a subdirectory, so the cursor has somewhere to go.
        fn dir_with_entries() -> tempfile::TempDir {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.txt"), "a").unwrap();
            std::fs::write(dir.path().join("b.txt"), "b").unwrap();
            std::fs::create_dir(dir.path().join("sub")).unwrap();
            dir
        }

        fn press(app: &mut App, c: char) {
            app.dispatch_key(&key(KeyCode::Char(c), KeyModifiers::NONE));
        }

        /// The path under the cursor, whatever the listing order turned out
        /// to be.
        fn cursor_path(app: &App) -> PathBuf {
            app.panes
                .get_active_pane()
                .get_selected_entry()
                .expect("an entry under the cursor")
                .path
        }

        #[test]
        fn b_bookmarks_the_entry_under_the_cursor() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'j');
            let path = cursor_path(&app);
            press(&mut app, 'b');

            // A bookmark is stored canonicalized (`App::normalized`), so the
            // raw cursor path — built from the tempdir's own, possibly
            // symlinked, spelling — has to be resolved the same way before
            // comparing.
            assert!(app.bookmarks.contains(&path.canonicalize().unwrap()));
        }

        #[test]
        fn b_again_removes_the_bookmark() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'j');
            let path = cursor_path(&app);
            press(&mut app, 'b');
            press(&mut app, 'b');

            assert!(!app.bookmarks.contains(&path));
            assert!(app.bookmarks.is_empty());
        }

        /// `b` follows the marked entries like copy, move and delete do,
        /// rather than being the one action that ignores a selection.
        #[test]
        fn b_bookmarks_every_marked_entry_at_once() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'j');
            press(&mut app, 'x');
            press(&mut app, 'j');
            press(&mut app, 'x');
            press(&mut app, 'b');

            assert_eq!(app.bookmarks.len(), 2);
        }

        /// Toggling each target on its own would add some and drop others,
        /// which looks arbitrary. Adding wins while anything is left to add.
        #[test]
        fn a_partly_bookmarked_selection_bookmarks_the_rest_instead_of_clearing() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            // Bookmark one entry, then mark it and its neighbour.
            press(&mut app, 'j');
            press(&mut app, 'b');
            assert_eq!(app.bookmarks.len(), 1);

            press(&mut app, 'x');
            press(&mut app, 'j');
            press(&mut app, 'x');
            press(&mut app, 'b');

            assert_eq!(app.bookmarks.len(), 2);
        }

        /// Bookmarking the folder you are looking at needs no second key: the
        /// cursor on `..` means the pane's own directory, not its parent.
        #[test]
        fn b_on_the_parent_row_bookmarks_the_pane_directory() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            assert_eq!(
                app.panes
                    .get_active_pane()
                    .get_selected_entry()
                    .unwrap()
                    .kind,
                EntryKind::Parent,
                "the cursor starts on the parent row"
            );
            press(&mut app, 'b');

            // See the comment above: bookmarks are stored canonicalized.
            assert!(app.bookmarks.contains(&dir.path().canonicalize().unwrap()));
        }

        /// Bookmarks are not part of config.toml, so `:w` never saves them —
        /// each change has to reach the disk on its own.
        #[test]
        fn bookmarks_are_written_as_soon_as_they_change() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'j');
            press(&mut app, 'b');

            let file = dir.path().join("bookmarks.toml");
            assert!(file.exists(), "the file is written on the first bookmark");
            assert_eq!(crate::bookmarks::Bookmarks::load(&file), app.bookmarks);
        }

        #[test]
        fn bookmarks_are_read_back_when_the_app_starts() {
            let dir = dir_with_entries();
            let mut first = test_app(dir.path());
            press(&mut first, 'j');
            press(&mut first, 'b');

            let second = test_app(dir.path());

            assert_eq!(second.bookmarks, first.bookmarks);
        }

        #[test]
        fn capital_b_opens_the_bookmarks_popup() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'B');

            assert!(app.bookmarks_view().is_some());
        }

        #[test]
        fn the_bookmarks_command_opens_the_same_popup() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            app.run_command("bookmarks");

            assert!(app.bookmarks_view().is_some());
        }

        #[test]
        fn enter_in_the_bookmarks_popup_takes_the_pane_to_the_bookmark() {
            let dir = dir_with_entries();
            let sub = dir.path().join("sub");
            let mut app = test_app(dir.path());
            app.bookmarks.add(sub.clone());

            press(&mut app, 'B');
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert_eq!(app.panes.get_active_pane().path, sub.to_string_lossy());
            assert!(app.bookmarks_view().is_none(), "the popup closes on a jump");
        }

        /// A bookmarked file puts the pane in its directory with the file
        /// under the cursor, the same as the file finder does.
        #[test]
        fn jumping_to_a_bookmarked_file_selects_it_in_its_directory() {
            let dir = dir_with_entries();
            let file = dir.path().join("sub").join("deep.txt");
            std::fs::write(&file, "x").unwrap();
            let mut app = test_app(dir.path());
            app.bookmarks.add(file.clone());

            press(&mut app, 'B');
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert_eq!(cursor_path(&app), file);
        }

        #[test]
        fn a_number_key_jumps_straight_to_that_bookmark() {
            let dir = dir_with_entries();
            let sub = dir.path().join("sub");
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().to_path_buf());
            app.bookmarks.add(sub.clone());

            press(&mut app, 'B');
            press(&mut app, '2');

            assert_eq!(app.panes.get_active_pane().path, sub.to_string_lossy());
        }

        #[test]
        fn a_number_past_the_end_of_the_list_does_nothing() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().to_path_buf());

            press(&mut app, 'B');
            press(&mut app, '9');

            assert!(app.bookmarks_view().is_some(), "the popup stays open");
        }

        /// A dead bookmark must say so and stay put, so it can be removed
        /// there and then instead of failing again next time.
        #[test]
        fn jumping_to_a_missing_bookmark_reports_it_and_keeps_the_popup_open() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().join("long-gone"));

            press(&mut app, 'B');
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert!(app.bookmarks_view().is_some());
            assert_eq!(
                app.panes.get_active_pane().path,
                dir.path().to_string_lossy()
            );
        }

        #[test]
        fn a_bookmark_whose_target_is_gone_is_shown_as_missing() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().to_path_buf());
            app.bookmarks.add(dir.path().join("long-gone"));

            press(&mut app, 'B');

            assert_eq!(app.bookmarks_view().unwrap().missing_count(), 1);
        }

        #[test]
        fn d_removes_the_highlighted_bookmark_and_rewrites_the_file() {
            let dir = dir_with_entries();
            let kept = dir.path().join("sub");
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().to_path_buf());
            app.bookmarks.add(kept.clone());
            assert!(app.save_bookmarks());

            press(&mut app, 'B');
            press(&mut app, 'd');

            assert_eq!(app.bookmarks.paths(), std::slice::from_ref(&kept));
            assert_eq!(app.bookmarks_view().unwrap().rows.len(), 1);
            let on_disk = crate::bookmarks::Bookmarks::load(&dir.path().join("bookmarks.toml"));
            assert_eq!(on_disk.paths(), std::slice::from_ref(&kept));
        }

        /// Removing the last row used to leave the cursor pointing past the
        /// end of the list.
        #[test]
        fn removing_the_last_bookmark_leaves_a_usable_cursor() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());
            app.bookmarks.add(dir.path().to_path_buf());
            app.bookmarks.add(dir.path().join("sub"));

            press(&mut app, 'B');
            press(&mut app, 'j');
            press(&mut app, 'd');

            let view = app.bookmarks_view().unwrap();
            assert_eq!(view.rows.len(), 1);
            assert_eq!(view.selected_idx(), Some(0));
        }

        #[test]
        fn p_prunes_only_the_missing_bookmarks() {
            let dir = dir_with_entries();
            let alive = dir.path().join("sub");
            let mut app = test_app(dir.path());
            app.bookmarks.add(alive.clone());
            app.bookmarks.add(dir.path().join("long-gone"));
            app.bookmarks.add(dir.path().join("also-gone"));

            press(&mut app, 'B');
            press(&mut app, 'P');

            assert_eq!(app.bookmarks.paths(), [alive]);
            assert_eq!(app.bookmarks_view().unwrap().rows.len(), 1);
        }

        #[test]
        fn esc_closes_the_bookmarks_popup() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'B');
            app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));

            assert!(app.bookmarks_view().is_none());
        }

        /// A bookmark outlives the session that made it, so a relative path is
        /// no use: `rodeo --left .` used to write `"."` into the file.
        #[test]
        fn a_bookmarked_path_is_stored_absolute() {
            let dir = dir_with_entries();
            let config = Config {
                initial_directory_left: ".".to_string(),
                initial_directory_right: ".".to_string(),
                ..Default::default()
            };
            let theme = Theme::load_theme(None).expect("default theme in themes/");
            let mut app = App::new(theme, config, &dir.path().join("config.toml"), None);

            press(&mut app, 'b');

            let stored = &app.bookmarks.paths()[0];
            assert!(stored.is_absolute(), "{stored:?} must not be relative");
        }

        /// One directory reached two ways is one bookmark. Reached through a
        /// symlink it arrives under a different string, and without
        /// normalizing you get two entries for the same folder — and neither
        /// `b` can remove the other.
        #[test]
        fn one_directory_reached_two_ways_is_one_bookmark() {
            let dir = dir_with_entries();
            let real = dir.path().join("sub");
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();

            let mut app = test_app(dir.path());

            // Bookmark it under its real name...
            app.panes.get_active_pane_mut().select_by_path(&real);
            press(&mut app, 'b');
            assert_eq!(app.bookmarks.len(), 1);

            // ...then again through the symlink.
            app.panes.get_active_pane_mut().select_by_path(&link);
            press(&mut app, 'b');

            assert_eq!(
                app.bookmarks.len(),
                0,
                "the same directory, so the second press removed it"
            );
        }

        /// The row's state is settled when the popup opens, and the popup can
        /// stay up. Trusting it landed the pane in a directory that had gone,
        /// with an empty listing and nothing said.
        #[test]
        fn a_bookmark_that_dies_while_the_popup_is_open_still_refuses_to_jump() {
            let dir = dir_with_entries();
            let doomed = dir.path().join("doomed");
            std::fs::create_dir(&doomed).unwrap();
            let mut app = test_app(dir.path());
            app.bookmarks.add(doomed.clone());

            press(&mut app, 'B');
            assert!(!app.bookmarks_view().unwrap().rows[0].state.is_missing());

            std::fs::remove_dir(&doomed).unwrap();
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert!(app.bookmarks_view().is_some(), "the popup stays open");
            assert_eq!(
                app.panes.get_active_pane().path,
                dir.path().to_string_lossy()
            );
        }

        /// `P` used to return early when there was nothing to prune, leaving a
        /// row that had come back to life still labelled `(missing)`.
        #[test]
        fn p_refreshes_the_list_even_when_there_is_nothing_to_prune() {
            let dir = dir_with_entries();
            let later = dir.path().join("later");
            let mut app = test_app(dir.path());
            app.bookmarks.add(later.clone());

            press(&mut app, 'B');
            assert_eq!(app.bookmarks_view().unwrap().missing_count(), 1);

            std::fs::create_dir(&later).unwrap();
            press(&mut app, 'P');

            assert_eq!(app.bookmarks_view().unwrap().missing_count(), 0);
            assert_eq!(app.bookmarks.len(), 1, "nothing was pruned");
        }

        /// A read-only directory made `b` say "Bookmarked", write nothing, and
        /// lose the whole list on restart.
        #[test]
        fn a_bookmark_that_cannot_be_written_is_reported_not_claimed() {
            use std::os::unix::fs::PermissionsExt;

            let dir = dir_with_entries();
            let locked = dir.path().join("locked");
            std::fs::create_dir(&locked).unwrap();

            let config = Config {
                initial_directory_left: dir.path().to_string_lossy().to_string(),
                initial_directory_right: dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };
            let theme = Theme::load_theme(None).expect("default theme in themes/");
            let mut app = App::new(theme, config, &locked.join("config.toml"), None);

            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();

            press(&mut app, 'j');
            press(&mut app, 'b');

            // Running as root defeats the permission bits.
            if !locked.join("bookmarks.toml").exists() {
                let status = app.footer.status_text().unwrap_or_default().to_string();
                assert!(
                    status.starts_with("Cannot write"),
                    "a failed write must be reported, not claimed as success: {status:?}"
                );
            }

            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// `B` was bulk rename before bookmarks took the pair `b` / `B`.
        #[test]
        fn bulk_rename_answers_to_r_now() {
            let dir = dir_with_entries();
            let mut app = test_app(dir.path());

            press(&mut app, 'j');
            press(&mut app, 'x');
            press(&mut app, 'j');
            press(&mut app, 'x');
            press(&mut app, 'R');

            assert_eq!(app.overlay_kind(), Some(OverlayKind::BulkRename));
        }
    }

    /// A directory tree with matches in a subdirectory, for find-in-files.
    fn dir_with_contents() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("top.txt"), "alpha\nbeta\n").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("deep.txt"), "one\nalpha\n").unwrap();
        dir
    }

    fn type_pattern(app: &mut App, pattern: &str) {
        for c in pattern.chars() {
            app.dispatch_key(&key(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    #[test]
    fn typing_a_pattern_is_not_yet_a_verdict() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "alpha");

        // Nothing has been searched yet, so the popup must not report on it —
        // this is what made Ctrl+G look broken.
        let find = app.find_in_files().unwrap();
        assert!(!find.results_are_current());
        assert!(find.results.is_empty());
    }

    #[test]
    fn enter_searches_the_tree_below_the_active_pane() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "alpha");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        let find = app.find_in_files().unwrap();
        assert!(find.results_are_current());
        assert_eq!(find.results.len(), 2, "the subdirectory must be searched");
        assert!(find.results.iter().any(|m| m.path.ends_with("deep.txt")));
    }

    #[test]
    fn editing_the_pattern_searches_again_instead_of_opening_a_file() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "alpha");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        // Second query, typed over the first.
        for _ in 0..5 {
            app.dispatch_key(&key(KeyCode::Backspace, KeyModifiers::NONE));
        }
        type_pattern(&mut app, "beta");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            app.pending_editor_file.is_none(),
            "Enter on an edited query must not launch the editor"
        );
        let find = app.find_in_files().expect("popup stays open");
        assert_eq!(find.results.len(), 1);
        assert!(find.results[0].path.ends_with("top.txt"));
    }

    #[test]
    fn opening_a_result_carries_its_line_number() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "beta");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));
        // The list is current now, so Enter opens the selection.
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.find_in_files().is_none());
        let target = app.pending_editor_file.expect("editor queued");
        assert!(target.path.ends_with("top.txt"));
        assert_eq!(target.line, Some(2));
    }

    #[test]
    fn an_invalid_pattern_is_reported_without_recording_a_search() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "alpha[");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            app.footer
                .status_text()
                .unwrap()
                .contains("Invalid regex pattern")
        );
        let find = app.find_in_files().unwrap();
        assert!(
            !find.searching,
            "a rejected pattern must not hang the popup"
        );
        assert!(!find.results_are_current());
    }

    #[test]
    fn opening_a_file_from_a_pane_has_no_line() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());
        app.panes.goto_next(MoveDirection::Down);

        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        if let Some(target) = app.pending_editor_file {
            assert_eq!(target.line, None);
        }
    }

    #[test]
    fn conflicting_bindings_raise_an_alert() {
        let dir = tempfile::tempdir().unwrap();
        // `x` is the default for select, so taking it must not pass silently.
        let app = app_with_bindings(dir.path(), &[("x", ":term lazygit")]);

        let dialog = app.dialog().expect("a warning dialog");
        assert_eq!(dialog.title, "Keybindings");
    }

    #[test]
    fn a_clean_keymap_raises_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with_bindings(dir.path(), &[("z", ":term lazygit")]);

        assert!(app.dialog().is_none());
    }

    #[test]
    fn command_palette_offers_completions_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char(':'), KeyModifiers::NONE));

        assert!(app.command().is_some());
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
        assert_eq!(app.command().unwrap().value, "q");

        app.dispatch_key(&key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.command().unwrap().value, "quit");

        // Shift+Tab walks back.
        app.dispatch_key(&key(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.command().unwrap().value, "q");
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

        assert!(app.command().is_none());
        assert!(!app.completion.is_active());
    }

    #[test]
    fn shifted_characters_still_reach_the_keymap() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // '?' arrives with SHIFT on most layouts — it must still reach the
        // keymap and open the help popup.
        app.dispatch_key(&key(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert!(app.keybinds_open());

        // …and toggle it shut again, rather than being swallowed by the popup.
        app.dispatch_key(&key(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert!(!app.keybinds_open());
    }

    #[test]
    fn slash_opens_the_file_finder_and_typing_narrows_it() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        let finder = app.find_files().expect("the finder opens");
        // The whole tree below the pane is a candidate, subdirectories included.
        assert!(finder.scanned() >= 3, "{}", finder.scanned());

        type_pattern(&mut app, "deep");
        let finder = app.find_files().unwrap();
        let hit = finder.selected().expect("a match");
        assert!(hit.path.ends_with("sub/deep.txt"), "{hit:?}");
        // The pane is untouched while the popup is up: this is a search, not
        // a filter.
        assert!(app.panes.get_active_pane().filter().is_none());
    }

    #[test]
    fn the_finder_takes_a_regex_in_the_same_box() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        type_pattern(&mut app, r"^top\.");

        let finder = app.find_files().unwrap();
        let hits: Vec<_> = finder.results().map(|e| e.rel.clone()).collect();
        assert_eq!(hits, vec!["top.txt".to_string()]);
    }

    #[test]
    fn enter_in_the_finder_moves_the_pane_onto_the_file() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        type_pattern(&mut app, "deep");
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.find_files().is_none(), "the popup closes");
        // The pane lists the containing directory, with the file selected.
        assert!(app.panes.get_active_pane().path.ends_with("sub"));
        let selected = app.panes.get_active_pane().get_selected_entry().unwrap();
        assert_eq!(selected.name, "deep.txt");
        // Enter navigates; it does not fire up an editor behind the user's back.
        assert!(app.pending_editor_file.is_none());
    }

    #[test]
    fn ctrl_e_in_the_finder_opens_the_file_in_the_editor() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        type_pattern(&mut app, "deep");
        app.dispatch_key(&key(KeyCode::Char('e'), KeyModifiers::CONTROL));

        let target = app.pending_editor_file.as_ref().expect("editor target");
        assert!(target.path.ends_with("deep.txt"));
        assert!(app.find_files().is_none());
    }

    #[test]
    fn the_finder_obeys_the_configured_filter() {
        let dir = dir_with_contents();
        std::fs::write(dir.path().join(".secret"), "x").unwrap();
        let mut app = test_app(dir.path());
        app.config.filter_entries = vec!["sub".to_string()];

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        let finder = app.find_files().unwrap();
        let names: Vec<_> = finder.results().map(|e| e.rel.clone()).collect();

        assert!(names.contains(&"top.txt".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("sub")), "{names:?}");
        assert!(!names.contains(&".secret".to_string()), "{names:?}");
    }

    #[test]
    fn escape_closes_the_finder_without_touching_the_pane() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());
        let before = app.panes.get_active_pane().path.clone();

        app.dispatch_key(&key(KeyCode::Char('/'), KeyModifiers::NONE));
        type_pattern(&mut app, "deep");
        app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.find_files().is_none());
        assert_eq!(app.panes.get_active_pane().path, before);
    }

    #[test]
    fn the_pane_filter_reads_a_word_fuzzily_and_a_pattern_as_a_regex() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        // One bar, no mode to choose: a plain word is fuzzy...
        app.dispatch_key(&key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "top");
        assert!(matches!(
            app.panes.get_active_pane().filter(),
            Some(FilterSpec::Fuzzy(_))
        ));

        app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));

        // ...and a pattern is a regex.
        app.dispatch_key(&key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "^top");
        assert!(matches!(
            app.panes.get_active_pane().filter(),
            Some(FilterSpec::Regex(_))
        ));

        // Enter keeps the filter in place; Esc is what clears it.
        app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.search().is_none());
        assert!(app.panes.get_active_pane().filter().is_some());
        app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.panes.get_active_pane().filter().is_none());
    }

    #[test]
    fn a_half_typed_regex_filter_is_flagged_but_keeps_filtering() {
        let dir = dir_with_contents();
        let mut app = test_app(dir.path());

        app.dispatch_key(&key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_pattern(&mut app, "(top");

        assert!(app.search().unwrap().regex_invalid);
        // Fuzzy fallback, so the listing keeps responding instead of freezing.
        assert!(matches!(
            app.panes.get_active_pane().filter(),
            Some(FilterSpec::Fuzzy(_))
        ));
    }

    /// `Enter` on a zip/tar/tar.gz switches the pane into a read-only virtual
    /// listing of its contents; navigation, selection and `Copy` (extraction)
    /// keep working, everything that writes is refused.
    mod archive_vfs {
        use super::*;

        fn zip_with(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
            let path = dir.join(name);
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (entry_name, content) in files {
                zip.start_file(*entry_name, options).unwrap();
                std::io::Write::write_all(&mut zip, content).unwrap();
            }
            zip.finish().unwrap();
            path
        }

        /// Two independent panes, so extraction has somewhere real to land.
        fn test_app_two_dirs(left: &Path, right: &Path) -> App {
            let config = Config {
                initial_directory_left: left.to_string_lossy().to_string(),
                initial_directory_right: right.to_string_lossy().to_string(),
                ..Default::default()
            };
            let theme = Theme::load_theme(None).expect("default theme in themes/");
            App::new(theme, config, &left.join("config.toml"), None)
        }

        fn enter_archive(app: &mut App, zip: &Path) {
            app.panes.get_active_pane_mut().select_by_path(zip);
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));
        }

        /// Drains the active transfer to completion, the same way the run
        /// loop does every frame, without needing a real terminal tick.
        fn settle_progress(app: &mut App) {
            for _ in 0..600 {
                app.pump_progress();
                if app.progress.is_none() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("the background transfer never finished");
        }

        #[test]
        fn enter_on_an_archive_lists_its_contents_instead_of_opening_an_editor() {
            let dir = tempfile::tempdir().unwrap();
            let zip = zip_with(dir.path(), "a.zip", &[("src/main.rs", b"fn main() {}")]);
            let mut app = test_app(dir.path());

            enter_archive(&mut app, &zip);

            assert!(app.panes.get_active_pane().is_archive());
            assert!(
                app.panes
                    .get_active_pane()
                    .display_path()
                    .ends_with("a.zip")
            );
            assert!(app.pending_editor_file.is_none());

            let pane = app.panes.get_active_pane_mut();
            pane.select_by_path(&PathBuf::from("src"));
            assert_eq!(
                pane.get_selected_entry().map(|e| e.name),
                Some("src".to_string())
            );
        }

        #[test]
        fn backspace_at_the_archive_root_exits_to_the_real_directory() {
            let dir = tempfile::tempdir().unwrap();
            let zip = zip_with(dir.path(), "a.zip", &[("top.txt", b"hi")]);
            let mut app = test_app(dir.path());
            enter_archive(&mut app, &zip);
            assert!(app.panes.get_active_pane().is_archive());

            app.dispatch_key(&key(KeyCode::Backspace, KeyModifiers::NONE));

            assert!(!app.panes.get_active_pane().is_archive());
            let pane = app.panes.get_active_pane_mut();
            pane.select_by_path(&dir.path().join("a.zip"));
            assert_eq!(
                pane.get_selected_entry().map(|e| e.name),
                Some("a.zip".to_string())
            );
        }

        #[test]
        fn write_actions_are_refused_inside_an_archive() {
            let dir = tempfile::tempdir().unwrap();
            let zip = zip_with(dir.path(), "a.zip", &[("top.txt", b"hi")]);
            let mut app = test_app(dir.path());
            enter_archive(&mut app, &zip);

            for (code, modifiers) in [
                (KeyCode::Char('a'), KeyModifiers::NONE), // Create
                (KeyCode::Char('r'), KeyModifiers::NONE), // Rename
                (KeyCode::Char('M'), KeyModifiers::NONE), // Move
                (KeyCode::Char('S'), KeyModifiers::NONE), // DirSizes
                (KeyCode::Char('C'), KeyModifiers::NONE), // Permissions
                (KeyCode::Char('L'), KeyModifiers::NONE), // CreateSymlink
            ] {
                app.dispatch_key(&key(code, modifiers));
                assert!(
                    app.overlay.is_none(),
                    "{code:?} must not open a dialog inside an archive"
                );
                let status = app.footer.status_text().unwrap_or_default();
                assert!(
                    status.contains("Read-only"),
                    "{code:?} should report read-only, got {status:?}"
                );
            }

            // `dd` (delete) is a chord; both keys must be swallowed.
            app.dispatch_key(&key(KeyCode::Char('d'), KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Char('d'), KeyModifiers::NONE));
            assert!(app.overlay.is_none());
        }

        #[test]
        fn copy_extracts_the_selected_entry_into_the_other_pane() {
            let left = tempfile::tempdir().unwrap();
            let right = tempfile::tempdir().unwrap();
            let zip = zip_with(
                left.path(),
                "a.zip",
                &[("src/main.rs", b"fn main() {}"), ("top.txt", b"hi")],
            );
            let mut app = test_app_two_dirs(left.path(), right.path());
            enter_archive(&mut app, &zip);
            app.panes
                .get_active_pane_mut()
                .select_by_path(&PathBuf::from("top.txt"));

            app.dispatch_key(&key(KeyCode::Char('Y'), KeyModifiers::NONE));
            settle_progress(&mut app);

            assert_eq!(
                std::fs::read_to_string(right.path().join("top.txt")).unwrap(),
                "hi"
            );
            let status = app.footer.status_text().unwrap_or_default().to_string();
            assert!(!status.to_lowercase().contains("fail"), "{status}");
        }

        #[test]
        fn move_is_refused_inside_an_archive_even_though_copy_extracts() {
            let left = tempfile::tempdir().unwrap();
            let right = tempfile::tempdir().unwrap();
            let zip = zip_with(left.path(), "a.zip", &[("top.txt", b"hi")]);
            let mut app = test_app_two_dirs(left.path(), right.path());
            enter_archive(&mut app, &zip);
            app.panes
                .get_active_pane_mut()
                .select_by_path(&PathBuf::from("top.txt"));

            app.dispatch_key(&key(KeyCode::Char('M'), KeyModifiers::NONE));

            assert!(!right.path().join("top.txt").exists());
            assert!(
                app.footer
                    .status_text()
                    .unwrap_or_default()
                    .contains("Read-only")
            );
        }

        #[test]
        fn pasting_into_an_archive_pane_is_refused() {
            let left = tempfile::tempdir().unwrap();
            let right = tempfile::tempdir().unwrap();
            std::fs::write(right.path().join("donor.txt"), "x").unwrap();
            let zip = zip_with(left.path(), "a.zip", &[("top.txt", b"hi")]);
            let mut app = test_app_two_dirs(left.path(), right.path());

            // Yank from the real (right) pane, switch to the archive (left)
            // pane, then try to paste into it.
            app.panes.set_active_pane(ActivePane::Right);
            app.panes
                .get_active_pane_mut()
                .select_by_path(&right.path().join("donor.txt"));
            app.dispatch_key(&key(KeyCode::Char('y'), KeyModifiers::NONE));
            app.panes.set_active_pane(ActivePane::Left);
            enter_archive(&mut app, &zip);

            app.dispatch_key(&key(KeyCode::Char('p'), KeyModifiers::NONE));

            assert!(
                app.footer
                    .status_text()
                    .unwrap_or_default()
                    .contains("Read-only")
            );
        }
    }

    /// `C` (chmod/chown) and `L` (create symlink).
    mod permissions_and_symlinks {
        use super::*;

        #[cfg(unix)]
        fn test_app_two_dirs(left: &Path, right: &Path) -> App {
            let config = Config {
                initial_directory_left: left.to_string_lossy().to_string(),
                initial_directory_right: right.to_string_lossy().to_string(),
                ..Default::default()
            };
            let theme = Theme::load_theme(None).expect("default theme in themes/");
            App::new(theme, config, &left.join("config.toml"), None)
        }

        #[cfg(unix)]
        #[test]
        fn c_opens_the_popup_seeded_from_the_highlighted_entry() {
            use std::os::unix::fs::PermissionsExt;

            let dir = dir_with_contents();
            std::fs::set_permissions(
                dir.path().join("top.txt"),
                std::fs::Permissions::from_mode(0o640),
            )
            .unwrap();
            let mut app = test_app(dir.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&dir.path().join("top.txt"));

            app.dispatch_key(&key(KeyCode::Char('C'), KeyModifiers::NONE));

            let pe = app.permissions_editor().expect("popup open");
            assert_eq!(pe.mode.value, "640");
            assert_eq!(pe.targets, vec![dir.path().join("top.txt")]);
        }

        #[cfg(unix)]
        #[test]
        fn toggling_a_bit_and_applying_changes_the_real_mode() {
            use std::os::unix::fs::PermissionsExt;

            let dir = dir_with_contents();
            std::fs::set_permissions(
                dir.path().join("top.txt"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let mut app = test_app(dir.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&dir.path().join("top.txt"));
            app.dispatch_key(&key(KeyCode::Char('C'), KeyModifiers::NONE));

            // Grid starts on (owner, r); move to (other, w) and toggle it on.
            app.dispatch_key(&key(KeyCode::Down, KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Down, KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Right, KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert!(app.overlay.is_none(), "the popup closes on a clean apply");
            let mode = std::fs::metadata(dir.path().join("top.txt"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o646);
            assert!(app.footer.status_text().unwrap_or_default().contains("1"));
        }

        #[cfg(unix)]
        #[test]
        fn typing_octal_digits_sets_a_whole_row_at_a_time() {
            let dir = dir_with_contents();
            let mut app = test_app(dir.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&dir.path().join("top.txt"));
            app.dispatch_key(&key(KeyCode::Char('C'), KeyModifiers::NONE));

            for c in ['7', '0', '0'] {
                app.dispatch_key(&key(KeyCode::Char(c), KeyModifiers::NONE));
            }

            assert_eq!(
                app.permissions_editor().unwrap().mode.value,
                "700".to_string()
            );
        }

        #[cfg(unix)]
        #[test]
        fn an_unresolvable_owner_blocks_apply_and_keeps_the_popup_open() {
            let dir = dir_with_contents();
            let mut app = test_app(dir.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&dir.path().join("top.txt"));
            app.dispatch_key(&key(KeyCode::Char('C'), KeyModifiers::NONE));

            // Tab to the owner field and replace it with a name that cannot
            // resolve to a uid.
            app.dispatch_key(&key(KeyCode::Tab, KeyModifiers::NONE));
            if let Some(pe) = app.permissions_editor_mut() {
                pe.owner = TextInput::new("definitely-not-a-real-user");
            }
            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert!(app.overlay.is_some(), "a bad name must not close the popup");
            assert!(
                app.permissions_editor().unwrap().error.is_some(),
                "the popup should carry the error for the user to see"
            );
        }

        #[cfg(unix)]
        #[test]
        fn esc_cancels_without_touching_the_file() {
            use std::os::unix::fs::PermissionsExt;

            let dir = dir_with_contents();
            std::fs::set_permissions(
                dir.path().join("top.txt"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let mut app = test_app(dir.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&dir.path().join("top.txt"));
            app.dispatch_key(&key(KeyCode::Char('C'), KeyModifiers::NONE));
            app.dispatch_key(&key(KeyCode::Char(' '), KeyModifiers::NONE)); // toggle owner-read off
            app.dispatch_key(&key(KeyCode::Esc, KeyModifiers::NONE));

            assert!(app.overlay.is_none());
            let mode = std::fs::metadata(dir.path().join("top.txt"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o644, "cancelling must not touch the file");
        }

        #[cfg(unix)]
        #[test]
        fn l_creates_a_symlink_in_the_other_pane_pointing_at_the_source() {
            let left = tempfile::tempdir().unwrap();
            let right = tempfile::tempdir().unwrap();
            std::fs::write(left.path().join("a.txt"), "hi").unwrap();
            let mut app = test_app_two_dirs(left.path(), right.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&left.path().join("a.txt"));

            app.dispatch_key(&key(KeyCode::Char('L'), KeyModifiers::NONE));

            let link = right.path().join("a.txt");
            assert_eq!(
                std::fs::read_link(&link).unwrap(),
                left.path().join("a.txt")
            );
            assert_eq!(std::fs::read_to_string(&link).unwrap(), "hi");
        }

        #[cfg(unix)]
        #[test]
        fn l_on_a_name_that_exists_asks_before_overwriting() {
            let left = tempfile::tempdir().unwrap();
            let right = tempfile::tempdir().unwrap();
            std::fs::write(left.path().join("a.txt"), "hi").unwrap();
            std::fs::write(right.path().join("a.txt"), "already here").unwrap();
            let mut app = test_app_two_dirs(left.path(), right.path());
            app.panes
                .get_active_pane_mut()
                .select_by_path(&left.path().join("a.txt"));

            app.dispatch_key(&key(KeyCode::Char('L'), KeyModifiers::NONE));

            assert_eq!(app.overlay_kind(), Some(OverlayKind::Dialog));
            assert_eq!(
                std::fs::read_to_string(right.path().join("a.txt")).unwrap(),
                "already here",
                "must not overwrite before confirmation"
            );

            app.dispatch_key(&key(KeyCode::Enter, KeyModifiers::NONE));

            assert!(
                std::fs::symlink_metadata(right.path().join("a.txt"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "confirming replaces the file with the link"
            );
        }
    }
}
