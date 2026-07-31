//! Completion for the `:` command line.
//!
//! Behaves like Vim's wildmenu: candidates are offered as you type, `Tab`
//! walks forward through them and `Shift+Tab` backwards, and the chosen one
//! replaces the word under the cursor. Unlike Vim, the menu is shown
//! immediately rather than only after the first `Tab`, so the available
//! commands are discoverable without knowing they exist.

use std::path::{Path, PathBuf};

use crate::ui::{
    command::{self, ArgKind},
    theme::Theme,
};

/// One entry of the completion menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Text inserted into the command line.
    pub value: String,
    /// Argument placeholder, shown after the value (`<path>`).
    pub args: String,
    /// Explanation shown next to it.
    pub description: String,
}

impl Candidate {
    fn command(spec: &command::CommandSpec, name: &str) -> Self {
        Self {
            value: name.to_string(),
            args: spec.args.to_string(),
            description: spec.description.to_string(),
        }
    }

    fn plain(value: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            args: String::new(),
            description: description.into(),
        }
    }
}

/// Live completion state for the command line.
#[derive(Debug, Clone, Default)]
pub struct Completion {
    candidates: Vec<Candidate>,
    /// Index into `candidates`, or `None` while nothing has been chosen yet.
    selected: Option<usize>,
    /// Byte offset in the command line where the completed word starts.
    replace_from: usize,
}

impl Completion {
    /// Recomputes the candidates for `line` (the text after the `:`).
    ///
    /// `pane_dir` is what relative paths are resolved against.
    pub fn compute(line: &str, pane_dir: &Path) -> Self {
        let (replace_from, candidates) = match line.split_once(char::is_whitespace) {
            // Still typing the command name.
            None => (0, command_candidates(line)),
            // Past the first space: complete the argument, if the command has one.
            Some((name, rest)) => {
                let arg_start = line.len() - rest.len();
                let spec = command::find(name);
                let candidates = match spec.map(|s| s.arg_kind) {
                    Some(ArgKind::Directory) => directory_candidates(rest, pane_dir),
                    Some(ArgKind::Theme) => theme_candidates(rest),
                    _ => Vec::new(),
                };
                (arg_start, candidates)
            }
        };

        Self {
            candidates,
            selected: None,
            replace_from,
        }
    }

    /// Candidates currently on offer.
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// Index of the highlighted candidate, if one has been chosen.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// `true` when there is a menu worth drawing.
    pub fn is_active(&self) -> bool {
        !self.candidates.is_empty()
    }

    /// Moves to the next (or previous) candidate, wrapping around, and returns
    /// the line that results from accepting it.
    pub fn cycle(&mut self, line: &str, forward: bool) -> Option<String> {
        if self.candidates.is_empty() {
            return None;
        }

        let last = self.candidates.len() - 1;
        self.selected = Some(match (self.selected, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(i), true) if i >= last => 0,
            (Some(i), true) => i + 1,
            (Some(0), false) => last,
            (Some(i), false) => i - 1,
        });

        Some(self.apply(line))
    }

    /// The command line with the selected candidate substituted in.
    fn apply(&self, line: &str) -> String {
        let Some(candidate) = self.selected.and_then(|i| self.candidates.get(i)) else {
            return line.to_string();
        };

        let mut completed = String::from(&line[..self.replace_from]);
        completed.push_str(&candidate.value);
        completed
    }
}

/// Commands whose name starts with `prefix`.
fn command_candidates(prefix: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for spec in command::COMMANDS {
        for name in spec.names {
            if name.starts_with(prefix) {
                candidates.push(Candidate::command(spec, name));
            }
        }
    }
    candidates
}

/// Installed themes starting with `prefix`.
fn theme_candidates(prefix: &str) -> Vec<Candidate> {
    Theme::get_theme_list()
        .into_iter()
        .filter(|name| name.starts_with(prefix))
        .map(|name| Candidate::plain(name, "theme"))
        .collect()
}

