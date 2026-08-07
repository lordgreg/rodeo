//! The application: state, the event loop and the render tree.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    fs::ops::ProgressMsg,
    ui::{footer::Footer, theme::Theme},
};
use notify::Watcher as _;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Gauge},
};

pub mod command;
pub mod completion;
pub mod component;
pub mod dialog;
pub mod filepreview;
pub mod footer;
pub mod git;
pub mod header;
pub mod input;
pub mod keymap;
pub mod panes;
pub mod popup_bulkrename;
pub mod popup_findfiles;
pub mod popup_findinfiles;
pub mod popup_keybinds;
pub mod popup_preview;
pub mod popup_trash;
pub mod search;
pub mod syntax;
pub mod textinput;
pub mod theme;

use component::Component;
use dialog::Dialog;
use header::Header;
use panes::Panes;
use popup_bulkrename::BulkRename;
use popup_findfiles::FileFinder;
use popup_findinfiles::FindInFiles;
use popup_keybinds::PopupKeybinds;
use popup_preview::PopupPreview;
use popup_trash::TrashView;
use search::Search;
use textinput::TextInput;

/// A file waiting to be opened in `$EDITOR`, optionally at a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTarget {
    pub path: PathBuf,
    /// 1-based line, when the file was reached through something that knows one
    /// (a find-in-files hit). Only passed on to editors known to accept `+N`.
    pub line: Option<usize>,
}

impl EditorTarget {
    pub fn new(path: PathBuf) -> Self {
        Self { path, line: None }
    }

    pub fn at_line(path: PathBuf, line: usize) -> Self {
        Self {
            path,
            line: Some(line),
        }
    }

    /// Arguments for `editor`, jumping to the line where that is known to work.
    ///
    /// `+N` is a long-standing convention but not a universal one: helix and
    /// VS Code take a `file:line` argument instead and would treat `+N` as a
    /// file name to create. Only editors known to understand it get it; every
    /// other editor opens the file at the top, which is what happened before.
    fn args(&self, editor: &str) -> Vec<std::ffi::OsString> {
        let mut args = Vec::new();
        if let Some(line) = self.line.filter(|_| editor_takes_plus_line(editor)) {
            args.push(std::ffi::OsString::from(format!("+{line}")));
        }
        args.push(self.path.clone().into_os_string());
        args
    }
}

/// Whether `editor` opens `+N file` at line N.
///
/// Matched on the program name so a path or a wrapper (`/usr/bin/vim`) still
/// resolves; anything unknown is left alone.
fn editor_takes_plus_line(editor: &str) -> bool {
    let name = std::path::Path::new(editor.split_whitespace().next().unwrap_or(editor))
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    matches!(
        name,
        "vi" | "vim"
            | "nvim"
            | "view"
            | "gvim"
            | "nano"
            | "pico"
            | "micro"
            | "emacs"
            | "emacsclient"
            | "kak"
            | "joe"
            | "jed"
            | "ne"
            | "mcedit"
            | "gedit"
            | "kate"
    )
}

/// State of a running background file transfer (copy or move).
#[derive(Debug)]
pub struct Progress {
    pub title: String,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub rx: mpsc::Receiver<ProgressMsg>,
    pub cancel: Arc<AtomicBool>,
    pub is_cut: bool,
}

/// The modal layer drawn over the panes.
///
/// Exactly one can be open at a time, which is why this is an enum. It used to
/// be six `Option` fields plus two booleans in `UiConfig`, and "is something
/// modal open?" was answered by two hand-written lists — `dispatch_key`'s
/// guard chain and `has_overlay` — that had already drifted apart: one covered
/// the input bar but not transfers, the other the reverse.
#[derive(Debug)]
pub enum Overlay {
    Preview(PopupPreview),
    Dialog(Dialog),
    FindInFiles(FindInFiles),
    FindFiles(FileFinder),
    BulkRename(BulkRename),
    Trash(TrashView),
    Keybinds,
}

