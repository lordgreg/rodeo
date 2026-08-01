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
pub mod footer;
pub mod git;
pub mod header;
pub mod input;
pub mod keymap;
pub mod panes;
pub mod popup_about;
pub mod popup_bulkrename;
pub mod popup_findinfiles;
pub mod popup_keybinds;
pub mod popup_preview;
pub mod popup_trash;
pub mod search;
pub mod textinput;
pub mod theme;
pub mod uiconfig;

use component::Component;
use crossterm::event::KeyCode;
use dialog::Dialog;
use header::Header;
use keymap::Action;
use panes::Panes;
use popup_about::PopupAbout;
use popup_bulkrename::BulkRename;
use popup_findinfiles::FindInFiles;
use popup_keybinds::PopupKeybinds;
use popup_preview::PopupPreview;
use popup_trash::TrashView;
use search::{FilterSpec, Search, SearchKind};
use textinput::TextInput;
use uiconfig::UiConfig;

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

#[derive(Debug)]
/// The whole application: widgets, state and everything a frame needs.
pub struct App {
    exit: bool,
    theme: Theme,
    ui_config: UiConfig,
    header: Header,
    footer: Footer,
    panes: Panes,
    config: Config,
    preview: Option<PopupPreview>,
    dialog: Option<Dialog>,
    search: Option<Search>,
    command: Option<TextInput>,
    /// Live completion for the command line, recomputed as it is edited.
    completion: completion::Completion,
    find_in_files: Option<FindInFiles>,
    bulk_rename: Option<BulkRename>,
    trash_view: Option<TrashView>,
    clipboard: Vec<PathBuf>,
    clipboard_cut: bool,
    pending_d: bool,
    pending_editor_file: Option<PathBuf>,
    /// Command to run attached to the terminal (`:term`), handled by the run
    /// loop where the terminal can be suspended.
    pending_terminal_command: Option<String>,
    /// Set when something outside rodeo may have written to the screen, so the
    /// run loop repaints from scratch instead of diffing against a frame that
    /// is no longer what the terminal shows.
    pending_redraw: bool,
    progress: Option<Progress>,
    keymap: Vec<(KeyCode, Action)>,
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

        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header,
            footer: Footer::default(),
            panes,
            keymap,
            config,
            preview: None,
            dialog: None,
            search: None,
            command: None,
            completion: completion::Completion::default(),
            find_in_files: None,
            bulk_rename: None,
            trash_view: None,
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
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;

            // Drain all pending filesystem events; arm the debounce timer.
            // Access (read/open) events are ignored: rodeo itself reads files
            // while building a preview, and reloading on those would fight the
            // cursor.
            while let Ok(Ok(event)) = self.fs_notify_rx.try_recv() {
                if event.kind.is_access() {
                    continue;
                }
                self.fs_debounce = Some(Instant::now());
            }