/// Directories matching `prefix`, which may be absolute, `~`-relative or
/// relative to the pane.
fn directory_candidates(prefix: &str, pane_dir: &Path) -> Vec<Candidate> {
    let expanded = expand_home(prefix);

    // Split into "directory to list" and "what the entry must start with".
    let (dir, partial) = match expanded.rsplit_once('/') {
        Some((dir, partial)) => (format!("{dir}/"), partial.to_string()),
        None => (String::new(), expanded.clone()),
    };

    let listing_dir = if dir.is_empty() {
        pane_dir.to_path_buf()
    } else if Path::new(&dir).is_absolute() {
        PathBuf::from(&dir)
    } else {
        pane_dir.join(&dir)
    };

    let Ok(entries) = std::fs::read_dir(&listing_dir) else {
        return Vec::new();
    };

    let mut candidates: Vec<Candidate> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&partial) {
                return None;
            }
            // Hidden directories only when explicitly asked for.
            if name.starts_with('.') && !partial.starts_with('.') {
                return None;
            }
            Some(Candidate::plain(format!("{dir}{name}/"), "directory"))
        })
        .collect();

    candidates.sort_by(|a, b| a.value.cmp(&b.value));
    candidates
}

/// Expands a leading `~` to the home directory.
fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_string();
    };
    format!("{}{}", home.to_string_lossy(), rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::create_dir(dir.path().join("alpine")).unwrap();
        std::fs::create_dir(dir.path().join("beta")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("a-file.txt"), "x").unwrap();
        dir
    }

    #[test]
    fn empty_line_offers_every_command() {
        let completion = Completion::compute("", Path::new("/"));
        let total: usize = command::COMMANDS.iter().map(|c| c.names.len()).sum();
        assert_eq!(completion.candidates().len(), total);
    }

    #[test]
    fn prefix_filters_commands_and_includes_aliases() {
        let completion = Completion::compute("q", Path::new("/"));
        let values: Vec<&str> = completion
            .candidates()
            .iter()
            .map(|c| c.value.as_str())
            .collect();
        assert_eq!(values, vec!["q", "quit"]);
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        let mut completion = Completion::compute("q", Path::new("/"));

        assert_eq!(completion.cycle("q", true).as_deref(), Some("q"));
        assert_eq!(completion.cycle("q", true).as_deref(), Some("quit"));
        // Past the end, back to the first.
        assert_eq!(completion.cycle("q", true).as_deref(), Some("q"));
        // And backwards from the first to the last.
        assert_eq!(completion.cycle("q", false).as_deref(), Some("quit"));
    }

    #[test]
    fn cycling_without_candidates_does_nothing() {
        let mut completion = Completion::compute("zzz", Path::new("/"));
        assert!(!completion.is_active());
        assert_eq!(completion.cycle("zzz", true), None);
    }

    #[test]
    fn directories_complete_for_cd_and_files_are_excluded() {
        let dir = temp_tree();
        let completion = Completion::compute("cd a", dir.path());

        let values: Vec<&str> = completion
            .candidates()
            .iter()
            .map(|c| c.value.as_str())
            .collect();
        assert_eq!(values, vec!["alpha/", "alpine/"]);
    }

    #[test]
    fn accepting_a_directory_keeps_the_command() {
        let dir = temp_tree();
        let mut completion = Completion::compute("cd a", dir.path());

        assert_eq!(completion.cycle("cd a", true).as_deref(), Some("cd alpha/"));
    }

    #[test]
    fn hidden_directories_need_an_explicit_dot() {
        let dir = temp_tree();
        assert!(
            Completion::compute("cd ", dir.path())
                .candidates()
                .iter()
                .all(|c| c.value != ".hidden/")
        );
        assert!(
            Completion::compute("cd .", dir.path())
                .candidates()
                .iter()
                .any(|c| c.value == ".hidden/")
        );
    }

    #[test]
    fn nested_paths_complete_within_their_parent() {
        let dir = temp_tree();
        std::fs::create_dir(dir.path().join("alpha/inner")).unwrap();

        let completion = Completion::compute("cd alpha/i", dir.path());
        let values: Vec<&str> = completion
            .candidates()
            .iter()
            .map(|c| c.value.as_str())
            .collect();
        assert_eq!(values, vec!["alpha/inner/"]);
    }

    #[test]
    fn commands_without_an_argument_offer_nothing() {
        let dir = temp_tree();
        assert!(!Completion::compute("quit ", dir.path()).is_active());
        // Free-text arguments have nothing sensible to offer either.
        assert!(!Completion::compute("mkdir a", dir.path()).is_active());
    }

    #[test]
    fn unknown_commands_offer_nothing_for_their_argument() {
        let dir = temp_tree();
        assert!(!Completion::compute("bogus a", dir.path()).is_active());
    }
}
