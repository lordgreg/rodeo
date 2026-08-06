//! The two directory panes.
//!
//! A [`Pane`] owns its listing, cursor, selection and filter. Columns adapt to
//! the available width ([`ColumnSet`]), so a narrow terminal degrades to
//! name/size/date instead of squeezing everything.

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
    time::SystemTime,
};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, HorizontalAlignment, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    ui::{
        component::Component,
        git::{self, GitEntryStatus, GitStatus as GitEntryState},
        search::FilterSpec,
        theme::Theme,
        uiconfig::ActivePane,
    },
};

/// Width of the size column: `123.4 KB` plus a space.
const SIZE_COLUMN: u16 = 9;
/// Width of the modification-date column: `2026-07-31 06:44`.
const DATE_COLUMN: u16 = 17;
/// Width of the git status column: the two porcelain characters.
const GIT_COLUMN: u16 = 2;
/// Width of the permission column: `rwxr-xr-x`.
const PERMS_COLUMN: u16 = 9;
/// Width of the owner column.
const OWNER_COLUMN: u16 = 10;
/// Name column width below which extra columns are not worth their space.
const NAME_BUDGET: u16 = 22;

/// Which optional columns fit in a pane of the given width.
///
/// Extra columns are added one at a time, each only when the name column can
/// still show a reasonable file name afterwards, so a narrow pane degrades to
/// name/size/date instead of squeezing everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ColumnSet {
    pub git: bool,
    pub permissions: bool,
    pub owner: bool,
}

impl ColumnSet {
    pub(crate) fn for_width(width: u16) -> Self {
        // Marker, name, size, date and the spacing between them.
        let fixed = 1 + SIZE_COLUMN + DATE_COLUMN + 4;
        let mut spare = width.saturating_sub(fixed + NAME_BUDGET);
        let mut set = Self::default();

        for (needed, flag) in [
            (GIT_COLUMN + 1, &mut set.git),
            (PERMS_COLUMN + 1, &mut set.permissions),
            (OWNER_COLUMN + 1, &mut set.owner),
        ] {
            if spare >= needed {
                *flag = true;
                spare -= needed;
            }
        }

        set
    }

    fn constraints(self) -> Vec<Constraint> {
        let mut constraints = vec![Constraint::Max(1), Constraint::Fill(1)];
        if self.git {
            constraints.push(Constraint::Length(GIT_COLUMN));
        }
        if self.permissions {
            constraints.push(Constraint::Length(PERMS_COLUMN));
        }
        if self.owner {
            constraints.push(Constraint::Length(OWNER_COLUMN));
        }
        constraints.push(Constraint::Length(SIZE_COLUMN));
        constraints.push(Constraint::Length(DATE_COLUMN));
        constraints
    }

    fn headers(self) -> Vec<EntryHeader> {
        let mut headers = vec![
            EntryHeader::new(String::new(), SortType::Flagged),
            EntryHeader::new("Name".to_string(), SortType::Name),
        ];
        if self.git {
            headers.push(EntryHeader::plain(""));
        }
        if self.permissions {
            headers.push(EntryHeader::plain("Perms"));
        }
        if self.owner {
            headers.push(EntryHeader::plain("Owner"));
        }
        headers.push(EntryHeader::new("Size".to_string(), SortType::Size));
        headers.push(EntryHeader::new("Modify Time".to_string(), SortType::Time));
        headers
    }
}

pub(crate) fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

pub(crate) fn format_date(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone, Copy)]
/// Column the listing is ordered by.
pub enum SortType {
    Flagged,
    Name,
    Size,
    Time,
}

impl SortType {
    /// The next column in the rotation, wrapping.
    pub fn next(self) -> Self {
        match self {
            Self::Flagged => Self::Name,
            Self::Name => Self::Size,
            Self::Size => Self::Time,
            Self::Time => Self::Flagged,
        }
    }

    /// The previous column in the rotation, wrapping.
    pub fn prev(self) -> Self {
        match self {
            Self::Flagged => Self::Time,
            Self::Time => Self::Size,
            Self::Size => Self::Name,
            Self::Name => Self::Flagged,
        }
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone, Copy)]
/// Direction of the active sort.
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
/// What a listing entry is. Symlinks keep their *resolved* kind so that
/// navigating and editing follow the link; only broken or exotic targets stay
/// [`EntryKind::Symlink`].
pub enum EntryKind {
    Parent,
    Directory,
    Symlink,
    File,
    Unknown,
}

#[derive(Debug, Default, Clone, Copy)]
/// Counts shown in the header and footer for the active pane.
pub struct PaneStats {
    pub files: usize,
    pub dirs: usize,
    pub selected: usize,
    pub hidden: usize,
}

#[derive(Debug)]
/// A column heading, and the sort it triggers (if any).
pub struct EntryHeader {
    pub name: String,
    /// Column the header sorts by, or `None` for informational columns that
    /// must never show a sort indicator.
    pub kind: Option<SortType>,
}

impl EntryHeader {
    pub fn new(name: String, kind: SortType) -> Self {
        Self {
            name,
            kind: Some(kind),
        }
    }

    /// A header that cannot be sorted by (permissions, owner, git status).
    pub fn plain(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// One row of a listing: a filesystem entry with everything needed to render
/// and sort it, gathered once when the directory is read.
pub struct Entry {
    pub kind: EntryKind,
    pub path: PathBuf,
    pub name: String,
    pub size: String,
    pub modified: String,
    pub selected: bool,
    pub raw_size: u64,
    pub raw_modified: SystemTime,
    pub git_status: Option<GitEntryState>,
    pub is_symlink: bool,
    pub link_target: Option<PathBuf>,
    /// Cumulative size for directories, computed on demand (`S`).
    pub dir_size: Option<String>,
    /// Unix mode as `rwxr-xr-x`, shown when the pane is wide enough.
    pub permissions: String,
    /// Owning user name (falling back to the numeric uid).
    pub owner: String,
}

impl Entry {
    pub fn new(path: PathBuf) -> Self {
        // symlink_metadata does not follow links: one call classifies most
        // entries. Symlinks keep their *resolved* kind (File/Directory) so
        // navigation and editing follow the link; only broken or
        // non-file/non-dir targets stay EntryKind::Symlink.
        let (is_symlink, link_target, kind) = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let resolved = if path.is_file() {
                    EntryKind::File
                } else if path.is_dir() {
                    EntryKind::Directory
                } else {
                    EntryKind::Symlink
                };
                (true, std::fs::read_link(&path).ok(), resolved)
            }
            Ok(meta) if meta.is_dir() => (false, None, EntryKind::Directory),
            Ok(meta) if meta.is_file() => (false, None, EntryKind::File),
            _ => (false, None, EntryKind::Unknown),
        };

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "-".to_string());

        let (permissions, owner) = path
            .symlink_metadata()
            .map(|meta| (format_permissions(&meta), owner_of(&meta)))
            .unwrap_or_else(|_| ("-".to_string(), "-".to_string()));