            // After 150 ms of FS silence, reload both panes and re-sync watches.
            if self
                .fs_debounce
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(150))
            {
                self.fs_debounce = None;
                // Keep flagged entries: an external change must not wipe the
                // user's selection.
                self.panes.reload(&self.config, false);
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
                self.refresh_fs_watches();
            }

            let preview_loading = self.preview.as_ref().is_some_and(|p| p.is_loading());
            let needs_tick =
                self.progress.is_some() || preview_loading || self.fs_debounce.is_some();

            if needs_tick {
                // While a transfer runs, a preview is loading, or a debounce
                // is pending, poll with a short timeout so animation stays
                // smooth and Esc remains responsive.
                if crossterm::event::poll(Duration::from_millis(50))? {
                    self.handle_input()?;
                }
                self.pump_progress();
            } else {
                self.handle_input()?;
            }

            // After each input event, sync watched directories to current panes.
            self.refresh_fs_watches();

            if let Some(path) = self.pending_editor_file.take() {
                let mtime_before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

                let editor = self.config.editor.clone();
                suspended(terminal, || {
                    let _ = Command::new(&editor).arg(&path).status();
                })?;
                self.after_external_program();

                let mtime_after = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                if mtime_after != mtime_before {
                    self.ok_status(format!("Modified: {}", path.display()));
                }
            }

            // A captured command cannot be trusted to have left the screen
            // alone: programs that want a terminal open /dev/tty and draw on it
            // regardless of the pipes they were given, and rodeo's diffing
            // renderer would leave whatever they drew on screen.
            if std::mem::take(&mut self.pending_redraw) {
                restore_terminal_state(terminal);
            }

            if let Some(command) = self.pending_terminal_command.take() {
                let status = suspended(terminal, || {
                    let status = Command::new("sh").args(["-c", &command]).status();

                    // The child owned the screen; pause so its output can be
                    // read before rodeo paints over it again.
                    match &status {
                        Ok(status) if status.success() => print!("\n[:term finished] "),
                        Ok(status) => print!("\n[:term exited {}] ", exit_label(status)),
                        Err(e) => print!("\n[:term failed: {e}] "),
                    }
                    print!("press Enter to return to rodeo");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    let _ = std::io::BufRead::read_line(
                        &mut std::io::stdin().lock(),
                        &mut String::new(),
                    );

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
            }
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
        self.header
            .update(self.panes.get_active_pane().path.to_string());
    }

    /// `true` while something modal covers the panes.
    fn has_overlay(&self) -> bool {
        self.ui_config.active_keybind_popup
            || self.ui_config.active_about_popup
            || self.ui_config.active_preview_popup
            || self.bulk_rename.is_some()
            || self.trash_view.is_some()
            || self.find_in_files.is_some()
            || self.dialog.is_some()
            || self.progress.is_some()
    }

    /// Draws one full frame. Public to the crate so integration tests can
    /// render into a `TestBackend` without a real terminal.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let background = Block::default().style(Style::new().bg(self.theme.colors.background()));

        frame.render_widget(background, frame.area());

        // The input bar is visible while editing a search/command or while a
        // regex filter is active on the active pane.
        let regex_filter_active = matches!(
            self.panes.get_active_pane().filter(),
            Some(FilterSpec::Regex(_))
        );
        let show_input_bar = self.search.is_some() || self.command.is_some() || regex_filter_active;

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
        self.header
            .render(frame, &self.theme, &self.ui_config, outer_layout[0]);
        self.panes
            .render(frame, &self.theme, &self.ui_config, outer_layout[1]);
        let footer_idx = outer_layout.len() - 1;
        self.footer.render(
            frame,
            &self.theme,
            &self.ui_config,
            outer_layout[footer_idx],
        );

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

        if self.ui_config.active_keybind_popup {
            PopupKeybinds::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_about_popup {
            PopupAbout::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_preview_popup {
            // Entry-bound previews follow the selection; free text previews
            // (e.g., `:!` output) stay until closed.
            if let Some(shown) = self.preview.as_ref().and_then(|p| p.selected()) {
                // Compare by path only: a reload rebuilds Entry values (sizes,
                // flags) and a full equality check would rebuild — and re-read
                // — the preview on every frame.
                let shown_path = shown.path.clone();
                let current = self.panes.get_active_pane().get_selected_entry();
                if current.as_ref().map(|e| &e.path) != Some(&shown_path) {
                    self.preview =
                        current.map(|e| PopupPreview::new(Some(e), self.syn_theme.clone()));
                }
            }
            if let Some(preview) = self.preview.as_mut() {
                preview.render(frame, &self.theme, &self.ui_config, frame.area());
            }
        }

        // Bulk rename popup renders on top of preview.
        if let Some(br) = self.bulk_rename.as_mut() {
            br.render(frame, &self.theme, &self.ui_config, frame.area());
        }

        // Trash view renders on top of everything except dialogs.
        if let Some(tv) = self.trash_view.as_mut() {
            tv.render(frame, &self.theme, &self.ui_config, frame.area());
        }

        // Find-in-files popup renders on top of preview.
        if let Some(find) = self.find_in_files.as_mut() {
            // Centered popup, 80% width and height
            let area = ratatui::layout::Rect {
                x: frame.area().width / 10,
                y: frame.area().height / 10,
                width: frame.area().width * 4 / 5,
                height: frame.area().height * 4 / 5,
            };
            frame.render_widget(Clear, area);
            find.render(frame, &self.theme, &self.ui_config, area);
        }

        // Dialogs render on top of everything.
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.render(frame, &self.theme, &self.ui_config, frame.area());
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

        if self.command.is_none() || !self.completion.is_active() {
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
            if let Some(input) = &self.command {
                (
                    format!(":{}", input.value),
                    Style::new().fg(self.theme.colors.foreground()),
                    Some(1 + input.cursor as u16),
                )
            } else {
                match &self.search {
                    Some(s) => match s.kind {
                        SearchKind::Fuzzy => (
                            format!("/{}", s.input.value),
                            Style::new().fg(self.theme.colors.foreground()),
                            Some(1 + s.input.cursor as u16),
                        ),
                        SearchKind::Regex => {
                            let style = if s.regex_invalid {
                                Style::new().fg(self.theme.colors.error())
                            } else {
                                Style::new().fg(self.theme.colors.foreground())
                            };
                            (
                                format!("regex: {}", s.input.value),
                                style,
                                Some(7 + s.input.cursor as u16),
                            )
                        }
                    },
                    None => match self.panes.get_active_pane().filter() {
                        Some(FilterSpec::Regex(pattern)) => (
                            format!("regex filter: {pattern}  (Ctrl+f to edit, Esc to clear)"),
                            Style::new().fg(self.theme.colors.muted()),
                            None,
                        ),
                        _ => (String::new(), Style::new(), None),
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
