use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use ratatui::style::Color;

use crate::ui::theme::Theme;

/// Per-entry git worktree status, derived from `git status --porcelain=v1 -z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitEntryStatus {
    /// Tracked, with staged and/or unstaged modifications (also renames/type changes).
    Modified,
    /// Newly added to the index, not yet committed.
    Added,
    /// Deleted from the worktree and/or index.
    Deleted,
    /// Not tracked by git.
    Untracked,
    /// Matched by .gitignore.
    Ignored,
}

impl GitEntryStatus {
    /// Higher severity wins when a directory aggregates multiple statuses.
    fn severity(self) -> u8 {
        match self {
            Self::Deleted => 5,
            Self::Modified => 4,
            Self::Added => 3,
            Self::Untracked => 2,
            Self::Ignored => 1,
        }
    }

    fn merge(self, other: Self) -> Self {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }

    pub fn color(self, theme: &Theme) -> Color {
        match self {
            Self::Modified => theme.colors.warning(),
            Self::Added => theme.colors.success(),
            Self::Deleted => theme.colors.error(),
            Self::Untracked => theme.colors.info(),
            Self::Ignored => theme.colors.muted(),
        }
    }
}

/// Maps the *name* of every direct child of `pane_dir` to its git status.
///
/// Files are matched directly; directories aggregate the most severe status of
/// any status-bearing path beneath them. Returns `None` outside a git worktree.
pub fn status_map(pane_dir: &Path) -> Option<HashMap<String, GitEntryStatus>> {
    let root = repo_root(pane_dir)?;
    let output = Command::new("git")
        .args([
            "-C",
            pane_dir.to_str()?,
            "status",
            "--porcelain=v1",
            "-z",
            "-uall",
            "--ignored=matching",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let statuses = parse_porcelain_z(&output.stdout, Path::new(&root));
    Some(aggregate(pane_dir, statuses))
}

fn repo_root(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", dir.to_str()?, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() { None } else { Some(root) }
}

/// Parses `git status --porcelain=v1 -z` output into (absolute path, status)
/// pairs. Rename/copy entries (`R`/`C`) contribute only their new path; the
/// following source-path field is skipped.
fn parse_porcelain_z(output: &[u8], repo_root: &Path) -> Vec<(PathBuf, GitEntryStatus)> {
    let text = String::from_utf8_lossy(output);
    let fields: Vec<&str> = text.split('\0').collect();

    let mut statuses = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let field = fields[i];
        i += 1;

        if field.len() < 4 {
            continue; // trailing empty field after the last NUL
        }

        let x = field.as_bytes()[0] as char;
        let y = field.as_bytes()[1] as char;
        let path = &field[3..];

        let status = match (x, y) {
            ('?', _) => GitEntryStatus::Untracked,
            ('!', _) => GitEntryStatus::Ignored,
            ('D', _) | (_, 'D') => GitEntryStatus::Deleted,
            ('A', _) => GitEntryStatus::Added,
            _ => GitEntryStatus::Modified,
        };
        statuses.push((repo_root.join(path), status));

        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            i += 1; // skip the source path of the rename/copy
        }
    }

    statuses
}

/// Reduces absolute status paths to a map of pane-dir child names, folding
/// nested paths into their top-level directory with severity merge.
fn aggregate(
    pane_dir: &Path,
    statuses: Vec<(PathBuf, GitEntryStatus)>,
) -> HashMap<String, GitEntryStatus> {
    let mut map: HashMap<String, GitEntryStatus> = HashMap::new();

    for (path, status) in statuses {
        let Ok(rel) = path.strip_prefix(pane_dir) else {
            continue; // outside this pane's directory
        };
        let Some(name) = rel
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
        else {
            continue;
        };

        map.entry(name)
            .and_modify(|s| *s = s.merge(status))
            .or_insert(status);
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn parses_basic_statuses() {
        let out = b" M src/main.rs\0?? new.txt\0!! target\0A  staged.rs\0 D gone.rs\0";
        let statuses = parse_porcelain_z(out, &root());

        assert_eq!(
            statuses,
            vec![
                (root().join("src/main.rs"), GitEntryStatus::Modified),
                (root().join("new.txt"), GitEntryStatus::Untracked),
                (root().join("target"), GitEntryStatus::Ignored),
                (root().join("staged.rs"), GitEntryStatus::Added),
                (root().join("gone.rs"), GitEntryStatus::Deleted),
            ]
        );
    }

    #[test]
    fn double_letter_codes_parse() {
        let out = b"MM both.rs\0AM added_mod.rs\0";
        let statuses = parse_porcelain_z(out, &root());

        assert_eq!(
            statuses,
            vec![
                (root().join("both.rs"), GitEntryStatus::Modified),
                (root().join("added_mod.rs"), GitEntryStatus::Added),
            ]
        );
    }

    #[test]
    fn rename_contributes_new_path_and_skips_source() {
        let out = b"R  new.rs\0old.rs\0 M other.rs\0";
        let statuses = parse_porcelain_z(out, &root());

        assert_eq!(
            statuses,
            vec![
                (root().join("new.rs"), GitEntryStatus::Modified),
                (root().join("other.rs"), GitEntryStatus::Modified),
            ]
        );
    }

    #[test]
    fn empty_output_parses_to_nothing() {
        assert!(parse_porcelain_z(b"", &root()).is_empty());
    }

    #[test]
    fn aggregate_maps_files_and_folds_directories() {
        let pane = PathBuf::from("/repo/sub");
        let statuses = vec![
            (PathBuf::from("/repo/sub/dir/x.rs"), GitEntryStatus::Ignored),
            (
                PathBuf::from("/repo/sub/dir/y.rs"),
                GitEntryStatus::Modified,
            ),
            (
                PathBuf::from("/repo/sub/file.rs"),
                GitEntryStatus::Untracked,
            ),
            (
                PathBuf::from("/repo/elsewhere.rs"),
                GitEntryStatus::Modified,
            ),
        ];

        let map = aggregate(&pane, statuses);

        assert_eq!(map.len(), 2);
        // Severity merge: Modified (4) beats Ignored (1) inside dir/.
        assert_eq!(map.get("dir"), Some(&GitEntryStatus::Modified));
        assert_eq!(map.get("file.rs"), Some(&GitEntryStatus::Untracked));
        // Paths outside the pane are dropped.
        assert!(!map.contains_key("elsewhere.rs"));
    }

    #[test]
    fn severity_ordering() {
        assert!(GitEntryStatus::Deleted.severity() > GitEntryStatus::Modified.severity());
        assert!(GitEntryStatus::Modified.severity() > GitEntryStatus::Added.severity());
        assert!(GitEntryStatus::Added.severity() > GitEntryStatus::Untracked.severity());
        assert!(GitEntryStatus::Untracked.severity() > GitEntryStatus::Ignored.severity());
    }
}