        let (size, modified, raw_size, raw_modified) = path
            .metadata()
            .ok()
            .map(|meta| {
                (
                    format_size(meta.len()),
                    meta.modified()
                        .ok()
                        .map(format_date)
                        .unwrap_or_else(|| "-".to_string()),
                    meta.len(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
            .unwrap_or_else(|| ("-".to_string(), "-".to_string(), 0, SystemTime::UNIX_EPOCH));

        Self {
            kind,
            path,
            name,
            size,
            modified,
            selected: false,
            raw_size,
            raw_modified,
            git_status: None,
            is_symlink,
            link_target,
            dir_size: None,
            permissions,
            owner,
        }
    }

    pub fn parent(dir: &str) -> Self {
        let path = match PathBuf::from(dir).join("..").canonicalize() {
            Ok(c) => c,
            Err(_) => PathBuf::from(dir).join(".."),
        };

        Self {
            kind: EntryKind::Parent,
            path,
            name: String::from(".."),
            modified: String::from("-"),
            raw_modified: SystemTime::UNIX_EPOCH,
            raw_size: 0,
            selected: false,
            size: String::from("-"),
            git_status: None,
            is_symlink: false,
            link_target: None,
            dir_size: None,
            permissions: "-".to_string(),
            owner: "-".to_string(),
        }
    }

    pub fn toggle_selected(&mut self) {
        self.selected = !self.selected;
    }
}

/// `Span::styled` when there is a style, a plain span when there is not.
///
/// This four-line match was written out eight times inside `highlight_name`.
fn span(text: String, style: Option<Style>) -> Span<'static> {
    match style {
        Some(style) => Span::styled(text, style),
        None => Span::from(text),
    }
}

/// The active filter, compiled once so a listing can be highlighted row by row.
///
/// Building this per row was costing a pattern parse and a `nucleo::Matcher`
/// (or a regex compile) for every visible entry on every frame.
enum Highlighter {
    Off,
    Fuzzy {
        pattern: nucleo::pattern::Pattern,
        matcher: nucleo::Matcher,
        buf: Vec<char>,
        indices: Vec<u32>,
    },
    Regex(regex::Regex),
}

impl Highlighter {
    fn new(filter: Option<&FilterSpec>) -> Self {
        match filter {
            None => Self::Off,
            Some(FilterSpec::Fuzzy(pattern)) => Self::Fuzzy {
                pattern: nucleo::pattern::Pattern::parse(
                    pattern,
                    nucleo::pattern::CaseMatching::Smart,
                    nucleo::pattern::Normalization::Smart,
                ),
                matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
                buf: Vec::new(),
                indices: Vec::new(),
            },
            // An invalid regex highlights nothing rather than failing the draw.
            Some(FilterSpec::Regex(pattern)) => match regex::Regex::new(pattern) {
                Ok(re) => Self::Regex(re),
                Err(_) => Self::Off,
            },
        }
    }

    /// Character ranges of `name` that the filter matched, in order and
    /// non-overlapping. Adjacent characters are merged into one range so a
    /// run of matches becomes a single span.
    fn matched_ranges(&mut self, name: &str) -> Vec<Range<usize>> {
        match self {
            Self::Off => Vec::new(),
            Self::Fuzzy {
                pattern,
                matcher,
                buf,
                indices,
            } => {
                indices.clear();
                let haystack = nucleo::Utf32Str::new(name, buf);
                if pattern.score(haystack, matcher).is_none() {
                    return Vec::new();
                }

                // `score` and `indices` each need the haystack, and building it
                // borrows `buf`, so it is rebuilt here rather than held.
                let haystack = nucleo::Utf32Str::new(name, buf);
                pattern.indices(haystack, matcher, indices);
                indices.sort_unstable();
                indices.dedup();

                let char_count = name.chars().count();
                let mut ranges: Vec<Range<usize>> = Vec::new();
                for &idx in indices.iter() {
                    let idx = idx as usize;
                    if idx >= char_count {
                        continue;
                    }
                    match ranges.last_mut() {
                        Some(last) if last.end == idx => last.end = idx + 1,
                        _ => ranges.push(idx..idx + 1),
                    }
                }
                ranges
            }
            Self::Regex(re) => match re.find(name) {
                // Byte offsets from the regex, character offsets out: the
                // caller indexes into a Vec<char>.
                Some(m) => {
                    let start = name[..m.start()].chars().count();
                    let end = start + m.as_str().chars().count();
                    std::iter::once(start..end).collect()
                }
                None => Vec::new(),
            },
        }
    }
}

#[derive(Debug, Clone)]
/// One directory pane: its listing, cursor, selection and filter.
pub struct Pane {
    pub state: TableState,
    pub path: String,
    /// Every entry in the directory. The single source of truth for what is
    /// selected, how big a directory is, and so on.
    entries: Vec<Entry>,
    /// Indices into [`Self::entries`], in display order: the filter narrows
    /// and reorders this rather than cloning the entries it keeps.
    visible: Vec<usize>,
    filter: Option<FilterSpec>,
    hidden_count: usize,
    sort_type: SortType,
    sort_order: SortOrder,
    /// Mirrors `config.icons`; kept per pane so rendering needs no config.
    icons: bool,
    /// Repository summary from the same `git status` run that filled in the
    /// per-entry statuses, so the header does not have to run git again.
    git_summary: Option<git::RepoSummary>,
}

impl Pane {
    pub fn new(config: &Config, path: &str) -> Self {
        let path = path.to_string();
        let (entries, hidden_count, git_summary) = read_entries(&path, config);
        let sort_order = config.sort_order;
        let sort_type = config.sort_type;

        let mut pane = Self {
            state: TableState::default(),
            path,
            visible: (0..entries.len()).collect(),
            entries,
            filter: None,
            hidden_count,
            sort_order,
            sort_type,
            icons: config.icons,
            git_summary,
        };

        pane.state.select(Some(0));
        pane
    }

    pub fn filter(&self) -> Option<&FilterSpec> {
        self.filter.as_ref()
    }

    /// Branch and repository-wide counts, from the listing's own `git` run.
    pub fn git_summary(&self) -> Option<&git::RepoSummary> {
        self.git_summary.as_ref()
    }

    /// Narrows the visible entries by the given filter. The parent entry (`..`)
    /// always stays pinned on top. Fuzzy matches are ranked best-first; regex
    /// matches keep their listing order. An invalid regex is returned as an
    /// error and leaves the current filter untouched.
    pub fn set_filter(&mut self, filter: FilterSpec) -> Result<(), regex::Error> {
        match &filter {
            FilterSpec::Fuzzy(pattern) => {
                if pattern.is_empty() {
                    self.show_all();
                } else {
                    let parsed = nucleo::pattern::Pattern::parse(
                        pattern,
                        nucleo::pattern::CaseMatching::Smart,
                        nucleo::pattern::Normalization::Smart,
                    );
                    let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
                    let mut buf = Vec::new();
                    self.apply_rank(|name| {
                        parsed.score(nucleo::Utf32Str::new(name, &mut buf), &mut matcher)
                    });
                }
            }
            FilterSpec::Regex(pattern) => {
                let re = regex::Regex::new(pattern)?;
                self.apply_rank(|name| if re.is_match(name) { Some(1) } else { None });
            }
        }
        self.filter = Some(filter);
        Ok(())
    }

    pub fn clear_filter(&mut self) {
        self.show_all();
        self.filter = None;
        self.state.select(Some(0));
    }

    /// Rebuilds the visible list, keeping entries the rank function scores
    /// `Some` and ordering them best-first (stable).
    fn apply_rank(&mut self, mut rank: impl FnMut(&str) -> Option<u32>) {
        let mut parents: Vec<usize> = Vec::new();
        let mut scored: Vec<(u32, usize)> = Vec::new();

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.kind == EntryKind::Parent {
                parents.push(i);
            } else if let Some(score) = rank(&entry.name) {
                scored.push((score, i));
            }
        }
        // Stable, so equally-scored entries keep their listing order.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));

        let parent_count = parents.len();
        self.visible = parents
            .into_iter()
            .chain(scored.into_iter().map(|(_, i)| i))
            .collect();

        // Select the first real entry (after the pinned parent) if any.
        self.state
            .select(Some(if self.visible.len() > parent_count {
                parent_count
            } else {
                0
            }));
    }

    /// Shows every entry, in listing order.
    fn show_all(&mut self) {
        self.visible = (0..self.entries.len()).collect();
    }

    /// The entries on screen, in display order.
    fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        self.visible.iter().filter_map(|&i| self.entries.get(i))
    }

    /// The entry shown on `row`, if there is one.
    fn visible_entry(&self, row: usize) -> Option<&Entry> {
        self.entries.get(*self.visible.get(row)?)
    }

    fn visible_entry_mut(&mut self, row: usize) -> Option<&mut Entry> {
        self.entries.get_mut(*self.visible.get(row)?)
    }

