//! Git status for the entries of a directory.
//!
//! Shells out to `git status --porcelain=v1 -z` once per pane reload and maps
//! the result onto the paths under that directory; directories aggregate the
//! most severe status found beneath them.
//!
//! The same run also yields a [`RepoSummary`] for the header, so the branch and
//! the change counts cost no extra subprocesses.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    thread,
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

/// A worktree status plus the raw porcelain code it came from.
///
/// The two characters are the staged (index) and unstaged (worktree) states —
/// `M ` is staged, ` M` is not, `MM` is both. Colours alone cannot express
/// that distinction, which is the whole point of the status column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitStatus {
    pub kind: GitEntryStatus,
    pub code: [char; 2],
}

impl GitStatus {
    /// Higher severity wins; the surviving status keeps its own code.
    fn merge(self, other: Self) -> Self {
        if other.kind.severity() > self.kind.severity() {
            other
        } else {
            self
        }
    }

    /// `true` when the index differs from HEAD (something is staged).
    pub fn is_staged(&self) -> bool {
        !matches!(self.code[0], ' ' | '?' | '!')
    }

    /// `true` when the worktree differs from the index.
    pub fn is_unstaged(&self) -> bool {
        !matches!(self.code[1], ' ' | '?' | '!')
    }
}

/// Repository-wide totals plus the current branch, for the header bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoSummary {
    /// Current branch, or `HEAD` when detached. Empty if git did not say.
    pub branch: String,
    /// Tracked paths that differ from HEAD (staged, unstaged, or both).
    pub modified: usize,
    /// Untracked paths. Ignored paths are deliberately not counted.
    pub untracked: usize,
}

/// What one `git status` run tells us about a directory.
///
/// Both views come from a single invocation. The header used to run its own
/// `git status` (plus two more `git` calls) alongside the one each pane
/// already ran, so navigating a directory cost seven `git` processes on the
/// UI thread; it now costs two per pane and none for the header.
#[derive(Debug, Clone, Default)]
pub struct RepoInfo {
    /// Status of every path under the directory queried, by absolute path.
    /// Directories carry the most severe status found beneath them.
    pub entries: HashMap<PathBuf, GitStatus>,
    /// Counts across the whole repository.
    pub summary: RepoSummary,
}

/// A [`repo_info`] call running on a worker thread.
///
/// `git status` is a subprocess walking a whole worktree: on a large or
/// cold-cache repository it takes long enough to be felt, and it used to run
/// inline in the pane reload, so every navigation froze the UI for as long as
/// git took. The listing is now drawn immediately without a status column and
/// the column fills in when the answer arrives, which is the one thing a
/// background thread has to buy to be worth having.
#[derive(Debug)]
pub struct PendingRepoInfo {
    rx: mpsc::Receiver<Option<RepoInfo>>,
}

impl PendingRepoInfo {
    /// Starts the `git` calls and returns without waiting for them.
    pub fn spawn(dir: &Path) -> Self {
        let (tx, rx) = mpsc::channel();
        let dir = dir.to_path_buf();

        thread::spawn(move || {
            // A send failure means the pane reloaded again and dropped the
            // receiver, so this answer is simply no longer wanted.
            let _ = tx.send(repo_info(&dir));
        });

        Self { rx }
    }

    /// The result if the worker has produced one. Never blocks.
    ///
    /// The outer `Option` is "has it finished"; the inner one is
    /// [`repo_info`]'s own "is this a worktree at all".
    pub fn take(&self) -> Option<Option<RepoInfo>> {
        match self.rx.try_recv() {
            Ok(info) => Some(info),
            Err(mpsc::TryRecvError::Empty) => None,
            // Disconnected without a value means the worker panicked. Report
            // "not a repository" so the pane stops waiting for ever.
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
        }
    }
}

