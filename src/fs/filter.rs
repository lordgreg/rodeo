//! What recursive searches are allowed to look at.
//!
//! Both the file finder (`/`) and find-in-files (`Ctrl+g`) walk a directory
//! tree, and both should ignore the same noise: build outputs, `.git`,
//! whatever `.gitignore` already says is uninteresting. Keeping the rules in
//! one place means the two searches can never disagree about what exists, and
//! gives the popups a single label to show the user what is being hidden.

use std::path::Path;

use crate::config::Config;

/// The filtering rules taken from the config, ready to build a walker from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// Honour `.gitignore` (plus `.ignore`, global and repo excludes).
    pub gitignore: bool,
    /// Skip dot-files and dot-directories.
    pub hidden: bool,
    /// Extra names to skip, see [`SearchFilter::excludes`].
    pub entries: Vec<String>,
}

impl SearchFilter {
    pub fn from_config(config: &Config) -> Self {
        Self {
            gitignore: config.filter_gitignore,
            hidden: config.filter_hidden,
            entries: config.filter_entries.clone(),
        }
    }

    /// Nothing filtered at all — searches see the whole tree.
    pub fn is_empty(&self) -> bool {
        !self.gitignore && !self.hidden && self.entries.is_empty()
    }

    /// One-line summary for a popup footer, so an unexpectedly short result
    /// list is explained where it is seen rather than in the config file.
    pub fn describe(&self) -> String {
        if self.is_empty() {
            return "filter: none".to_string();
        }

        let mut parts = Vec::new();
        if self.gitignore {
            parts.push("gitignore".to_string());
        }
        if self.hidden {
            parts.push("hidden".to_string());
        }
        match self.entries.len() {
            0 => {}
            // Few enough to name: far more useful than a count.
            1..=3 => parts.push(self.entries.join(", ")),
            n => parts.push(format!("{n} entries")),
        }
        format!("filter: {}", parts.join(" · "))
    }

    /// Whether `path` is knocked out by one of `entries`.
    ///
    /// A pattern matches when it equals the file name (`target`, `.git`), when
    /// it is `*.ext` and the name has that extension, or — when it contains a
    /// separator — when it appears as a run of components in the path
    /// (`src/generated`).
    pub fn excludes(&self, path: &Path) -> bool {
        self.entries.iter().any(|pattern| {
            let pattern = pattern.trim_matches('/');
            if pattern.is_empty() {
                return false;
            }
            if pattern.contains('/') {
                return path.to_string_lossy().contains(pattern);
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            match pattern.strip_prefix("*.") {
                Some(ext) => Path::new(name).extension().and_then(|e| e.to_str()) == Some(ext),
                None => name == pattern,
            }
        })
    }

    /// A directory walker over `root` obeying these rules.
    ///
    /// `require_git` is off so a `.gitignore` is respected even outside a
    /// repository: the user asked for those files to be ignored, and whether a
    /// `.git` directory happens to be present is not part of that wish.
    pub fn walk(&self, root: &Path) -> ignore::Walk {
        let excludes = self.clone();
        ignore::WalkBuilder::new(root)
            .hidden(self.hidden)
            .git_ignore(self.gitignore)
            .git_global(self.gitignore)
            .git_exclude(self.gitignore)
            .ignore(self.gitignore)
            .parents(self.gitignore)
            .require_git(false)
            // Depth 0 is the search root itself: a root that happens to be
            // called `target` must still be searchable.
            .filter_entry(move |entry| entry.depth() == 0 || !excludes.excludes(entry.path()))
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn with_entries(entries: &[&str]) -> SearchFilter {
        SearchFilter {
            entries: entries.iter().map(|e| e.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_bare_name_matches_the_file_name_anywhere() {
        let filter = with_entries(&["target", ".git"]);
        assert!(filter.excludes(&PathBuf::from("/home/u/p/target")));
        assert!(filter.excludes(&PathBuf::from("/home/u/p/deep/.git")));
        assert!(!filter.excludes(&PathBuf::from("/home/u/p/targets")));
        assert!(!filter.excludes(&PathBuf::from("/home/u/target/src/lib.rs")));
    }

    #[test]
    fn a_star_pattern_matches_by_extension() {
        let filter = with_entries(&["*.lock"]);
        assert!(filter.excludes(&PathBuf::from("/p/Cargo.lock")));
        assert!(!filter.excludes(&PathBuf::from("/p/Cargo.toml")));
        assert!(!filter.excludes(&PathBuf::from("/p/lock")));
    }

    #[test]
    fn a_pattern_with_a_separator_matches_a_sub_path() {
        let filter = with_entries(&["src/generated"]);
        assert!(filter.excludes(&PathBuf::from("/p/src/generated/api.rs")));
        assert!(!filter.excludes(&PathBuf::from("/p/src/handwritten/api.rs")));
    }

    #[test]
    fn an_empty_filter_describes_itself_as_such() {
        assert_eq!(SearchFilter::default().describe(), "filter: none");
        assert!(SearchFilter::default().is_empty());
    }

    #[test]
    fn a_description_names_a_few_entries_and_counts_many() {
        let filter = SearchFilter {
            gitignore: true,
            hidden: true,
            entries: vec!["target".into(), "node_modules".into()],
        };
        assert_eq!(
            filter.describe(),
            "filter: gitignore · hidden · target, node_modules"
        );

        let many = with_entries(&["a", "b", "c", "d"]);
        assert_eq!(many.describe(), "filter: 4 entries");
    }

    #[test]
    fn the_walker_skips_filtered_directories_but_not_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("target");
        std::fs::create_dir_all(root.join("debug")).unwrap();
        std::fs::write(root.join("debug/out.bin"), "x").unwrap();
        std::fs::write(root.join("keep.txt"), "x").unwrap();

        let filter = with_entries(&["debug"]);
        let names: Vec<String> = filter
            .walk(&root)
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"keep.txt".to_string()), "{names:?}");
        assert!(!names.contains(&"out.bin".to_string()), "{names:?}");
    }

    #[test]
    fn hidden_files_are_skipped_only_when_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".secret"), "x").unwrap();

        let hiding = SearchFilter {
            hidden: true,
            ..Default::default()
        };
        let count = |f: &SearchFilter| {
            f.walk(dir.path())
                .filter_map(Result::ok)
                .filter(|e| e.file_name() == ".secret")
                .count()
        };
        assert_eq!(count(&hiding), 0);
        assert_eq!(count(&SearchFilter::default()), 1);
    }
}