    /// How many entries are on screen.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn select_by_path(&mut self, path: &Path) {
        let row = self.visible_entries().position(|e| e.path == path);
        if let Some(row) = row {
            self.state.select(Some(row));
        }
    }

    pub fn get_selected_entry(&self) -> Option<Entry> {
        let selected = self.state.selected()?;

        self.visible_entry(selected).cloned()
    }

    pub fn stats(&self) -> PaneStats {
        let mut stats = PaneStats {
            hidden: self.hidden_count,
            ..Default::default()
        };

        for entry in self.visible_entries() {
            match entry.kind {
                EntryKind::Parent => {}
                EntryKind::Directory => stats.dirs += 1,
                _ => stats.files += 1,
            }

            if entry.selected {
                stats.selected += 1;
            }
        }

        stats
    }

    /// Name spans for one entry, with the filter's matches picked out.
    ///
    /// `highlighter` is built once per frame by the caller: this used to parse
    /// the pattern and construct a `nucleo::Matcher` — or compile the regex —
    /// once per row, per frame.
    fn highlight_name(
        highlighter: &mut Highlighter,
        name: &str,
        base_style: Option<Style>,
        theme: &Theme,
    ) -> Vec<Span<'static>> {
        let matched = highlighter.matched_ranges(name);
        if matched.is_empty() {
            return vec![span(name.to_string(), base_style)];
        }

        let highlight_style = match base_style {
            Some(style) => style.bg(theme.colors.warning()),
            None => Style::new().bg(theme.colors.warning()),
        };

        let chars: Vec<char> = name.chars().collect();
        let mut spans = Vec::new();
        let mut at = 0;

        for range in matched {
            if range.start > at {
                spans.push(span(chars[at..range.start].iter().collect(), base_style));
            }
            spans.push(Span::styled(
                chars[range.start..range.end].iter().collect::<String>(),
                highlight_style,
            ));
            at = range.end;
        }

        if at < chars.len() {
            spans.push(span(chars[at..].iter().collect(), base_style));
        }

        spans
    }

    /// Message to show when the listing has nothing worth displaying.
    fn placeholder(&self) -> Option<&'static str> {
        let entries = self
            .visible_entries()
            .filter(|e| e.kind != EntryKind::Parent)
            .count();
        if entries > 0 {
            return None;
        }
        match self.filter {
            Some(_) => Some("(no matches)"),
            None => Some("(empty directory)"),
        }
    }

    fn entry_rows(&self, theme: &Theme, columns: ColumnSet) -> Vec<Row<'static>> {
        // Compiled once for the whole listing, not once per row.
        let mut highlighter = Highlighter::new(self.filter.as_ref());

        self.visible_entries()
            .map(|e| {
                let marker = if e.selected { "●" } else { "" };
                let size = match e.kind {
                    EntryKind::Directory => e.dir_size.clone().unwrap_or_else(|| "DIR".to_string()),
                    EntryKind::Parent => String::from("UP"),
                    _ => e.size.clone(),
                };

                let name_cell = {
                    // Broken/special symlinks (kind stays Symlink) stand out
                    // in the error color; git colors apply otherwise.
                    let name_style = if e.kind == EntryKind::Symlink {
                        Some(Style::new().fg(theme.colors.error()))
                    } else {
                        e.git_status.map(|s| Style::new().fg(s.kind.color(theme)))
                    };

                    let mut spans = Vec::new();
                    if self.icons {
                        // Coloured like the name so git status and broken
                        // symlinks stay readable at a glance.
                        spans.push(Span::styled(
                            format!("{} ", icon_for(e)),
                            name_style.unwrap_or_else(|| Style::new().fg(theme.colors.muted())),
                        ));
                    }
                    spans.extend(Self::highlight_name(
                        &mut highlighter,
                        &e.name,
                        name_style,
                        theme,
                    ));
                    if let Some(target) = &e.link_target {
                        spans.push(Span::styled(
                            format!(" -> {}", target.display()),
                            Style::new().fg(theme.colors.muted()),
                        ));
                    }
                    Cell::from(Line::from(spans))
                };

                let mut cells = vec![
                    Cell::from(marker.to_string()).style(theme.colors.accent1()),
                    name_cell,
                ];
                if columns.git {
                    cells.push(Cell::from(git_cell(e, theme)));
                }
                if columns.permissions {
                    cells.push(Cell::from(
                        Line::from(e.permissions.clone()).style(theme.colors.muted()),
                    ));
                }
                if columns.owner {
                    cells.push(Cell::from(
                        Line::from(e.owner.clone()).style(theme.colors.muted()),
                    ));
                }
                // Right-aligned so magnitudes line up and are comparable.
                cells.push(Cell::from(
                    Line::from(size).alignment(HorizontalAlignment::Right),
                ));
                cells.push(Cell::from(
                    Line::from(e.modified.clone()).style(theme.colors.muted()),
                ));

                Row::new(cells)
            })
            .collect()
    }

    pub fn toggle_select(&mut self) {
        let Some(i) = self.state.selected() else {
            return;
        };
        let Some(entry) = self.visible_entry_mut(i) else {
            return;
        };

        if !matches!(
            entry.kind,
            EntryKind::File | EntryKind::Directory | EntryKind::Symlink
        ) {
            return;
        }

        entry.toggle_selected();
    }

    /// Entries marked as selected (via `x`) *and* currently on screen.
    ///
    /// Deliberately not every selected entry: an operation must not touch
    /// something the active filter is hiding.
    pub fn selected_entries(&self) -> Vec<Entry> {
        self.visible_entries()
            .filter(|e| e.selected)
            .cloned()
            .collect()
    }

    /// Whether anything is selected, filtered out or not.
    pub fn has_selections(&self) -> bool {
        self.entries.iter().any(|e| e.selected)
    }

    /// Marks every selectable entry matching the wildcard pattern; returns
    /// how many entries were (newly) selected.
    pub fn select_matching(&mut self, pattern: &str) -> usize {
        let mut count = 0;
        for entry in &mut self.entries {
            let selectable = matches!(
                entry.kind,
                EntryKind::File | EntryKind::Directory | EntryKind::Symlink
            );
            if selectable && wildcard_match(pattern, &entry.name) {
                if !entry.selected {
                    count += 1;
                }
                entry.selected = true;
            }
        }
        count
    }

    /// Marks all selectable entries; returns the number selected.
    pub fn select_all(&mut self) -> usize {
        self.select_matching("*")
    }

    /// Computes cumulative sizes for all directory entries (capped walk with
    /// a shared entry budget so huge trees stay cheap). Returns how many
    /// directory sizes were computed.
    pub fn compute_dir_sizes(&mut self) -> usize {
        const ENTRY_BUDGET: u64 = 200_000;
        let mut budget = ENTRY_BUDGET;

        let dir_paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Directory)
            .map(|e| e.path.clone())
            .collect();

        let mut computed = 0;
        for path in dir_paths {
            if budget == 0 {
                break;
            }
            let est = crate::fs::ops::total_size_capped(std::slice::from_ref(&path), budget);
            budget = budget.saturating_sub(est.entries);

            let text = if est.truncated {
                format!("≥{}", format_size(est.bytes))
            } else {
                format_size(est.bytes)
            };
            if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
                e.dir_size = Some(text);
            }
            computed += 1;
        }
        computed
    }

    pub fn clear_selections(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
    }

    pub fn reload(&mut self, config: &Config, clear_selection: bool) {
        // Remember the highlighted entry so a background refresh (filesystem
        // watcher, transfer, editor exit) does not throw the cursor back to
        // the top of the list.
        let cursor_path = self.get_selected_entry().map(|e| e.path);

        let selected_paths: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|p| p.selected)
            .map(|p| p.path.clone())
            .collect();

        (self.entries, self.hidden_count, self.git_summary) = read_entries(&self.path, config);

        if !clear_selection {
            for entry in &mut self.entries {
                if selected_paths.contains(&entry.path) {
                    entry.selected = true
                }
            }
        }

        self.sort_order = config.sort_order;
        self.sort_type = config.sort_type;
        self.icons = config.icons;

        // Re-apply the active filter on the fresh listing.
        if let Some(filter) = self.filter.clone() {
            let _ = self.set_filter(filter);
        } else {
            self.show_all();
        }

        // Restore the cursor on the same entry when it still exists; after a
        // directory change the old path is gone and the cursor starts at the
        // top.
        let cursor_index = cursor_path
            .and_then(|p| self.visible_entries().position(|e| e.path == p))
            .unwrap_or(0);
        self.state = TableState::default();
        self.state.select(Some(cursor_index));
    }

    pub fn go_to_parent(&mut self, current_path: &str) -> OpenAction {
        if let Some(parent) = Path::new(current_path).parent() {
            let resolved = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            self.path = resolved.to_string_lossy().to_string();

            OpenAction::DirectoryOpened
        } else {
            OpenAction::Nothing
        }
    }

    pub fn open(&mut self) -> OpenAction {
        let Some(i) = self.state.selected() else {
            return OpenAction::Nothing;
        };

        let Some(entry) = self.visible_entry(i) else {
            return OpenAction::Nothing;
        };

        match entry.kind {
            EntryKind::File => OpenAction::FileOpened(entry.path.clone()),
            EntryKind::Parent => {
                let previous = self.path.clone();

                self.go_to_parent(&previous)
            }
            EntryKind::Directory => match entry.path.canonicalize() {
                Ok(p) => {
                    self.path = p.to_string_lossy().to_string();
                    OpenAction::Reload
                }
                Err(e) => {
                    log::error!("cannot open directory {}", e);
                    OpenAction::Nothing
                }
            },
            _ => OpenAction::Nothing,
        }
        // OpenAction::Nothing
    }

    pub fn header_to_cell(
        header: &'_ EntryHeader,
        current_sort_type: SortType,
        current_sort_order: SortOrder,
    ) -> Cell<'_> {
        let mut name = header.name.to_string();
        let mut cell = Cell::from(name);

        if header.kind == Some(current_sort_type) {
            cell = cell.bold();

            let symbol = if current_sort_order == SortOrder::Ascending {
                "▾"
            } else {
                "▴"
            };

            name = format!("{} {}", header.name, symbol);
            cell = cell.content(name);
        }

        cell
    }

    /// Argument order matches [`Component::render`], with the extra `active`
    /// flag last: the two used to be transposed, so `theme` and `area` could
    /// be swapped between them without the compiler noticing.
    pub fn render(&mut self, frame: &mut Frame, theme: &Theme, area: Rect, active: bool) {
        // Borders eat two cells; decide what fits in what is left.
        let columns = ColumnSet::for_width(area.width.saturating_sub(2));
        let rows = self.entry_rows(theme, columns);

        let color_border = if active {
            theme.colors.secondary()
        } else {
            theme.colors.border()
        };

        let headers = columns.headers();
        let header = Row::new(
            headers
                .iter()
                .map(|f| Self::header_to_cell(f, self.sort_type, self.sort_order)),
        )
        .style(Style::new().fg(theme.colors.highlight()));

        let width = (area.width as usize).saturating_sub_signed(8);

        let path = if self.path.len() > width {
            format!("...{}", &self.path[self.path.len().saturating_sub(width)..])
        } else {
            self.path.clone()
        };

        // The cursor row is only shown in the focused pane, and the unfocused
        // pane recedes: with two identical tables side by side, a border
        // colour alone is not enough to say where the keyboard is pointing.
        let row_highlight = if active {
            Style::new()
                .fg(theme.colors.highlight())
                .bg(theme.colors.surface())
                .bold()
        } else {
            Style::new().fg(theme.colors.muted())
        };

        let base_style = if active {
            Style::new().fg(theme.colors.primary())
        } else {
            Style::new().fg(theme.colors.primary()).dim()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(path)
            .border_style(color_border)
            .title_style(Style::new().fg(theme.colors.primary()));
        let inner = block.inner(area);

        let table = Table::new(rows, columns.constraints())
            .header(header)
            .column_spacing(1)
            .style(base_style)
            .row_highlight_style(row_highlight)
            .block(block);

        frame.render_stateful_widget(table, area, &mut self.state);

        // An empty listing would otherwise render as a blank box.
        if let Some(message) = self.placeholder() {
            let y = inner
                .y
                .saturating_add(2)
                .min(inner.bottom().saturating_sub(1));
            let line = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(message)
                    .alignment(HorizontalAlignment::Center)
                    .style(Style::new().fg(theme.colors.muted())),
                line,
            );
        }
    }
}