/// Which overlay is open, without borrowing it.
///
/// Lets `dispatch_key` pick a handler in one `match` and still hand `&mut self`
/// to it. Kept exhaustive by the compiler through [`Overlay::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Preview,
    Dialog,
    FindInFiles,
    FindFiles,
    BulkRename,
    Trash,
    Keybinds,
}

impl Overlay {
    pub fn kind(&self) -> OverlayKind {
        match self {
            Self::Preview(_) => OverlayKind::Preview,
            Self::Dialog(_) => OverlayKind::Dialog,
            Self::FindInFiles(_) => OverlayKind::FindInFiles,
            Self::FindFiles(_) => OverlayKind::FindFiles,
            Self::BulkRename(_) => OverlayKind::BulkRename,
            Self::Trash(_) => OverlayKind::Trash,
            Self::Keybinds => OverlayKind::Keybinds,
        }
    }
}

/// What the bar along the bottom is editing.
///
/// Deliberately *not* an [`Overlay`]: the panes stay visible and keep
/// responding to Up/Down underneath, and the bar can sit below an open popup —
/// `:` still opens the command line while the preview is up.
#[derive(Debug)]
pub enum InputMode {
    Filter(Search),
    Command(TextInput),
}

#[derive(Debug)]
/// The whole application: widgets, state and everything a frame needs.
pub struct App {
    exit: bool,
    theme: Theme,
    header: Header,
    footer: Footer,
    panes: Panes,
    config: Config,
    /// The modal layer, if any. See [`Overlay`].
    pub(crate) overlay: Option<Overlay>,
    /// What the bottom bar is editing, if anything. See [`InputMode`].
    pub(crate) input_mode: Option<InputMode>,
    /// Live completion for the command line, recomputed as it is edited.
    completion: completion::Completion,
    clipboard: Vec<PathBuf>,
    clipboard_cut: bool,
    pending_d: bool,
    pending_editor_file: Option<EditorTarget>,
    /// Command to run attached to the terminal (`:term`), handled by the run
    /// loop where the terminal can be suspended.
    pending_terminal_command: Option<String>,
    /// Set when something outside rodeo may have written to the screen, so the
    /// run loop repaints from scratch instead of diffing against a frame that
    /// is no longer what the terminal shows.
    pending_redraw: bool,
    progress: Option<Progress>,
    keymap: keymap::Keymap,
    /// Filesystem event receiver — events trigger a debounced pane reload.
    fs_notify_rx: mpsc::Receiver<notify::Result<notify::Event>>,
    /// The watcher must stay alive for the duration of the app. `None` when it
    /// could not be created (inotify limits, unsupported filesystem) — the app
    /// then simply does not auto-refresh.
    _fs_watcher: Option<notify::RecommendedWatcher>,
    /// Currently watched directories (left pane, right pane).
    watched_dirs: [PathBuf; 2],
    /// When the last filesystem event arrived; reload fires after 150 ms silence.
    fs_debounce: Option<Instant>,
    /// Syntax colours derived from the active theme. Built once here and
    /// shared with every preview (and its background loader) instead of being
    /// rebuilt per preview.
    syn_theme: Arc<syntect::highlighting::Theme>,
}

impl App {
    pub fn new(theme: Theme, config: Config) -> Self {
        let panes = Panes::new(&config);
        let syn_theme = Arc::new(theme.to_syntect_theme());
        let current_directory = config.get_initial_dir();
        let header = Header::new(current_directory);
        let keymap = keymap::build_keymap(&config);

        // Set up filesystem watcher. Errors are non-fatal: auto-refresh just
        // won't work, but everything else continues normally.
        let (fs_tx, fs_notify_rx) = mpsc::channel();
        let watched_dirs = panes.pane_dirs();
        let mut _fs_watcher = notify::RecommendedWatcher::new(fs_tx, notify::Config::default())
            .map_err(|e| log::warn!("Cannot start filesystem watcher: {e}"))
            .ok();
        if let Some(watcher) = _fs_watcher.as_mut() {
            for dir in &watched_dirs {
                if let Err(e) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
                    log::warn!("Cannot watch {dir:?}: {e}");
                }
            }
        }