/// Git status for `pane_dir`: per-path statuses and a repository summary.
///
/// Files are matched directly; directories aggregate the most severe status of
/// any status-bearing path beneath them. Returns `None` outside a git worktree.
///
/// Blocking, and called from a worker thread — see [`PendingRepoInfo`].
pub fn repo_info(pane_dir: &Path) -> Option<RepoInfo> {
    let (root, branch) = repo_head(pane_dir)?;
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
    let summary = summarize(&statuses, branch);
    Some(RepoInfo {
        entries: aggregate(pane_dir, statuses),
        summary,
    })
}

/// The repository root and the current branch, in one `git` call.
///
/// `rev-parse` takes both questions at once and answers them on one line each,
/// which is why this is not two spawns.
fn repo_head(dir: &Path) -> Option<(String, String)> {
    let output = Command::new("git")
        .args([
            "-C",
            dir.to_str()?,
            "rev-parse",
            "--show-toplevel",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let root = lines.next().unwrap_or_default().trim().to_string();
    // Detached HEAD reports the literal `HEAD`; an unborn branch reports
    // nothing. Neither is worth failing over — the status is still useful.
    let branch = lines.next().unwrap_or_default().trim().to_string();

    if root.is_empty() {
        None
    } else {
        Some((root, branch))
    }
}

/// Repository-wide counts for the header.
///
/// Ignored paths are excluded so the number means "work in progress" rather
/// than "files git can see".
fn summarize(statuses: &[(PathBuf, GitStatus)], branch: String) -> RepoSummary {
    let mut summary = RepoSummary {
        branch,
        ..Default::default()
    };

    for (_, status) in statuses {
        match status.kind {
            GitEntryStatus::Untracked => summary.untracked += 1,
            GitEntryStatus::Ignored => {}
            _ => summary.modified += 1,
        }
    }

    summary
}

/// Parses `git status --porcelain=v1 -z` output into (absolute path, status)
/// pairs. Rename/copy entries (`R`/`C`) contribute only their new path; the
/// following source-path field is skipped.
fn parse_porcelain_z(output: &[u8], repo_root: &Path) -> Vec<(PathBuf, GitStatus)> {
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

        let kind = match (x, y) {
            ('?', _) => GitEntryStatus::Untracked,
            ('!', _) => GitEntryStatus::Ignored,
            ('D', _) | (_, 'D') => GitEntryStatus::Deleted,
            ('A', _) => GitEntryStatus::Added,
            _ => GitEntryStatus::Modified,
        };
        statuses.push((repo_root.join(path), GitStatus { kind, code: [x, y] }));

        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            i += 1; // skip the source path of the rename/copy
        }
    }

    statuses
}

