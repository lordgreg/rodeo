//! Find-in-files popup: recursive regex search over file contents, honouring
//! `.gitignore`.

use std::path::PathBuf;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::ui::{component::Component, textinput::TextInput, theme::Theme};

#[derive(Debug, Clone)]
pub struct FindMatch {
    pub path: PathBuf,
    pub line_num: usize,
    pub line_content: String,
}

impl FindMatch {
    pub fn display_path(&self) -> String {
        format!(
            "{}:{}: {}",
            self.path.display(),
            self.line_num,
            self.line_content.trim()
        )
    }
}

/// State for the find-in-files popup.
#[derive(Debug, Default)]
pub struct FindInFiles {
    pub input: TextInput,
    pub searching: bool,
    pub results: Vec<FindMatch>,
    pub list_state: ListState,
    /// The pattern the current `results` came from, so an empty list can be
    /// told apart from a search that has not been run yet — the popup must not
    /// claim "no matches" for a query nobody searched for.
    last_query: Option<String>,
}

impl FindInFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when `results` reflect exactly what is in the input box.
    pub fn results_are_current(&self) -> bool {
        self.last_query.as_deref() == Some(self.input.value.as_str())
    }

    pub fn start_search(&mut self, pattern: String) {
        self.searching = true;
        self.last_query = Some(pattern);
        self.results.clear();
        self.list_state.select(None);
    }

    pub fn finish_search(&mut self) {
        self.searching = false;
        if !self.results.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn add_result(&mut self, result: FindMatch) {
        self.results.push(result);
    }

    pub fn selected_match(&self) -> Option<&FindMatch> {
        self.list_state.selected().and_then(|i| self.results.get(i))
    }

    pub fn move_up(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        if i > 0 {
            self.list_state.select(Some(i - 1));
        }
    }

    pub fn move_down(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        if i + 1 < self.results.len() {
            self.list_state.select(Some(i + 1));
        }
    }

    pub fn clear(&mut self) {
        self.input = TextInput::default();
        self.results.clear();
        self.list_state.select(None);
        self.searching = false;
        self.last_query = None;
    }
}

impl Component for FindInFiles {
    fn render(
        &mut self,
        frame: &mut Frame<'_>,
        theme: &Theme,
        _ui: &crate::ui::uiconfig::UiConfig,
        area: Rect,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Find in Files (Ctrl+G)")
            .border_style(theme.colors.secondary());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into input area and results area
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);

        // Render input box
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title("Search pattern");
        let input_inner = input_block.inner(chunks[0]);
        frame.render_widget(input_block, chunks[0]);

        let input_text = Paragraph::new(Line::from(vec![
            Span::from(self.input.value.clone()),
            Span::from(" "),
        ]));
        frame.render_widget(input_text, input_inner);

        // Position cursor
        if !self.searching {
            frame.set_cursor_position((input_inner.x + self.input.cursor as u16, input_inner.y));
        }

        // Render results or status message
        if self.searching {
            let msg = Paragraph::new("Searching...").style(Style::new().fg(theme.colors.info()));
            frame.render_widget(msg, chunks[1]);
        } else if !self.results_are_current() {
            // Nothing has been searched for what is in the box yet. Say so
            // rather than showing a verdict on a search that never ran.
            let hint = if self.input.value.is_empty() {
                "Type a regular expression, then Enter to search this directory and below"
            } else {
                "Press Enter to search"
            };
            let msg = Paragraph::new(hint).style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, chunks[1]);
        } else if self.results.is_empty() {
            let msg =
                Paragraph::new("No matches found").style(Style::new().fg(theme.colors.muted()));
            frame.render_widget(msg, chunks[1]);
        } else {
            let items: Vec<ListItem> = self
                .results
                .iter()
                .map(|m| {
                    let line = Line::from(vec![
                        Span::styled(
                            format!("{}:{}", m.path.display(), m.line_num),
                            Style::new().fg(theme.colors.accent1()),
                        ),
                        Span::from(": "),
                        Span::from(m.line_content.trim()),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::new()
                        .fg(theme.colors.highlight())
                        .bg(theme.colors.surface())
                        .bold(),
                )
                .highlight_symbol("› ");

            frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
        }
    }
}