/// Matches a name against a shell-style wildcard pattern supporting `*`
/// (any sequence, including empty) and `?` (exactly one character).
/// Case-sensitive, like glob.
pub(crate) fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();

    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', name idx at '*')

    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi + 1, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            // Backtrack: let '*' consume one more character.
            pi = sp;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Sorts entries in-place according to config settings.
/// Applies sort_type, sort_order, and optionally pins directories on top.
fn sort_entries(entries: &mut [Entry], config: &Config) {
    entries.sort_by(|a, b| {
        let mut cmp = match config.sort_type {
            SortType::Flagged => a.selected.cmp(&b.selected),
            SortType::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortType::Size => a.raw_size.cmp(&b.raw_size),
            SortType::Time => a.raw_modified.cmp(&b.raw_modified),
        };
        cmp = match config.sort_order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        };

        if config.directories_on_top {
            let a_is_dir = matches!(a.kind, EntryKind::Directory | EntryKind::Parent);
            let b_is_dir = matches!(b.kind, EntryKind::Directory | EntryKind::Parent);
            match (a_is_dir, b_is_dir) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {} // both same group, fall through to sort_type
            }
        }

        cmp
    });
}

fn read_entries(dir: &str, config: &Config) -> (Vec<Entry>, usize, Option<git::RepoSummary>) {
    let mut hidden_count = 0;

    let mut entries: Vec<Entry> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let is_hidden = p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'));
                if is_hidden {
                    hidden_count += 1;
                }
                config.show_hidden || !is_hidden
            })
            .map(Entry::new)
            .collect(),

        Err(e) => {
            log::error!("cannot read {}: {}", dir, e);

            vec![]
        }
    };

    sort_entries(&mut entries, config);

    entries.insert(0, Entry::parent(dir));

    // One `git status` run serves both the per-entry column and the header's
    // branch/counts, which is why the summary is carried back out of here.
    let git_summary = git::repo_info(Path::new(dir)).map(|info| {
        for entry in &mut entries {
            entry.git_status = info.entries.get(&entry.name).copied();
        }
        info.summary
    });

    (entries, hidden_count, git_summary)
}

#[derive(PartialEq)]
/// Cursor movement direction.
pub enum MoveDirection {
    Up,
    Down,
}

/// What the caller must do after `Enter` on an entry.
pub enum OpenAction {
    Reload,
    /// Only the path is needed to hand the file to the editor.
    FileOpened(PathBuf),
    Nothing,
    DirectoryOpened,
}

#[derive(Debug)]
/// The pair of panes and which one has focus.
pub struct Panes {
    pub pane_left: Pane,
    pub pane_right: Pane,
    active_pane: ActivePane,
}

impl Panes {
    pub fn new(config: &Config) -> Self {
        Self {
            pane_left: Pane::new(config, &config.initial_directory_left),
            pane_right: Pane::new(config, &config.initial_directory_right),
            active_pane: config.active_pane,
        }
    }

    pub fn set_active_pane(&mut self, pane: ActivePane) {
        self.active_pane = pane;
    }

    pub fn get_active_pane_mut(&mut self) -> &mut Pane {
        if self.active_pane == ActivePane::Left {
            &mut self.pane_left
        } else {
            &mut self.pane_right
        }
    }

    pub fn get_active_pane(&self) -> &Pane {
        if self.active_pane == ActivePane::Left {
            &self.pane_left
        } else {
            &self.pane_right
        }
    }

    /// Returns the filesystem paths currently displayed in both panes.
    pub fn pane_dirs(&self) -> [PathBuf; 2] {
        [
            PathBuf::from(&self.pane_left.path),
            PathBuf::from(&self.pane_right.path),
        ]
    }

    pub fn get_inactive_pane(&self) -> &Pane {
        if self.active_pane == ActivePane::Left {
            &self.pane_right
        } else {
            &self.pane_left
        }
    }

    pub fn toggle_active_pane(&mut self) {
        self.active_pane = if self.active_pane == ActivePane::Left {
            ActivePane::Right
        } else {
            ActivePane::Left
        }
    }

    pub fn reload(&mut self, config: &Config, clear_selection: bool) {
        self.pane_left.reload(config, clear_selection);
        self.pane_right.reload(config, clear_selection);
    }

    pub fn next_index(row_count: usize, current: Option<usize>, direction: MoveDirection) -> usize {
        let max = row_count.saturating_sub(1);
        match direction {
            MoveDirection::Down => match current {
                Some(i) if i >= max => 0,
                Some(i) => i + 1,
                None => 0,
            },
            MoveDirection::Up => match current {
                Some(0) => max,
                Some(i) => i - 1,
                None => 0,
            },
        }
    }

    pub fn goto_next(&mut self, direction: MoveDirection) {
        let pane = self.get_active_pane_mut();
        let next = Self::next_index(pane.visible_len(), pane.state.selected(), direction);

        pane.state.select(Some(next));
    }

    pub fn goto_first(&mut self) {
        self.get_active_pane_mut().state.select_first();
    }

    pub fn goto_last(&mut self) {
        self.get_active_pane_mut().state.select_last();
    }
}

impl Component for Panes {
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let active = self.active_pane;
        self.pane_left
            .render(frame, theme, layout[0], active == ActivePane::Left);
        self.pane_right
            .render(frame, theme, layout[1], active == ActivePane::Right);
    }
}

/// Unix mode as `rwxr-xr-x`. Empty on platforms without unix permissions.
fn format_permissions(meta: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = meta.permissions().mode();
        let bit = |shift: u32, chars: [char; 3]| -> String {
            let bits = (mode >> shift) & 0o7;
            [
                if bits & 0o4 != 0 { chars[0] } else { '-' },
                if bits & 0o2 != 0 { chars[1] } else { '-' },
                if bits & 0o1 != 0 { chars[2] } else { '-' },
            ]
            .iter()
            .collect()
        };

        format!(
            "{}{}{}",
            bit(6, ['r', 'w', 'x']),
            bit(3, ['r', 'w', 'x']),
            bit(0, ['r', 'w', 'x'])
        )
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        "-".to_string()
    }
}

/// Owning user name, falling back to the numeric uid.
fn owner_of(meta: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let uid = meta.uid();
        user_name(uid).unwrap_or_else(|| uid.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        "-".to_string()
    }
}