/// Indexes absolute status paths by path, recording each status against every
/// directory level between it and `pane_dir` with a severity merge.
///
/// Keying by path rather than by child name is what lets a tree pane, whose
/// rows sit at arbitrary depths, look up a row directly — two `config.rs` in
/// different directories are two keys here, where a name would collide. The
/// ancestor walk is what makes a *collapsed* directory show the worst status
/// beneath it; for a flat listing, whose rows are all direct children, only
/// the last step of that walk is ever read, which is exactly the top-level
/// fold this used to do.
///
/// `git` reports symlink-resolved paths (`rev-parse --show-toplevel` and the
/// status paths built from it), but `pane_dir` is whatever the pane is
/// currently showing, symlinks and all — e.g. macOS puts temp directories
/// under `/var`, itself a symlink to `/private/var`. Stripping against the
/// raw `pane_dir` would then never match, so the comparison walks the
/// resolved side while map keys stay in `pane_dir`'s original form, which is
/// what callers elsewhere still look rows up by.
fn aggregate(pane_dir: &Path, statuses: Vec<(PathBuf, GitStatus)>) -> HashMap<PathBuf, GitStatus> {
    let mut map: HashMap<PathBuf, GitStatus> = HashMap::new();
    let real_pane_dir = pane_dir
        .canonicalize()
        .unwrap_or_else(|_| pane_dir.to_path_buf());

    for (path, status) in statuses {
        let Ok(rel) = path.strip_prefix(&real_pane_dir) else {
            continue; // outside this pane's directory
        };

        // `ancestors` ends at the empty path, which is `pane_dir` itself: a
        // pane never shows a row for its own directory, so stop before it.
        //
        // Ignored is the exception to the propagation: it describes the
        // matched path itself, not a "worst status beneath" worth surfacing.
        // Letting it climb would grey out an ordinary, fully tracked
        // directory just because a `target/` or `node_modules/` sits
        // somewhere underneath it. Everything else still climbs, since those
        // are real work-in-progress a collapsed folder should surface.
        for (depth, level) in rel
            .ancestors()
            .take_while(|p| !p.as_os_str().is_empty())
            .enumerate()
        {
            if depth > 0 && status.kind == GitEntryStatus::Ignored {
                continue;
            }
            map.entry(pane_dir.join(level))
                .and_modify(|s| *s = s.merge(status))
                .or_insert(status);
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/repo")
    }

    /// Drops the porcelain code so assertions stay about the classification.
    fn kinds(statuses: Vec<(PathBuf, GitStatus)>) -> Vec<(PathBuf, GitEntryStatus)> {
        statuses.into_iter().map(|(p, s)| (p, s.kind)).collect()
    }

    fn status(kind: GitEntryStatus) -> GitStatus {
        GitStatus {
            kind,
            code: [' ', ' '],
        }
    }

    /// Builds a throwaway repository, or returns `None` when `git` is not
    /// installed so the suite still runs on a machine without it.
    fn scratch_repo() -> Option<tempfile::TempDir> {
        let dir = tempfile::tempdir().ok()?;
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-C", dir.path().to_str()?])
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
        };

        git(&["init", "--initial-branch=trunk"])?;
        git(&["config", "user.email", "t@example.com"])?;
        git(&["config", "user.name", "t"])?;
        std::fs::write(dir.path().join("tracked.txt"), "hello").ok()?;
        git(&["add", "tracked.txt"])?;
        git(&["commit", "-m", "init"])?;

        Some(dir)
    }

    /// `repo_head` reads the root and the branch from one `rev-parse` call and
    /// relies on their output order, so it is worth checking against real git.
    #[test]
    fn repo_info_reads_branch_and_statuses_from_a_real_repository() {
        let Some(dir) = scratch_repo() else {
            return; // No usable git on this machine.
        };

        std::fs::write(dir.path().join("tracked.txt"), "changed").expect("write");
        std::fs::write(dir.path().join("fresh.txt"), "new").expect("write");

        let info = repo_info(dir.path()).expect("inside a worktree");

        assert_eq!(info.summary.branch, "trunk");
        assert_eq!(info.summary.modified, 1);
        assert_eq!(info.summary.untracked, 1);
        assert_eq!(
            info.entries
                .get(&dir.path().join("tracked.txt"))
                .map(|s| s.kind),
            Some(GitEntryStatus::Modified)
        );
        assert_eq!(
            info.entries
                .get(&dir.path().join("fresh.txt"))
                .map(|s| s.kind),
            Some(GitEntryStatus::Untracked)
        );
    }

    /// `git rev-parse --show-toplevel` resolves symlinks, but a pane can be
    /// showing a symlinked path (macOS routinely does this: `/var`, where
    /// temp directories live, is itself a symlink to `/private/var`). Regression
    /// test for a bug where `aggregate` stripped the resolved git paths
    /// against the raw `pane_dir`, never matched, and silently produced an
    /// empty status map.
    #[test]
    fn repo_info_matches_statuses_when_pane_dir_is_a_symlink() {
        let Some(dir) = scratch_repo() else {
            return; // No usable git on this machine.
        };

        // A nested tempdir holding just the symlink: dropping it unlinks the
        // symlink entry without following it, leaving `dir`'s real files
        // untouched.
        let link_holder = tempfile::tempdir().expect("tempdir");
        let link_path = link_holder.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link_path).expect("symlink");

        std::fs::write(dir.path().join("tracked.txt"), "changed").expect("write");

        let info = repo_info(&link_path).expect("inside a worktree");

        assert_eq!(
            info.entries
                .get(&link_path.join("tracked.txt"))
                .map(|s| s.kind),
            Some(GitEntryStatus::Modified)
        );
    }

    #[test]
    fn repo_info_is_none_outside_a_worktree() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(repo_info(dir.path()).is_none());
    }

    #[test]
    fn summary_counts_tracked_changes_apart_from_untracked() {
        let out = b" M src/main.rs\0?? new.txt\0?? other.txt\0A  staged.rs\0 D gone.rs\0";
        let summary = summarize(&parse_porcelain_z(out, &root()), "feature/x".to_string());

        assert_eq!(summary.branch, "feature/x");
        // Modified + Added + Deleted are all "work in progress".
        assert_eq!(summary.modified, 3);
        assert_eq!(summary.untracked, 2);
    }

    /// The pane listing needs ignored paths (they render muted), but counting
    /// them in the header would report every file under `target/` as work.
    #[test]
    fn summary_ignores_ignored_paths() {
        let out = b"!! target\0!! node_modules\0 M real.rs\0";
        let summary = summarize(&parse_porcelain_z(out, &root()), String::new());

        assert_eq!(summary.modified, 1);
        assert_eq!(summary.untracked, 0);
    }

    #[test]
    fn a_clean_repository_summarises_to_zero() {
        let summary = summarize(&parse_porcelain_z(b"", &root()), "main".to_string());

        assert_eq!(
            summary,
            RepoSummary {
                branch: "main".to_string(),
                modified: 0,
                untracked: 0,
            }
        );
    }

    #[test]
    fn parses_basic_statuses() {
        let out = b" M src/main.rs\0?? new.txt\0!! target\0A  staged.rs\0 D gone.rs\0";
        let statuses = kinds(parse_porcelain_z(out, &root()));

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
        let statuses = kinds(parse_porcelain_z(out, &root()));

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
        let statuses = kinds(parse_porcelain_z(out, &root()));

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
            (
                PathBuf::from("/repo/sub/dir/x.rs"),
                status(GitEntryStatus::Ignored),
            ),
            (
                PathBuf::from("/repo/sub/dir/y.rs"),
                status(GitEntryStatus::Modified),
            ),
            (
                PathBuf::from("/repo/sub/file.rs"),
                status(GitEntryStatus::Untracked),
            ),
            (
                PathBuf::from("/repo/elsewhere.rs"),
                status(GitEntryStatus::Modified),
            ),
        ];

        let map = aggregate(&pane, statuses);

        // Severity merge: Modified (4) beats Ignored (1) inside dir/.
        assert_eq!(
            map.get(Path::new("/repo/sub/dir")).map(|s| s.kind),
            Some(GitEntryStatus::Modified)
        );
        assert_eq!(
            map.get(Path::new("/repo/sub/file.rs")).map(|s| s.kind),
            Some(GitEntryStatus::Untracked)
        );
        // Paths outside the pane are dropped.
        assert!(!map.contains_key(Path::new("/repo/elsewhere.rs")));
        // The pane directory itself never carries a status of its own.
        assert!(!map.contains_key(Path::new("/repo/sub")));
    }

    /// A tree pane shows rows several levels below its root, so each level has
    /// to be addressable in its own right — not just folded into the top-level
    /// name the flat listing happens to show.
    #[test]
    fn aggregate_keeps_every_level_of_a_nested_path() {
        let pane = PathBuf::from("/repo");
        let statuses = vec![(
            PathBuf::from("/repo/a/b/c.rs"),
            status(GitEntryStatus::Modified),
        )];

        let map = aggregate(&pane, statuses);

        for level in ["/repo/a", "/repo/a/b", "/repo/a/b/c.rs"] {
            assert_eq!(
                map.get(Path::new(level)).map(|s| s.kind),
                Some(GitEntryStatus::Modified),
                "{level} should carry the status"
            );
        }
        assert_eq!(map.len(), 3);
    }

    /// The severity merge has to apply at every level, not only the top one:
    /// a collapsed `a/b` must show the worst thing inside it.
    #[test]
    fn aggregate_merges_severity_at_each_level() {
        let pane = PathBuf::from("/repo");
        let statuses = vec![
            (
                PathBuf::from("/repo/a/b/x.rs"),
                status(GitEntryStatus::Ignored),
            ),
            (
                PathBuf::from("/repo/a/b/y.rs"),
                status(GitEntryStatus::Deleted),
            ),
        ];

        let map = aggregate(&pane, statuses);

        assert_eq!(
            map.get(Path::new("/repo/a/b")).map(|s| s.kind),
            Some(GitEntryStatus::Deleted)
        );
        assert_eq!(
            map.get(Path::new("/repo/a")).map(|s| s.kind),
            Some(GitEntryStatus::Deleted)
        );
        // Each leaf keeps its own status, which is what an expanded tree shows.
        assert_eq!(
            map.get(Path::new("/repo/a/b/x.rs")).map(|s| s.kind),
            Some(GitEntryStatus::Ignored)
        );
    }

    /// A directory that merely contains a gitignored child (e.g. `target/`)
    /// must not itself render as ignored/grey: only the ignored path itself
    /// should carry that status. Regression test for a bug where `Ignored`
    /// climbed the ancestor chain like every other status.
    #[test]
    fn aggregate_does_not_mark_ancestors_of_an_ignored_only_child_as_ignored() {
        let pane = PathBuf::from("/repo");
        let statuses = vec![(
            PathBuf::from("/repo/a/b/target"),
            status(GitEntryStatus::Ignored),
        )];

        let map = aggregate(&pane, statuses);

        // The ignored path itself still shows as ignored.
        assert_eq!(
            map.get(Path::new("/repo/a/b/target")).map(|s| s.kind),
            Some(GitEntryStatus::Ignored)
        );
        // Its ancestors are ordinary, tracked directories and carry no
        // status at all.
        assert!(!map.contains_key(Path::new("/repo/a/b")));
        assert!(!map.contains_key(Path::new("/repo/a")));
    }

    /// A real change elsewhere still has to surface through an ancestor even
    /// when an ignored sibling also sits beneath it.
    #[test]
    fn aggregate_still_surfaces_real_changes_past_an_ignored_sibling() {
        let pane = PathBuf::from("/repo");
        let statuses = vec![
            (
                PathBuf::from("/repo/a/target"),
                status(GitEntryStatus::Ignored),
            ),
            (
                PathBuf::from("/repo/a/b/main.rs"),
                status(GitEntryStatus::Modified),
            ),
        ];

        let map = aggregate(&pane, statuses);

        assert_eq!(
            map.get(Path::new("/repo/a")).map(|s| s.kind),
            Some(GitEntryStatus::Modified)
        );
        assert_eq!(
            map.get(Path::new("/repo/a/target")).map(|s| s.kind),
            Some(GitEntryStatus::Ignored)
        );
    }

    #[test]
    fn severity_ordering() {
        assert!(GitEntryStatus::Deleted.severity() > GitEntryStatus::Modified.severity());
        assert!(GitEntryStatus::Modified.severity() > GitEntryStatus::Added.severity());
        assert!(GitEntryStatus::Added.severity() > GitEntryStatus::Untracked.severity());
        assert!(GitEntryStatus::Untracked.severity() > GitEntryStatus::Ignored.severity());
    }
}
