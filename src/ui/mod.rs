use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

use crate::{
    config::Config,
    fs::ops::ProgressMsg,
    ui::{footer::Footer, theme::Theme},
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::{Block, Borders, Clear, Gauge},
};

pub mod component;
pub mod dialog;
pub mod footer;
pub mod git;
pub mod header;
pub mod input;
pub mod panes;
pub mod popup_about;
pub mod popup_keybinds;
pub mod popup_preview;
pub mod search;
pub mod textinput;
pub mod theme;
pub mod uiconfig;

use component::Component;
use dialog::Dialog;
use header::Header;
use panes::Panes;
use popup_about::PopupAbout;
use popup_keybinds::PopupKeybinds;
use popup_preview::PopupPreview;
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
    clipboard: Vec<PathBuf>,
    clipboard_cut: bool,
    pending_d: bool,
    pending_editor_file: Option<PathBuf>,
    pending_shell: bool,
    progress: Option<Progress>,
}

impl App {
    pub fn new(theme: Theme, config: Config) -> Self {
        let panes = Panes::new(&config);

        let current_directory = config.get_initial_dir();

        let header = Header::new(current_directory);
        Self {
            exit: false,
            theme,
            ui_config: UiConfig::new(),
            header,
            footer: Footer::default(),
            panes,
            config,
            preview: None,
            dialog: None,
            search: None,
            command: None,
            clipboard: Vec::new(),
            clipboard_cut: false,
            pending_d: false,
            pending_editor_file: None,
            pending_shell: false,
            progress: None,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;

            if self.progress.is_some() {
                // While a transfer runs, poll with a timeout so the gauge
                // updates and Esc stays responsive.
                if crossterm::event::poll(Duration::from_millis(50))? {
                    self.handle_input()?;
                }
                self.pump_progress();
            } else {
                self.handle_input()?;
            }

            if let Some(path) = self.pending_editor_file.take() {
                terminal.clear()?;
                Command::new(&self.config.editor).arg(&path).status()?;
                terminal.clear()?;
                self.panes.reload(&self.config, true);
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }

            if self.pending_shell {
                self.pending_shell = false;
                terminal.clear()?;
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
                let _ = Command::new(&shell).status();
                terminal.clear()?;
                self.panes.reload(&self.config, true);
                self.header
                    .update(self.panes.get_active_pane().path.to_string());
            }
        }
        Ok(())
    }

    /// Drains progress messages from a background transfer; finishes it when
    /// the worker reports Done.
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

    fn render(&mut self, frame: &mut Frame<'_>) {
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
        }

        if self.ui_config.active_keybind_popup {
            PopupKeybinds::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_about_popup {
            PopupAbout::new().render(frame, &self.theme, &self.ui_config, frame.area());
        }

        if self.ui_config.active_preview_popup {
            let current = self.panes.get_active_pane().get_selected_entry();
            if self.preview.as_ref().and_then(|p| p.selected()) != current.as_ref() {
                self.preview = current.map(|e| PopupPreview::new(Some(e)));
            }
            if let Some(preview) = self.preview.as_mut() {
                preview.render(frame, &self.theme, &self.ui_config, frame.area());
            }
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