/// uid → user name from `/etc/passwd`, read once.
///
/// Deliberately dependency-free: this is a cosmetic column, not worth a crate.
/// Users that only exist in a directory service (LDAP/SSSD) are not in
/// `/etc/passwd`, so callers fall back to the numeric uid.
#[cfg(unix)]
fn user_name(uid: u32) -> Option<String> {
    use std::sync::OnceLock;

    static USERS: OnceLock<std::collections::HashMap<u32, String>> = OnceLock::new();

    let users = USERS.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        let Ok(passwd) = std::fs::read_to_string("/etc/passwd") else {
            return map;
        };
        for line in passwd.lines() {
            let mut fields = line.split(':');
            let (Some(name), Some(_), Some(uid)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            if let Ok(uid) = uid.parse::<u32>() {
                map.entry(uid).or_insert_with(|| name.to_string());
            }
        }
        map
    });

    users.get(&uid).cloned()
}

/// The two porcelain characters, coloured so staged and unstaged changes are
/// distinguishable at a glance: index state in the success colour, worktree
/// state in the warning colour.
fn git_cell(entry: &Entry, theme: &Theme) -> Line<'static> {
    let Some(status) = entry.git_status else {
        return Line::from("  ");
    };

    match status.kind {
        GitEntryStatus::Untracked | GitEntryStatus::Ignored => Line::from(Span::styled(
            status.code.iter().collect::<String>(),
            Style::new().fg(status.kind.color(theme)),
        )),
        _ => Line::from(vec![
            Span::styled(
                status.code[0].to_string(),
                Style::new().fg(theme.colors.success()),
            ),
            Span::styled(
                status.code[1].to_string(),
                Style::new().fg(theme.colors.warning()),
            ),
        ]),
    }
}