        let mut app = Self {
            exit: false,
            theme,
            header,
            footer: Footer::default(),
            panes,
            keymap,
            config,
            overlay: None,
            input_mode: None,
            completion: completion::Completion::default(),
            clipboard: Vec::new(),
            clipboard_cut: false,
            pending_d: false,
            pending_editor_file: None,
            pending_terminal_command: None,
            pending_redraw: false,
            progress: None,
            fs_notify_rx,
            _fs_watcher,
            watched_dirs,
            fs_debounce: None,
            syn_theme,
        };

        app.footer.update_hints(&app.keymap);
        // Points the header at the starting directory. The branch and counts
        // are still being fetched on a worker thread and land a frame or two
        // later, through `poll_git` in the event loop.
        app.sync_header();
        app.report_keymap_warnings();
        app
    }

    /// Puts keybinding problems in front of the user instead of only in the
    /// log: a typo that silently drops a feature is otherwise found weeks
    /// later, by pressing a key that does nothing.
    pub(crate) fn report_keymap_warnings(&mut self) {
        if self.keymap.warnings.is_empty() {
            return;
        }

        for warning in &self.keymap.warnings {
            log::warn!("keybindings: {warning}");
        }

        let detail = self
            .keymap
            .warnings
            .iter()
            .map(|w| format!("• {w}"))
            .collect::<Vec<_>>()
            .join("\n");

        self.overlay = Some(Overlay::Dialog(Dialog::message(
            "Keybindings",
            format!("{detail}\n\nEdit [keybindings] in config.toml, then :so to reload."),
        )));
    }

    /// The event loop.
    ///
    /// Note what is *not* here: terminal setup and teardown. `ratatui::run` in
    /// `main` owns those and hands this an already-configured terminal, so
    /// there is no `init`/`shutdown` pair to split out. What is left is one
    /// turn of rodeo's life, and each turn is these five steps — which used to
    /// be a hundred lines of inline detail, and are now readable in one
    /// screen.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            self.draw_frame(terminal)?;
            self.absorb_filesystem_events();
            self.wait_for_input()?;
            // Navigation may have changed which directories matter.
            self.refresh_fs_watches();
            self.run_pending_editor(terminal)?;
            self.run_pending_shell_command(terminal)?;
        }
        Ok(())
    }

    /// Settles all state the frame depends on, then paints exactly one frame.
    fn draw_frame(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        // Everything the frame reads is settled before the frame starts.
        self.prepare_frame();
        // A background `git status` may have finished since the last frame;
        // fold it in before painting rather than after.
        if self.panes.poll_git() {
            self.sync_header();
        }
        terminal.draw(|frame| self.render(frame))?;
        Ok(())
    }

    /// Takes in what changed on disk while rodeo was busy elsewhere.
    ///
    /// Events are debounced rather than acted on directly: a single `cargo
    /// build` produces thousands, and reloading on each would be a reload per
    /// file written.
    fn absorb_filesystem_events(&mut self) {
        // Access (read/open) events are ignored: rodeo itself reads files
        // while building a preview, and reloading on those would fight the
        // cursor.
        while let Ok(Ok(event)) = self.fs_notify_rx.try_recv() {
            if event.kind.is_access() {
                continue;
            }
            self.fs_debounce = Some(Instant::now());
        }

        // After 150 ms of filesystem silence, reload both panes.
        if self
            .fs_debounce
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(150))
        {
            self.fs_debounce = None;
            // Keep flagged entries: an external change must not wipe the
            // user's selection.
            self.panes.reload(&self.config, false);
            self.sync_header();
            self.refresh_fs_watches();
        }
    }

    /// Waits for the user, blocking only when nothing else needs attention.
    ///
    /// Anything with a deadline — a running transfer, a loading preview, a
    /// pending debounce, a background `git status` — forces a short poll
    /// instead, so those finish on their own rather than on the next keypress.
    fn wait_for_input(&mut self) -> std::io::Result<()> {
        let needs_tick = self.progress.is_some()
            || self.preview().is_some_and(|p| p.is_loading())
            || self.fs_debounce.is_some()
            || self.panes.git_pending();

        if !needs_tick {
            return self.handle_input();
        }

        if crossterm::event::poll(Duration::from_millis(50))? {
            self.handle_input()?;
        }
        self.pump_progress();
        Ok(())
    }

    /// Hands the terminal to `$EDITOR` if something asked for it, and reports
    /// whether the file came back changed.
    fn run_pending_editor(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        let Some(target) = self.pending_editor_file.take() else {
            return Ok(());
        };

        let path = target.path.clone();
        let mtime_before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        let editor = self.config.editor.clone();
        let args = target.args(&editor);
        suspended(terminal, || {
            let _ = Command::new(&editor).args(&args).status();
        })?;
        self.after_external_program();

        let mtime_after = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime_after != mtime_before {
            self.ok_status(format!("Modified: {}", path.display()));
        }
        Ok(())
    }

    /// Runs a `:term` command with the screen to itself, and reports how it
    /// went.
    ///
    /// The repaint comes first: a *captured* command (`:!`) cannot be trusted
    /// to have left the screen alone, because programs that want a terminal
    /// open `/dev/tty` and draw on it regardless of the pipes they were given,
    /// and rodeo's diffing renderer would leave whatever they drew on screen.
    fn run_pending_shell_command(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        if std::mem::take(&mut self.pending_redraw) {
            restore_terminal_state(terminal);
        }

        let Some(command) = self.pending_terminal_command.take() else {
            return Ok(());
        };

        let status = suspended(terminal, || {
            let status = Command::new("sh").args(["-c", &command]).status();

            // The child owned the screen; pause so its output can be read
            // before rodeo paints over it again.
            match &status {
                Ok(status) if status.success() => print!("\n[:term finished] "),
                Ok(status) => print!("\n[:term exited {}] ", exit_label(status)),
                Err(e) => print!("\n[:term failed: {e}] "),
            }
            print!("press Enter to return to rodeo");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let _ = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut String::new());

            status
        })?;
        self.after_external_program();

        match status {
            Ok(status) if !status.success() => {
                self.err_status(format!(":term — exit {}", exit_label(&status)));
            }
            Err(e) => self.err_status(format!("Cannot run: {e}")),
            _ => {}
        }
        Ok(())
    }

    /// Drains progress messages from a background transfer; finishes it when
    /// the worker reports Done.
    /// Updates the filesystem watcher to track the two currently-displayed
    /// directories. Called after navigation so new directories are watched
    /// automatically.
    fn refresh_fs_watches(&mut self) {
        let new_dirs = self.panes.pane_dirs();
        if new_dirs == self.watched_dirs {
            return;
        }
        let Some(watcher) = self._fs_watcher.as_mut() else {
            self.watched_dirs = new_dirs;
            return;
        };

        // Unwatch old directories.
        for dir in &self.watched_dirs {
            let _ = watcher.unwatch(dir);
        }
        // Watch new directories.
        for dir in &new_dirs {
            if let Err(e) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
                log::warn!("Cannot watch {dir:?}: {e}");
            }
        }
        self.watched_dirs = new_dirs;
    }

    fn pump_progress(&mut self) {
        let Some(p) = self.progress.as_mut() else {
            return;
        };

        let mut finished = None;
        while let Ok(msg) = p.rx.try_recv() {
            match msg {
                ProgressMsg::Advance(n) => p.done_bytes += n,
                ProgressMsg::Done(result) => {
                    finished = Some(result);
                    break;
                }
            }
        }

        let Some(result) = finished else {
            return;
        };

        let is_cut = p.is_cut;
        self.progress = None;
        self.panes.reload(&self.config, false);
        if is_cut {
            self.clipboard.clear();
            self.clipboard_cut = false;
        }
        if let Err(e) = result {
            self.err_status(format!("Transfer failed: {e}"));
        } else {
            self.ok_status("Transfer complete".to_string());
        }
    }

    /// Reloads the panes after an external program had the terminal: it may
    /// have changed anything on disk.
    fn after_external_program(&mut self) {
        self.panes.reload(&self.config, false);
        self.sync_header();
    }

    /// Points the header at the active pane. The git summary comes from the
    /// listing's own `git status` run, so this costs no subprocesses.
    pub(crate) fn sync_header(&mut self) {
        let pane = self.panes.get_active_pane();
        let path = pane.path.to_string();
        let git = pane.git_summary().cloned();
        self.header.update(path, git);
    }

    /// `true` while something modal covers the panes.
    ///
    /// A transfer counts: it blocks further operations until it finishes.
    fn has_overlay(&self) -> bool {
        self.overlay.is_some() || self.progress.is_some()
    }

    /// Brings every piece of state the frame will read up to date, before the
    /// frame starts.
    ///
    /// The draw is meant to be data-in, paint-out. Three overlays used to
    /// break that: the preview popup decided a file's type and spawned its
    /// loader from inside `render`, and both search popups read the selected
    /// file from disk to build their preview pane — a `terminal.draw` closure
    /// that opened files and rewrote widget state. All of that happens here
    /// now.
    ///
    /// Public because it is half of the frame contract: every caller that
    /// draws — the run loop, and the render tests — must call this first.
    pub fn prepare_frame(&mut self) {
        self.sync_preview_to_selection();

        match &mut self.overlay {
            Some(Overlay::Preview(preview)) => preview.prepare(),
            Some(Overlay::FindFiles(finder)) => finder.prepare(),
            Some(Overlay::FindInFiles(finder)) => finder.prepare(),
            _ => {}
        }
    }

    /// Keeps an entry-bound preview pointed at the highlighted entry.
    ///
    /// Free-text previews (`:!` output) are not bound to an entry and stay as
    /// they are until closed. This used to live inside `render`, where a draw
    /// could re-read a file from disk and swap out a widget.
    pub(crate) fn sync_preview_to_selection(&mut self) {
        let Some(Overlay::Preview(preview)) = &self.overlay else {
            return;
        };
        let Some(shown) = preview.selected() else {
            return;
        };

        // Compare by path only: a reload rebuilds Entry values (sizes, flags)
        // and a full equality check would rebuild — and re-read — the preview
        // on every frame.
        let shown_path = shown.path.clone();
        let current = self.panes.get_active_pane().get_selected_entry();
        if current.as_ref().map(|e| &e.path) == Some(&shown_path) {
            return;
        }

        // Nothing highlighted (the last entry was just deleted) closes the
        // preview rather than leaving a dimmed, empty screen.
        self.overlay =
            current.map(|e| Overlay::Preview(PopupPreview::new(Some(e), self.syn_theme.clone())));
    }

    /// Which overlay is open, if any.
    pub(crate) fn overlay_kind(&self) -> Option<OverlayKind> {
        self.overlay.as_ref().map(Overlay::kind)
    }

    pub(crate) fn preview(&self) -> Option<&PopupPreview> {
        match &self.overlay {
            Some(Overlay::Preview(p)) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn preview_mut(&mut self) -> Option<&mut PopupPreview> {
        match &mut self.overlay {
            Some(Overlay::Preview(p)) => Some(p),
            _ => None,
        }
    }

    /// Only the tests inspect this one; production code matches on
    /// `self.overlay` directly.
    #[cfg(test)]
    pub(crate) fn dialog(&self) -> Option<&Dialog> {
        match &self.overlay {
            Some(Overlay::Dialog(d)) => Some(d),
            _ => None,
        }
    }

    pub(crate) fn find_in_files(&self) -> Option<&FindInFiles> {
        match &self.overlay {
            Some(Overlay::FindInFiles(f)) => Some(f),
            _ => None,
        }
    }

    pub(crate) fn find_in_files_mut(&mut self) -> Option<&mut FindInFiles> {
        match &mut self.overlay {
            Some(Overlay::FindInFiles(f)) => Some(f),
            _ => None,
        }
    }

    /// Only the tests inspect this one; production code matches on
    /// `self.overlay` directly.
    #[cfg(test)]
    pub(crate) fn find_files(&self) -> Option<&FileFinder> {
        match &self.overlay {
            Some(Overlay::FindFiles(f)) => Some(f),
            _ => None,
        }
    }

    pub(crate) fn find_files_mut(&mut self) -> Option<&mut FileFinder> {
        match &mut self.overlay {
            Some(Overlay::FindFiles(f)) => Some(f),
            _ => None,
        }
    }

    pub(crate) fn bulk_rename(&self) -> Option<&BulkRename> {
        match &self.overlay {
            Some(Overlay::BulkRename(b)) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn bulk_rename_mut(&mut self) -> Option<&mut BulkRename> {
        match &mut self.overlay {
            Some(Overlay::BulkRename(b)) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn trash_view(&self) -> Option<&TrashView> {
        match &self.overlay {
            Some(Overlay::Trash(t)) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn trash_view_mut(&mut self) -> Option<&mut TrashView> {
        match &mut self.overlay {
            Some(Overlay::Trash(t)) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn keybinds_open(&self) -> bool {
        self.overlay_kind() == Some(OverlayKind::Keybinds)
    }

    pub(crate) fn preview_open(&self) -> bool {
        self.overlay_kind() == Some(OverlayKind::Preview)
    }

    pub(crate) fn search(&self) -> Option<&Search> {
        match &self.input_mode {
            Some(InputMode::Filter(s)) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn search_mut(&mut self) -> Option<&mut Search> {
        match &mut self.input_mode {
            Some(InputMode::Filter(s)) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn command(&self) -> Option<&TextInput> {
        match &self.input_mode {
            Some(InputMode::Command(c)) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn command_mut(&mut self) -> Option<&mut TextInput> {
        match &mut self.input_mode {
            Some(InputMode::Command(c)) => Some(c),
            _ => None,
        }
    }

    /// Draws one full frame. Public to the crate so integration tests can
    /// render into a `TestBackend` without a real terminal.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let background = Block::default().style(Style::new().bg(self.theme.colors.background()));

        frame.render_widget(background, frame.area());

        // The input bar is visible while editing a filter/command or while a
        // filter is active on the active pane.
        let filter_active = self.panes.get_active_pane().filter().is_some();
        let show_input_bar = self.input_mode.is_some() || filter_active;

        let outer_layout = if show_input_bar {
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
        self.footer.set_clipboard(if self.clipboard.is_empty() {
            None
        } else {
            Some((self.clipboard.len(), self.clipboard_cut))
        });
        self.header.render(frame, &self.theme, outer_layout[0]);
        self.panes.render(frame, &self.theme, outer_layout[1]);
        let footer_idx = outer_layout.len() - 1;
        self.footer
            .render(frame, &self.theme, outer_layout[footer_idx]);

        if show_input_bar {
            self.render_input_bar(frame, outer_layout[2]);
            // The menu floats above the command line, like an editor's
            // completion popup.
            self.render_completion_menu(frame, outer_layout[2]);
        }

        // Anything modal reads as a focused layer only if what is behind it
        // recedes. Ratatui has no transparency, so dim the cells instead.
        if self.has_overlay() {
            dim_area(frame, frame.area());
        }

        // Only one overlay can be open, so there is no layering to get right.
        let area = frame.area();
        // The search popups are centred at 80% of the screen; the rest size
        // themselves and treat `area` as the whole screen.
        let search_popup_area = ratatui::layout::Rect {
            x: area.width / 10,
            y: area.height / 10,
            width: area.width * 4 / 5,
            height: area.height * 4 / 5,
        };

        match &mut self.overlay {
            Some(Overlay::Keybinds) => PopupKeybinds::new().render(frame, &self.theme, area),
            Some(Overlay::Preview(preview)) => preview.render(frame, &self.theme, area),
            Some(Overlay::BulkRename(br)) => br.render(frame, &self.theme, area),
            Some(Overlay::Trash(tv)) => tv.render(frame, &self.theme, area),
            Some(Overlay::FindInFiles(find)) => {
                frame.render_widget(Clear, search_popup_area);
                find.render(frame, &self.theme, search_popup_area);
            }
            Some(Overlay::FindFiles(finder)) => {
                frame.render_widget(Clear, search_popup_area);
                finder.render(frame, &self.theme, search_popup_area);
            }
            Some(Overlay::Dialog(dialog)) => dialog.render(frame, &self.theme, area),
            None => {}
        }

        // The progress gauge renders above everything (a transfer blocks
        // further operations until it finishes or is cancelled).
        if let Some(p) = &self.progress {
            let area = ratatui::layout::Rect {
                x: frame.area().x + frame.area().width / 4,
                y: frame.area().y + frame.area().height / 2 - 1,
                width: frame.area().width / 2,
                height: 3,
            };
            let ratio = if p.total_bytes == 0 {
                1.0
            } else {
                (p.done_bytes as f64 / p.total_bytes as f64).min(1.0)
            };
            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .title(p.title.as_str())
                        .borders(Borders::ALL),
                )
                .gauge_style(Style::default().fg(self.theme.colors.info()))
                .ratio(ratio)
                .label(format!("{:.0}% — Esc to cancel", ratio * 100.0));
            frame.render_widget(Clear, area);
            frame.render_widget(gauge, area);
        }
    }

    /// Draws the command-line completion menu just above `input_area`.
    ///
    /// Candidates are listed with their argument placeholder and description,
    /// the highlighted one inverted, and the list scrolls to keep the
    /// selection visible when there are more candidates than rows.
    fn render_completion_menu(&mut self, frame: &mut Frame<'_>, input_area: Rect) {
        use ratatui::{
            text::{Line, Span},
            widgets::Paragraph,
        };

        if self.command().is_none() || !self.completion.is_active() {
            return;
        }

        let candidates = self.completion.candidates();
        let name_width = candidates
            .iter()
            .map(|c| c.value.chars().count() + c.args.chars().count() + 1)
            .max()
            .unwrap_or(0);
        let description_width = candidates
            .iter()
            .map(|c| c.description.chars().count())
            .max()
            .unwrap_or(0);

        let width = (name_width + description_width + 4).min(MAX_COMPLETION_WIDTH) as u16;
        let width = width.min(input_area.width);

        // Never grow past the panes, and never past the available room above
        // the command line.
        let rows = candidates
            .len()
            .min(MAX_COMPLETION_ROWS)
            .min(input_area.y.saturating_sub(1) as usize) as u16;
        if rows == 0 || width == 0 {
            return;
        }

        // Keep the selection inside the visible window.
        let selected = self.completion.selected();
        let first = match selected {
            Some(index) if index >= rows as usize => index + 1 - rows as usize,
            _ => 0,
        };

        let area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(rows),
            width,
            height: rows,
        };

        let lines: Vec<Line> = candidates
            .iter()
            .enumerate()
            .skip(first)
            .take(rows as usize)
            .map(|(index, candidate)| {
                let is_selected = selected == Some(index);
                let (name_style, description_style) = if is_selected {
                    (
                        Style::new()
                            .fg(self.theme.colors.background())
                            .bg(self.theme.colors.primary()),
                        Style::new()
                            .fg(self.theme.colors.background())
                            .bg(self.theme.colors.primary()),
                    )
                } else {
                    (
                        Style::new().fg(self.theme.colors.foreground()),
                        Style::new().fg(self.theme.colors.muted()),
                    )
                };

                let mut name = candidate.value.clone();
                if !candidate.args.is_empty() {
                    name.push(' ');
                    name.push_str(&candidate.args);
                }

                Line::from(vec![
                    Span::styled(format!(" {name:<name_width$} "), name_style),
                    Span::styled(candidate.description.clone(), description_style),
                ])
            })
            .collect();

        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).style(Style::new().bg(self.theme.colors.surface())),
            area,
        );
    }

    fn render_input_bar(&mut self, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
        use ratatui::widgets::Paragraph;

        let (text, style, cursor_offset): (String, Style, Option<u16>) =
            if let Some(input) = self.command() {
                (
                    format!(":{}", input.value),
                    Style::new().fg(self.theme.colors.foreground()),
                    Some(1 + input.cursor as u16),
                )
            } else {
                match self.search() {
                    // One filter bar for both kinds of query: the prefix says
                    // how what has been typed is being read.
                    Some(s) => {
                        let style = if s.regex_invalid {
                            Style::new().fg(self.theme.colors.error())
                        } else {
                            Style::new().fg(self.theme.colors.foreground())
                        };
                        const PREFIX: &str = "filter: ";
                        (
                            format!("{PREFIX}{}", s.input.value),
                            style,
                            Some(PREFIX.len() as u16 + s.input.cursor as u16),
                        )
                    }
                    None => match self.panes.get_active_pane().filter() {
                        Some(filter) => (
                            format!(
                                "{} filter: {}  (Ctrl+f to edit, Esc to clear)",
                                filter.kind_label(),
                                filter.pattern()
                            ),
                            Style::new().fg(self.theme.colors.muted()),
                            None,
                        ),
                        None => (String::new(), Style::new(), None),
                    },
                }
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

/// Re-asserts the modes rodeo needs and forces a full repaint.
///
/// Failures are logged rather than propagated: this is cosmetic recovery, and
/// terminals that do not answer the cursor-position query
/// ([`ratatui::Terminal::clear`] asks) must not take the file manager down.
fn restore_terminal_state(terminal: &mut DefaultTerminal) {
    use crossterm::terminal::enable_raw_mode;

    if let Err(e) = enable_raw_mode() {
        log::warn!("cannot re-enable raw mode: {e}");
    }
    let _ = terminal.hide_cursor();
    if let Err(e) = terminal.clear() {
        log::warn!("cannot repaint the screen: {e}");
    }
}

/// Runs `f` with the terminal handed back to the shell: raw mode off and the
/// alternate screen left, so the child gets a clean, normal screen and its
/// scrollback survives. rodeo's own screen is restored afterwards.
fn suspended<T>(terminal: &mut DefaultTerminal, f: impl FnOnce() -> T) -> std::io::Result<T> {
    use crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode},
    };

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;

    let result = f();

    execute!(std::io::stdout(), EnterAlternateScreen)?;
    restore_terminal_state(terminal);

    Ok(result)
}

/// Exit status as a short label: a code, or the signal that killed it.
fn exit_label(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        #[cfg(unix)]
        None => {
            use std::os::unix::process::ExitStatusExt;
            match status.signal() {
                Some(signal) => format!("signal {signal}"),
                None => "unknown".to_string(),
            }
        }
        #[cfg(not(unix))]
        None => "unknown".to_string(),
    }
}

/// Widest the completion menu may get.
const MAX_COMPLETION_WIDTH: usize = 64;
/// Most candidates shown at once; the list scrolls beyond that.
const MAX_COMPLETION_ROWS: usize = 8;

/// Fades everything already drawn in `area` so a modal layer on top of it
/// stands out. Ratatui cannot draw translucent widgets, so this walks the
/// buffer and adds the terminal's DIM attribute to every cell.
fn dim_area(frame: &mut Frame<'_>, area: Rect) {
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.modifier.insert(Modifier::DIM);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_for(editor: &str, line: Option<usize>) -> Vec<String> {
        let target = EditorTarget {
            path: PathBuf::from("/tmp/a.rs"),
            line,
        };
        target
            .args(editor)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_known_editor_is_told_the_line() {
        assert_eq!(args_for("vim", Some(12)), vec!["+12", "/tmp/a.rs"]);
        assert_eq!(args_for("/usr/bin/nvim", Some(3)), vec!["+3", "/tmp/a.rs"]);
        assert_eq!(args_for("micro", Some(1)), vec!["+1", "/tmp/a.rs"]);
    }

    #[test]
    fn an_unknown_editor_only_gets_the_path() {
        // `+12` would be taken for a file name by these, creating one.
        assert_eq!(args_for("hx", Some(12)), vec!["/tmp/a.rs"]);
        assert_eq!(args_for("code", Some(12)), vec!["/tmp/a.rs"]);
        assert_eq!(args_for("subl", Some(12)), vec!["/tmp/a.rs"]);
    }

    #[test]
    fn without_a_line_nothing_changes() {
        assert_eq!(args_for("vim", None), vec!["/tmp/a.rs"]);
    }
}