/// Nerd Font glyph for an entry, by kind, then well-known name, then
/// extension.
///
/// Codepoints are written as escapes on purpose: the glyphs live in the
/// Unicode private use area, where they are invisible in most editors and easy
/// to mangle when the source is copied around. Only reached when
/// `config.icons` is on, which requires a patched font.
fn icon_for(entry: &Entry) -> &'static str {
    match entry.kind {
        EntryKind::Parent => return "\u{f062}",  // arrow-up
        EntryKind::Symlink => return "\u{f0c1}", // link
        EntryKind::Unknown => return "\u{f128}", // question
        EntryKind::Directory => {
            return match entry.name.as_str() {
                ".git" => "\u{e702}",                              // git
                "src" | "source" => "\u{f121}",                    // code
                "target" | "build" | "dist" | "out" => "\u{f1b3}", // cubes
                "tests" | "test" => "\u{f0c3}",                    // flask
                "docs" | "doc" => "\u{f02d}",                      // book
                ".config" | "config" | "etc" => "\u{f013}",        // gear
                "themes" | "assets" | "images" => "\u{f03e}",      // picture
                _ => "\u{f07b}",                                   // folder
            };
        }
        EntryKind::File => {}
    }

    // Whole-name matches beat extensions: a Makefile has no extension, and
    // Cargo.toml deserves the Rust glyph rather than the generic TOML one.
    match entry.name.as_str() {
        "Cargo.toml" | "Cargo.lock" => return "\u{e7a8}", // rust
        "Makefile" | "makefile" | "CMakeLists.txt" | "justfile" => return "\u{f0ad}", // wrench
        "Dockerfile" | "docker-compose.yml" | "compose.yaml" => return "\u{f308}", // docker
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "\u{e702}", // git
        "LICENSE" | "LICENCE" | "COPYING" => return "\u{f02d}", // book
        _ => {}
    }

    let extension = entry
        .path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "rs" => "\u{e7a8}",
        "py" => "\u{e73c}",
        "js" | "mjs" | "cjs" => "\u{e74e}",
        "ts" | "tsx" => "\u{e628}",
        "go" => "\u{e627}",
        "c" | "h" => "\u{e61e}",
        "cpp" | "cc" | "hpp" => "\u{e61d}",
        "java" | "class" => "\u{e738}",
        "rb" => "\u{e739}",
        "php" => "\u{e73d}",
        "lua" => "\u{e620}",
        "sh" | "bash" | "zsh" | "fish" => "\u{f489}", // terminal
        "html" | "htm" => "\u{e736}",
        "css" | "scss" | "sass" => "\u{e749}",
        "json" => "\u{e60b}",
        "toml" | "ini" | "cfg" | "conf" => "\u{f013}", // gear
        "yaml" | "yml" => "\u{f013}",
        "xml" => "\u{e619}",
        "md" | "markdown" => "\u{e73e}",
        "txt" | "text" | "log" => "\u{f0f6}",
        "pdf" => "\u{f1c1}",
        "doc" | "docx" | "odt" => "\u{f1c2}",
        "xls" | "xlsx" | "ods" | "csv" => "\u{f1c3}",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" => "\u{f1c5}",
        "mp3" | "flac" | "wav" | "ogg" | "m4a" => "\u{f1c7}",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "\u{f1c8}",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" => "\u{f1c6}",
        "iso" | "img" => "\u{f1c0}",
        "lock" => "\u{f023}",
        "ttf" | "otf" | "woff" | "woff2" => "\u{f031}",
        "sql" | "db" | "sqlite" => "\u{f1c0}",
        "exe" | "bin" | "so" | "dll" | "o" => "\u{f471}",
        _ => "\u{f016}", // generic file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    mod format_size {
        use super::*;

        #[test]
        fn zero_bytes() {
            assert_eq!(format_size(0), "0 B");
        }

        #[test]
        fn bytes_below_one_kb_stay_in_bytes() {
            assert_eq!(format_size(1023), "1023 B");
        }

        #[test]
        fn one_kb() {
            assert_eq!(format_size(1024), "1.0 KB");
        }

        #[test]
        fn one_mb() {
            assert_eq!(format_size(1048576), "1.0 MB");
        }

        #[test]
        fn one_gb() {
            assert_eq!(format_size(1073741824), "1.0 GB");
        }

        #[test]
        fn one_tb() {
            assert_eq!(format_size(1099511627776), "1.0 TB");
        }

        #[test]
        fn beyond_tb_stays_in_tb() {
            assert_eq!(format_size(1099511627776 * 1024), "1024.0 TB");
        }

        #[test]
        fn fractional_units_round_to_one_decimal() {
            assert_eq!(format_size(1536), "1.5 KB");
        }
    }

    mod format_date {
        use super::*;

        #[test]
        fn unix_epoch() {
            assert_eq!(format_date(SystemTime::UNIX_EPOCH), "1970-01-01 00:00");
        }

        #[test]
        fn known_utc_timestamp() {
            // 2024-01-15 12:30:00 UTC
            let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1705321800);
            assert_eq!(format_date(t), "2024-01-15 12:30");
        }
    }

    mod entry_new {
        use super::*;

        #[test]
        fn file_entry() {
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            write!(tmp, "hello").unwrap();

            let entry = Entry::new(tmp.path().to_path_buf());

            assert_eq!(entry.kind, EntryKind::File);
            assert_eq!(
                entry.name,
                tmp.path().file_name().unwrap().to_string_lossy()
            );
            assert!(entry.raw_size > 0);
            assert!(!entry.selected);
        }

        #[test]
        fn directory_entry() {
            let tmp = tempfile::tempdir().unwrap();

            let entry = Entry::new(tmp.path().to_path_buf());

            assert_eq!(entry.kind, EntryKind::Directory);
            assert_eq!(
                entry.name,
                tmp.path().file_name().unwrap().to_string_lossy()
            );
        }

        #[test]
        fn nonexistent_path_is_unknown() {
            let entry = Entry::new(PathBuf::from("/definitely/does/not/exist/rodeo-test"));

            assert_eq!(entry.kind, EntryKind::Unknown);
            assert_eq!(entry.size, "-");
            assert_eq!(entry.modified, "-");
            assert_eq!(entry.raw_size, 0);
            assert_eq!(entry.raw_modified, SystemTime::UNIX_EPOCH);
        }

        #[cfg(unix)]
        #[test]
        fn symlink_to_file_resolves_kind_and_records_target() {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("real.txt");
            std::fs::File::create(&file).unwrap();
            let link = dir.path().join("link.txt");
            std::os::unix::fs::symlink(&file, &link).unwrap();

            let entry = Entry::new(link.clone());

            assert_eq!(entry.kind, EntryKind::File);
            assert!(entry.is_symlink);
            assert_eq!(entry.link_target, Some(file));
        }

        #[cfg(unix)]
        #[test]
        fn symlink_to_dir_resolves_to_directory() {
            let dir = tempfile::tempdir().unwrap();
            let sub = dir.path().join("sub");
            std::fs::create_dir(&sub).unwrap();
            let link = dir.path().join("linkdir");
            std::os::unix::fs::symlink(&sub, &link).unwrap();

            let entry = Entry::new(link);

            assert_eq!(entry.kind, EntryKind::Directory);
            assert!(entry.is_symlink);
        }

        #[cfg(unix)]
        #[test]
        fn broken_symlink_stays_symlink_kind() {
            let dir = tempfile::tempdir().unwrap();
            let link = dir.path().join("broken");
            std::os::unix::fs::symlink(dir.path().join("missing"), &link).unwrap();

            let entry = Entry::new(link);

            assert_eq!(entry.kind, EntryKind::Symlink);
            assert!(entry.is_symlink);
            assert!(entry.link_target.is_some());
        }

        #[cfg(unix)]
        #[test]
        fn regular_file_is_not_marked_as_symlink() {
            let tmp = tempfile::NamedTempFile::new().unwrap();

            let entry = Entry::new(tmp.path().to_path_buf());

            assert!(!entry.is_symlink);
            assert_eq!(entry.link_target, None);
        }
    }

    mod highlighter {
        use super::*;

        fn ranges(filter: Option<FilterSpec>, name: &str) -> Vec<Range<usize>> {
            Highlighter::new(filter.as_ref()).matched_ranges(name)
        }

        #[test]
        fn no_filter_highlights_nothing() {
            assert!(ranges(None, "readme.md").is_empty());
        }

        #[test]
        fn a_regex_marks_its_whole_match() {
            let got = ranges(Some(FilterSpec::Regex("read".into())), "readme.md");
            assert_eq!(got, vec![0..4]);
        }

        #[test]
        fn a_regex_that_does_not_match_marks_nothing() {
            assert!(ranges(Some(FilterSpec::Regex("zzz".into())), "readme.md").is_empty());
        }

        /// Ranges index into a `Vec<char>`, but the regex reports bytes.
        #[test]
        fn regex_ranges_are_character_offsets_not_byte_offsets() {
            let got = ranges(Some(FilterSpec::Regex("b".into())), "äöü_b");
            assert_eq!(got, vec![4..5], "three 2-byte chars precede the match");
        }

        #[test]
        fn an_invalid_regex_highlights_nothing_instead_of_failing() {
            assert!(ranges(Some(FilterSpec::Regex("[unclosed".into())), "a.txt").is_empty());
        }

        #[test]
        fn a_fuzzy_match_marks_the_matched_characters() {
            let got = ranges(Some(FilterSpec::Fuzzy("rdm".into())), "readme.md");
            let marked: String = "readme.md"
                .chars()
                .enumerate()
                .filter(|(i, _)| got.iter().any(|r| r.contains(i)))
                .map(|(_, c)| c)
                .collect();
            assert_eq!(marked, "rdm");
        }

        /// Consecutive hits collapse into one range so they render as a single
        /// span rather than one span per character.
        #[test]
        fn adjacent_fuzzy_matches_merge_into_one_range() {
            let got = ranges(Some(FilterSpec::Fuzzy("read".into())), "readme.md");
            assert_eq!(got, vec![0..4]);
        }

        #[test]
        fn ranges_are_ordered_and_do_not_overlap() {
            let got = ranges(Some(FilterSpec::Fuzzy("rme".into())), "readme.md");
            for pair in got.windows(2) {
                assert!(pair[0].end <= pair[1].start, "{got:?}");
            }
        }

        /// The matcher is reused across rows, so state from one name must not
        /// leak into the next.
        #[test]
        fn one_highlighter_handles_many_names_independently() {
            let mut hl = Highlighter::new(Some(&FilterSpec::Fuzzy("md".into())));

            let first = hl.matched_ranges("readme.md");
            let miss = hl.matched_ranges("zzz");
            let again = hl.matched_ranges("readme.md");

            assert!(miss.is_empty());
            assert_eq!(first, again, "a non-matching name must not corrupt state");
        }
    }

    mod filter {
        use super::*;

        /// Builds a pane over a temp dir containing: a.rs, ab.rs, b.txt, sub/
        /// (plus the synthesized `..` parent entry).
        fn test_pane() -> (tempfile::TempDir, Pane) {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            std::fs::File::create(dir.path().join("ab.rs")).unwrap();
            std::fs::File::create(dir.path().join("b.txt")).unwrap();
            std::fs::create_dir(dir.path().join("sub")).unwrap();

            let config = Config::default();
            let pane = Pane::new(&config, dir.path().to_str().unwrap());
            (dir, pane)
        }

        fn names(pane: &Pane) -> Vec<&str> {
            pane.visible_entries().map(|e| e.name.as_str()).collect()
        }

        #[test]
        fn fuzzy_filter_matches_subsequence() {
            let (_dir, mut pane) = test_pane();
            pane.set_filter(FilterSpec::Fuzzy("ab".to_string()))
                .unwrap();

            assert_eq!(names(&pane), vec!["..", "ab.rs"]);
            assert_eq!(pane.visible_entry(0).unwrap().kind, EntryKind::Parent);
            // First real entry is selected.
            assert_eq!(pane.state.selected(), Some(1));
        }

        #[test]
        fn fuzzy_empty_pattern_shows_everything() {
            let (_dir, mut pane) = test_pane();
            pane.set_filter(FilterSpec::Fuzzy(String::new())).unwrap();

            assert_eq!(pane.visible_len(), 5);
        }

        #[test]
        fn regex_filter_matches_pattern() {
            let (_dir, mut pane) = test_pane();
            pane.set_filter(FilterSpec::Regex("^a".to_string()))
                .unwrap();

            assert_eq!(names(&pane), vec!["..", "a.rs", "ab.rs"]);
        }

        #[test]
        fn invalid_regex_returns_error_and_keeps_listing() {
            let (_dir, mut pane) = test_pane();
            let result = pane.set_filter(FilterSpec::Regex("(".to_string()));

            assert!(result.is_err());
            assert_eq!(pane.visible_len(), 5);
            assert_eq!(pane.filter(), None);
        }

        #[test]
        fn clear_filter_restores_full_listing() {
            let (_dir, mut pane) = test_pane();
            pane.set_filter(FilterSpec::Regex("^a".to_string()))
                .unwrap();
            assert_eq!(pane.visible_len(), 3);

            pane.clear_filter();
            assert_eq!(pane.visible_len(), 5);
            assert_eq!(pane.filter(), None);
        }

        #[test]
        fn reload_reapplies_active_filter() {
            let (dir, mut pane) = test_pane();
            pane.set_filter(FilterSpec::Regex("^a".to_string()))
                .unwrap();

            std::fs::File::create(dir.path().join("ax.rs")).unwrap();
            pane.reload(&Config::default(), false);

            assert_eq!(names(&pane), vec!["..", "a.rs", "ab.rs", "ax.rs"]);
        }

        #[test]
        fn select_by_path_moves_cursor() {
            let (dir, mut pane) = test_pane();
            let target = dir.path().join("b.txt");
            pane.select_by_path(&target);

            assert_eq!(
                pane.get_selected_entry().map(|e| e.name),
                Some("b.txt".to_string())
            );
        }
    }

    /// The listing used to be two vectors — every visible entry was a *clone*
    /// of one in the full list — so each mutation had to be written twice, in
    /// five places, each with its own hand-rolled strategy. These pin the
    /// behaviour that used to depend on getting that right.
    mod visible_and_full_listing_stay_consistent {
        use super::*;

        fn pane_with_files(names: &[&str]) -> (tempfile::TempDir, Pane) {
            let dir = tempfile::tempdir().unwrap();
            for name in names {
                std::fs::File::create(dir.path().join(name)).unwrap();
            }
            let pane = Pane::new(&Config::default(), dir.path().to_str().unwrap());
            (dir, pane)
        }

        fn row_of(pane: &Pane, name: &str) -> usize {
            pane.visible_entries()
                .position(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} is not visible"))
        }

        #[test]
        fn a_selection_survives_being_filtered_out_and_back() {
            let (_dir, mut pane) = pane_with_files(&["keep.rs", "other.txt"]);

            let row = row_of(&pane, "keep.rs");
            pane.state.select(Some(row));
            pane.toggle_select();
            assert!(pane.has_selections());

            // Filter it out of view, then bring it back.
            pane.set_filter(FilterSpec::Fuzzy("other".into())).unwrap();
            assert!(pane.has_selections(), "a hidden entry is still selected");

            pane.clear_filter();
            let row = row_of(&pane, "keep.rs");
            assert!(
                pane.visible_entry(row).unwrap().selected,
                "the selection must come back with the entry"
            );
        }

        #[test]
        fn selected_entries_only_reports_what_is_on_screen() {
            let (_dir, mut pane) = pane_with_files(&["keep.rs", "other.txt"]);

            let row = row_of(&pane, "keep.rs");
            pane.state.select(Some(row));
            pane.toggle_select();

            pane.set_filter(FilterSpec::Fuzzy("other".into())).unwrap();
            assert!(
                pane.selected_entries().is_empty(),
                "an operation must not touch what the filter hides"
            );
            assert!(pane.has_selections(), "but it is still selected");
        }

        #[test]
        fn select_matching_reaches_entries_the_filter_hides() {
            let (_dir, mut pane) = pane_with_files(&["a.rs", "b.rs", "c.txt"]);
            pane.set_filter(FilterSpec::Fuzzy("c.txt".into())).unwrap();

            assert_eq!(pane.select_matching("*.rs"), 2);

            pane.clear_filter();
            let selected: Vec<&str> = pane
                .visible_entries()
                .filter(|e| e.selected)
                .map(|e| e.name.as_str())
                .collect();
            assert_eq!(selected, vec!["a.rs", "b.rs"]);
        }

        #[test]
        fn clearing_selections_clears_hidden_ones_too() {
            let (_dir, mut pane) = pane_with_files(&["a.rs", "b.rs"]);
            pane.select_matching("*");
            assert!(pane.has_selections());

            pane.set_filter(FilterSpec::Fuzzy("a".into())).unwrap();
            pane.clear_selections();
            pane.clear_filter();

            assert!(!pane.has_selections(), "nothing may stay selected");
        }

        #[test]
        fn a_filter_reorders_without_losing_entries() {
            let (_dir, mut pane) = pane_with_files(&["a.rs", "b.rs", "c.rs"]);
            let total = pane.entries.len();

            pane.set_filter(FilterSpec::Regex("b".into())).unwrap();
            assert!(pane.visible_len() < total);

            pane.clear_filter();
            assert_eq!(pane.visible_len(), total, "every entry comes back");
        }
    }

    mod selection {
        use super::*;

        fn test_pane() -> (tempfile::TempDir, Pane) {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            std::fs::File::create(dir.path().join("b.txt")).unwrap();
            std::fs::create_dir(dir.path().join("sub")).unwrap();

            let config = Config::default();
            let pane = Pane::new(&config, dir.path().to_str().unwrap());
            (dir, pane)
        }

        fn select_index(pane: &mut Pane, i: usize) {
            pane.state.select(Some(i));
            pane.toggle_select();
        }

        #[test]
        fn file_can_be_selected_and_gathered() {
            let (_dir, mut pane) = test_pane();
            // Default sort (dirs on top): [.., sub, a.rs, b.txt]
            select_index(&mut pane, 2);

            let selected = pane.selected_entries();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].name, "a.rs");
            assert!(pane.has_selections());
        }

        #[test]
        fn directory_can_be_selected() {
            let (_dir, mut pane) = test_pane();
            select_index(&mut pane, 1); // sub

            let selected = pane.selected_entries();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].kind, EntryKind::Directory);
        }

        #[test]
        fn parent_cannot_be_selected() {
            let (_dir, mut pane) = test_pane();
            select_index(&mut pane, 0); // ..

            assert!(pane.selected_entries().is_empty());
            assert!(!pane.has_selections());
        }

        #[test]
        fn clear_selections_unmarks_all() {
            let (_dir, mut pane) = test_pane();
            select_index(&mut pane, 1);
            select_index(&mut pane, 2);
            assert_eq!(pane.selected_entries().len(), 2);

            pane.clear_selections();
            assert!(pane.selected_entries().is_empty());
            assert!(!pane.has_selections());
        }

        #[test]
        fn selections_survive_reload() {
            let (_dir, mut pane) = test_pane();
            select_index(&mut pane, 2); // a.rs

            pane.reload(&Config::default(), false);
            let selected = pane.selected_entries();
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].name, "a.rs");
        }

        #[test]
        fn cursor_survives_reload() {
            let (dir, mut pane) = test_pane();
            pane.state.select(Some(3)); // b.txt
            assert_eq!(
                pane.get_selected_entry().map(|e| e.name),
                Some("b.txt".to_string())
            );

            // A new file shifts indices; the cursor must stay on b.txt.
            std::fs::File::create(dir.path().join("aa.rs")).unwrap();
            pane.reload(&Config::default(), false);

            assert_eq!(
                pane.get_selected_entry().map(|e| e.name),
                Some("b.txt".to_string())
            );
        }

        #[test]
        fn cursor_resets_when_entry_disappears() {
            let (dir, mut pane) = test_pane();
            pane.state.select(Some(3)); // b.txt

            std::fs::remove_file(dir.path().join("b.txt")).unwrap();
            pane.reload(&Config::default(), false);

            assert_eq!(pane.state.selected(), Some(0));
        }
    }

    mod icons {
        use super::*;

        fn file(name: &str) -> Entry {
            let mut entry = Entry::new(PathBuf::from(name));
            entry.kind = EntryKind::File;
            entry.name = name.to_string();
            entry
        }

        #[test]
        fn well_known_names_beat_extensions() {
            // Cargo.toml is Rust, not "some TOML file".
            assert_ne!(icon_for(&file("Cargo.toml")), icon_for(&file("other.toml")));
        }

        #[test]
        fn extensions_are_case_insensitive() {
            assert_eq!(icon_for(&file("PHOTO.JPG")), icon_for(&file("photo.jpg")));
        }

        #[test]
        fn kinds_win_over_names() {
            let mut dir = Entry::new(PathBuf::from("Cargo.toml"));
            dir.kind = EntryKind::Directory;
            dir.name = "Cargo.toml".to_string();
            assert_ne!(icon_for(&dir), icon_for(&file("Cargo.toml")));
        }

        #[test]
        fn unknown_extensions_still_get_a_glyph() {
            assert!(!icon_for(&file("mystery.qqq")).is_empty());
        }
    }

    mod columns {
        use super::*;

        #[test]
        fn narrow_panes_keep_only_the_essentials() {
            let set = ColumnSet::for_width(40);
            assert_eq!(set, ColumnSet::default());
        }

        #[test]
        fn columns_appear_one_at_a_time_as_width_grows() {
            // git is cheapest, so it arrives first, then permissions, then owner.
            let narrow = ColumnSet::for_width(58);
            assert!(narrow.git && !narrow.permissions && !narrow.owner);

            let medium = ColumnSet::for_width(70);
            assert!(medium.git && medium.permissions && !medium.owner);

            let wide = ColumnSet::for_width(120);
            assert!(wide.git && wide.permissions && wide.owner);
        }

        #[test]
        fn cells_and_constraints_stay_in_step() {
            // A mismatch would silently shift every column in the table.
            for width in [30, 58, 70, 120, 250] {
                let set = ColumnSet::for_width(width);
                assert_eq!(
                    set.constraints().len(),
                    set.headers().len(),
                    "width {width}"
                );
            }
        }

        #[test]
        fn only_sortable_columns_carry_a_sort_kind() {
            let headers = ColumnSet::for_width(200).headers();
            let sortable = headers.iter().filter(|h| h.kind.is_some()).count();
            // Flagged marker, Name, Size, Modify Time.
            assert_eq!(sortable, 4);
        }
    }

    mod placeholder {
        use super::*;

        #[test]
        fn empty_directory_is_announced() {
            let dir = tempfile::tempdir().unwrap();
            let pane = Pane::new(&Config::default(), dir.path().to_str().unwrap());
            assert_eq!(pane.placeholder(), Some("(empty directory)"));
        }

        #[test]
        fn populated_directory_has_no_placeholder() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            let pane = Pane::new(&Config::default(), dir.path().to_str().unwrap());
            assert_eq!(pane.placeholder(), None);
        }

        #[test]
        fn filter_hiding_everything_says_no_matches() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            let mut pane = Pane::new(&Config::default(), dir.path().to_str().unwrap());
            pane.set_filter(FilterSpec::Regex("zzz".to_string()))
                .unwrap();
            assert_eq!(pane.placeholder(), Some("(no matches)"));
        }
    }

    mod wildcard {
        use super::*;

        #[test]
        fn star_matches_everything() {
            assert!(wildcard_match("*", "anything.rs"));
            assert!(wildcard_match("*", ""));
        }

        #[test]
        fn extension_pattern() {
            assert!(wildcard_match("*.rs", "main.rs"));
            assert!(!wildcard_match("*.rs", "main.toml"));
        }

        #[test]
        fn question_mark_matches_single_char() {
            assert!(wildcard_match("?.rs", "a.rs"));
            assert!(!wildcard_match("?.rs", "ab.rs"));
        }

        #[test]
        fn prefix_and_suffix() {
            assert!(wildcard_match("foo*", "foobar"));
            assert!(!wildcard_match("foo*", "barfoo"));
            assert!(wildcard_match("*bar", "foobar"));
            assert!(!wildcard_match("*bar", "barfoo"));
        }

        #[test]
        fn middle_star_backtracks() {
            assert!(wildcard_match("f*b*r", "foobar"));
            assert!(wildcard_match("f*b*r", "foobazbar"));
            assert!(!wildcard_match("f*b*r", "foobaz"));
        }

        #[test]
        fn exact_match_required_without_wildcards() {
            assert!(wildcard_match("exact", "exact"));
            assert!(!wildcard_match("exact", "exactly"));
        }

        #[test]
        fn unicode_names() {
            assert!(wildcard_match("*.txt", "日本語.txt"));
            assert!(wildcard_match("日?", "日本"));
        }

        #[test]
        fn select_matching_marks_and_counts() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            std::fs::File::create(dir.path().join("ab.rs")).unwrap();
            std::fs::File::create(dir.path().join("b.txt")).unwrap();

            let config = Config::default();
            let mut pane = Pane::new(&config, dir.path().to_str().unwrap());

            let count = pane.select_matching("*.rs");
            assert_eq!(count, 2);
            let names: Vec<String> = pane
                .selected_entries()
                .into_iter()
                .map(|e| e.name)
                .collect();
            assert!(names.contains(&"a.rs".to_string()));
            assert!(names.contains(&"ab.rs".to_string()));

            // Idempotent: second call selects nothing new.
            assert_eq!(pane.select_matching("*.rs"), 0);
        }

        #[test]
        fn select_all_marks_everything_selectable() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::File::create(dir.path().join("a.rs")).unwrap();
            std::fs::create_dir(dir.path().join("sub")).unwrap();

            let config = Config::default();
            let mut pane = Pane::new(&config, dir.path().to_str().unwrap());

            assert_eq!(pane.select_all(), 2);
            // Parent entry is never selected.
            assert!(
                pane.selected_entries()
                    .iter()
                    .all(|e| e.kind != EntryKind::Parent)
            );
        }
    }

    mod next_index {
        use super::*;

        #[test]
        fn down_wraps_from_last_to_first() {
            assert_eq!(Panes::next_index(3, Some(2), MoveDirection::Down), 0);
        }

        #[test]
        fn down_moves_to_next() {
            assert_eq!(Panes::next_index(3, Some(0), MoveDirection::Down), 1);
        }

        #[test]
        fn up_wraps_from_first_to_last() {
            assert_eq!(Panes::next_index(3, Some(0), MoveDirection::Up), 2);
        }

        #[test]
        fn up_moves_to_previous() {
            assert_eq!(Panes::next_index(3, Some(2), MoveDirection::Up), 1);
        }

        #[test]
        fn single_item_list_stays_at_zero() {
            assert_eq!(Panes::next_index(1, Some(0), MoveDirection::Down), 0);
            assert_eq!(Panes::next_index(1, Some(0), MoveDirection::Up), 0);
        }

        #[test]
        fn empty_list_returns_zero() {
            assert_eq!(Panes::next_index(0, None, MoveDirection::Down), 0);
            assert_eq!(Panes::next_index(0, None, MoveDirection::Up), 0);
        }

        #[test]
        fn none_selected_returns_zero() {
            assert_eq!(Panes::next_index(5, None, MoveDirection::Down), 0);
            assert_eq!(Panes::next_index(5, None, MoveDirection::Up), 0);
        }
    }

    mod sort_rotation {
        use super::*;

        #[test]
        fn next_and_prev_walk_the_whole_cycle() {
            let order = [
                SortType::Flagged,
                SortType::Name,
                SortType::Size,
                SortType::Time,
            ];

            let mut sort = SortType::Flagged;
            for expected in [
                SortType::Name,
                SortType::Size,
                SortType::Time,
                SortType::Flagged,
            ] {
                sort = sort.next();
                assert_eq!(sort, expected);
            }

            // Walking back must retrace the same cycle, not a different one.
            for expected in order.iter().rev() {
                sort = sort.prev();
                assert_eq!(sort, *expected);
            }
        }

        #[test]
        fn prev_is_the_inverse_of_next() {
            for sort in [
                SortType::Flagged,
                SortType::Name,
                SortType::Size,
                SortType::Time,
            ] {
                assert_eq!(sort.next().prev(), sort);
                assert_eq!(sort.prev().next(), sort);
            }
        }

        #[test]
        fn reversing_an_order_twice_restores_it() {
            assert_eq!(SortOrder::Ascending.reversed(), SortOrder::Descending);
            assert_eq!(
                SortOrder::Ascending.reversed().reversed(),
                SortOrder::Ascending
            );
        }
    }

    mod sort_entries {
        use super::*;

        fn make_file(name: &str, size: u64) -> Entry {
            Entry {
                path: PathBuf::from(name),
                name: name.to_string(),
                kind: EntryKind::File,
                size: format_size(size),
                raw_size: size,
                modified: String::new(),
                raw_modified: SystemTime::UNIX_EPOCH,
                selected: false,
                git_status: None,
                is_symlink: false,
                link_target: None,
                dir_size: None,
                permissions: "rw-r--r--".to_string(),
                owner: "tester".to_string(),
            }
        }

        fn make_dir(name: &str) -> Entry {
            let mut e = make_file(name, 0);
            e.kind = EntryKind::Directory;
            e
        }

        #[test]
        fn sort_by_name_ascending() {
            let mut entries = vec![
                make_file("zebra.txt", 100),
                make_file("alpha.txt", 100),
                make_file("beta.txt", 100),
            ];
            let config = Config {
                sort_type: SortType::Name,
                sort_order: SortOrder::Ascending,
                directories_on_top: false,
                ..Config::default()
            };
            super::sort_entries(&mut entries, &config);

            assert_eq!(entries[0].name, "alpha.txt");
            assert_eq!(entries[1].name, "beta.txt");
            assert_eq!(entries[2].name, "zebra.txt");
        }

        #[test]
        fn sort_by_name_descending() {
            let mut entries = vec![
                make_file("alpha.txt", 100),
                make_file("zebra.txt", 100),
                make_file("beta.txt", 100),
            ];
            let config = Config {
                sort_type: SortType::Name,
                sort_order: SortOrder::Descending,
                directories_on_top: false,
                ..Config::default()
            };
            super::sort_entries(&mut entries, &config);

            assert_eq!(entries[0].name, "zebra.txt");
            assert_eq!(entries[1].name, "beta.txt");
            assert_eq!(entries[2].name, "alpha.txt");
        }

        #[test]
        fn sort_by_size_ascending() {
            let mut entries = vec![
                make_file("big.txt", 1000),
                make_file("small.txt", 10),
                make_file("medium.txt", 500),
            ];
            let config = Config {
                sort_type: SortType::Size,
                sort_order: SortOrder::Ascending,
                directories_on_top: false,
                ..Config::default()
            };
            super::sort_entries(&mut entries, &config);

            assert_eq!(entries[0].name, "small.txt");
            assert_eq!(entries[1].name, "medium.txt");
            assert_eq!(entries[2].name, "big.txt");
        }

        #[test]
        fn directories_on_top() {
            let mut entries = vec![
                make_file("aaa-file.txt", 100),
                make_dir("zzz-dir"),
                make_file("bbb-file.txt", 100),
                make_dir("aaa-dir"),
            ];
            let config = Config {
                sort_type: SortType::Name,
                sort_order: SortOrder::Ascending,
                directories_on_top: true,
                ..Config::default()
            };
            super::sort_entries(&mut entries, &config);

            // Directories should be first, then files, both alphabetically
            assert_eq!(entries[0].name, "aaa-dir");
            assert_eq!(entries[1].name, "zzz-dir");
            assert_eq!(entries[2].name, "aaa-file.txt");
            assert_eq!(entries[3].name, "bbb-file.txt");
        }

        #[test]
        fn sort_by_flagged() {
            let mut entries = vec![
                make_file("a.txt", 100),
                make_file("b.txt", 100),
                make_file("c.txt", 100),
            ];
            entries[2].selected = true;
            entries[0].selected = true;

            let config = Config {
                sort_type: SortType::Flagged,
                sort_order: SortOrder::Descending,
                directories_on_top: false,
                ..Config::default()
            };

            super::sort_entries(&mut entries, &config);

            // Descending: selected (true) comes before unselected (false)
            assert!(entries[0].selected);
            assert!(entries[1].selected);
            assert!(!entries[2].selected);
        }
    }
}
