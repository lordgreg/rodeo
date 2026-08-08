//! Bookmarked paths, persisted beside the configuration file.
//!
//! Bookmarks live in their own `bookmarks.toml` rather than in `config.toml`
//! for two reasons. They are written by the application every time one is
//! toggled, and `config.toml` is a file the user writes by hand — folding
//! machine-managed state into it means every keypress rewrites the user's
//! settings. And bookmarks are worth far less than a configuration: a
//! malformed bookmark file starts empty with a warning, where a malformed
//! `config.toml` refuses to start.

use std::{
    io,
    path::{Path, PathBuf},
};

use log::{info, warn};
use serde::{Deserialize, Serialize};

pub const BOOKMARKS_FILENAME: &str = "bookmarks.toml";

/// What a bookmarked path turned out to be.
///
/// [`Self::Missing`] and [`Self::Unknown`] are deliberately distinct. `exists()`
/// answers `false` for both "deleted" and "I was not allowed to look" — an
/// unreadable parent directory, an unresponsive network mount, a symlink loop —
/// and pruning on that answer throws away bookmarks that are perfectly fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    Dir,
    File,
    /// Definitely not there any more.
    Missing,
    /// Cannot be determined right now. Never pruned.
    Unknown,
}

impl PathState {
    /// Classifies `path` with at most two `stat` calls.
    pub fn of(path: &Path) -> Self {
        // `symlink_metadata` answers "is there an entry here", which is the
        // question. A symlink whose target is gone is still a real entry that
        // a file manager should show and let you delete — following the link
        // would report it as missing and prune it away.
        match std::fs::symlink_metadata(path) {
            // Only the directory-or-file question follows the link.
            Ok(_) => match std::fs::metadata(path) {
                Ok(md) if md.is_dir() => Self::Dir,
                _ => Self::File,
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(_) => Self::Unknown,
        }
    }

    pub fn is_missing(self) -> bool {
        self == Self::Missing
    }

    pub fn is_dir(self) -> bool {
        self == Self::Dir
    }
}

/// The bookmarked paths, in the order they were added.
///
/// Order is insertion order and stays that way: the popup addresses entries by
/// number (`1`–`9`), so re-sorting would move a bookmark out from under the key
/// the user just learned.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct Bookmarks {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

impl Bookmarks {
    /// Where the bookmarks belonging to `config_path` are kept: the same
    /// directory, so `--config ./rodeo.toml` keeps its bookmarks next to it
    /// instead of in the user's real configuration directory.
    pub fn beside(config_path: &Path) -> PathBuf {
        config_path.with_file_name(BOOKMARKS_FILENAME)
    }

    /// Reads `file`, falling back to an empty list.
    ///
    /// Never fails: a missing file is the normal first-run case, and a corrupt
    /// one must not stop rodeo from starting over a list of paths.
    pub fn load(file: &Path) -> Self {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                warn!(
                    "cannot read {}: {e}; starting with no bookmarks",
                    file.display()
                );
                return Self::default();
            }
        };

        match toml::from_str::<Self>(&text) {
            // The file is documented as hand-editable, so it can contain the
            // same path twice. A duplicate makes `remove` drop one copy while
            // `contains` still answers true, and the bookmark key then reports
            // a removal that did not happen.
            Ok(mut bookmarks) => {
                bookmarks.dedup();
                bookmarks
            }
            Err(e) => {
                warn!(
                    "cannot parse {}: {e}; starting with no bookmarks",
                    file.display()
                );
                Self::default()
            }
        }
    }

    fn dedup(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.paths.retain(|p| seen.insert(p.clone()));
    }

    /// Writes the list to `file`, creating the directory if needed.
    ///
    /// Writes a temporary file and renames it over the target. This runs on
    /// every single change, so a crash or a full disk part-way through a plain
    /// truncating write would leave a half-written file — which `load` then
    /// discards whole, losing every bookmark rather than the last one.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Same directory, so the rename cannot cross a filesystem boundary.
        let temp = file.with_extension("toml.tmp");
        std::fs::write(&temp, text)?;
        if let Err(e) = std::fs::rename(&temp, file) {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }

        info!("bookmarks saved to {}", file.display());
        Ok(())
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| p == path)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Adds `path` if absent, removes it if present. `true` when it was added.
    pub fn toggle(&mut self, path: PathBuf) -> bool {
        match self.paths.iter().position(|p| *p == path) {
            Some(i) => {
                self.paths.remove(i);
                false
            }
            None => {
                self.paths.push(path);
                true
            }
        }
    }

    /// Adds `path` unless it is already bookmarked. `true` when it was added.
    pub fn add(&mut self, path: PathBuf) -> bool {
        if self.contains(&path) {
            return false;
        }
        self.paths.push(path);
        true
    }

    /// Removes `path`. `true` when it was there.
    pub fn remove(&mut self, path: &Path) -> bool {
        match self.paths.iter().position(|p| p == path) {
            Some(i) => {
                self.paths.remove(i);
                true
            }
            None => false,
        }
    }

    /// Removes the bookmark at `index`, if there is one.
    pub fn remove_at(&mut self, index: usize) -> Option<PathBuf> {
        (index < self.paths.len()).then(|| self.paths.remove(index))
    }

    /// Drops every bookmark whose path is definitely gone, returning how many
    /// went.
    ///
    /// A path that merely cannot be read right now ([`PathState::Unknown`]) is
    /// kept: pruning is destructive and "I could not look" is not "it is gone".
    pub fn prune_missing(&mut self) -> usize {
        let before = self.paths.len();
        self.paths.retain(|p| !PathState::of(p).is_missing());
        before - self.paths.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn the_file_sits_beside_the_config_it_belongs_to() {
        assert_eq!(
            Bookmarks::beside(Path::new("/home/u/.config/rodeo/config.toml")),
            PathBuf::from("/home/u/.config/rodeo/bookmarks.toml")
        );
        // An explicit --config keeps its bookmarks next to it, not in the
        // user's real configuration directory.
        assert_eq!(
            Bookmarks::beside(Path::new("./rodeo.toml")),
            PathBuf::from("./bookmarks.toml")
        );
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_list() {
        let dir = temp();
        let bookmarks = Bookmarks::load(&dir.path().join("bookmarks.toml"));
        assert!(bookmarks.is_empty());
    }

    /// A corrupt bookmark file is worth a warning, not a refusal to start.
    #[test]
    fn a_malformed_file_loads_as_an_empty_list_instead_of_failing() {
        let dir = temp();
        let file = dir.path().join("bookmarks.toml");
        std::fs::write(&file, "this is not = = toml").unwrap();

        assert!(Bookmarks::load(&file).is_empty());
    }

    #[test]
    fn bookmarks_survive_a_save_and_load() {
        let dir = temp();
        let file = dir.path().join("bookmarks.toml");

        let mut saved = Bookmarks::default();
        saved.add(PathBuf::from("/etc/hosts"));
        saved.add(PathBuf::from("/home/u/src"));
        saved.save(&file).unwrap();

        assert_eq!(Bookmarks::load(&file), saved);
    }

    /// The parent directory may not exist yet on a first run.
    #[test]
    fn saving_creates_the_directory_it_needs() {
        let dir = temp();
        let file = dir.path().join("nested/deeper/bookmarks.toml");

        Bookmarks::default().save(&file).unwrap();

        assert!(file.exists());
    }

    #[test]
    fn toggling_the_same_path_twice_leaves_no_trace() {
        let mut bookmarks = Bookmarks::default();
        let path = PathBuf::from("/tmp/x");

        assert!(bookmarks.toggle(path.clone()));
        assert!(bookmarks.contains(&path));

        assert!(!bookmarks.toggle(path.clone()));
        assert!(!bookmarks.contains(&path));
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn adding_a_path_twice_keeps_one_bookmark() {
        let mut bookmarks = Bookmarks::default();

        assert!(bookmarks.add(PathBuf::from("/tmp/x")));
        assert!(!bookmarks.add(PathBuf::from("/tmp/x")));
        assert_eq!(bookmarks.len(), 1);
    }

    #[test]
    fn bookmarks_keep_the_order_they_were_added_in() {
        let mut bookmarks = Bookmarks::default();
        for p in ["/c", "/a", "/b"] {
            bookmarks.add(PathBuf::from(p));
        }

        let names: Vec<_> = bookmarks
            .paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(names, ["/c", "/a", "/b"]);
    }

    #[test]
    fn removing_by_index_takes_the_one_asked_for() {
        let mut bookmarks = Bookmarks::default();
        for p in ["/a", "/b", "/c"] {
            bookmarks.add(PathBuf::from(p));
        }

        assert_eq!(bookmarks.remove_at(1), Some(PathBuf::from("/b")));
        assert_eq!(
            bookmarks.paths(),
            [PathBuf::from("/a"), PathBuf::from("/c")]
        );
        // Out of range is a no-op rather than a panic.
        assert_eq!(bookmarks.remove_at(9), None);
    }

    /// The file is documented as hand-editable, so it can name the same path
    /// twice. A duplicate made `remove` drop one copy while `contains` still
    /// answered true, and `b` then reported a removal that had not happened.
    #[test]
    fn a_path_listed_twice_in_the_file_loads_once() {
        let dir = temp();
        let file = dir.path().join("bookmarks.toml");
        std::fs::write(&file, "paths = [\"/a\", \"/b\", \"/a\"]").unwrap();

        let bookmarks = Bookmarks::load(&file);

        assert_eq!(bookmarks.len(), 2);
        assert_eq!(
            bookmarks.paths(),
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    /// The file is rewritten on every change, so a partial write would cost
    /// every bookmark, not the last one.
    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = temp();
        let file = dir.path().join("bookmarks.toml");

        let mut bookmarks = Bookmarks::default();
        bookmarks.add(PathBuf::from("/a"));
        bookmarks.save(&file).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["bookmarks.toml"]);
    }

    /// A symlink whose target is gone is still a real entry: following the
    /// link would call it missing and prune it away.
    #[test]
    fn a_symlink_with_no_target_is_present_not_missing() {
        let dir = temp();
        let link = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("nowhere"), &link).unwrap();

        assert_eq!(PathState::of(&link), PathState::File);

        let mut bookmarks = Bookmarks::default();
        bookmarks.add(link);
        assert_eq!(bookmarks.prune_missing(), 0);
    }

    #[test]
    fn a_directory_a_file_and_a_gap_are_told_apart() {
        let dir = temp();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"").unwrap();

        assert_eq!(PathState::of(dir.path()), PathState::Dir);
        assert_eq!(PathState::of(&file), PathState::File);
        assert_eq!(PathState::of(&dir.path().join("nope")), PathState::Missing);
    }

    /// `exists()` answers `false` for "no permission to look", and pruning on
    /// that throws away bookmarks that are perfectly fine.
    #[test]
    fn a_path_under_an_unreadable_directory_is_unknown_not_missing() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let hidden = locked.join("target");
        std::fs::write(&hidden, b"").unwrap();

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Running as root defeats the permission bits; skip rather than lie.
        let state = PathState::of(&hidden);
        if state != PathState::Dir && state != PathState::File {
            assert_eq!(state, PathState::Unknown, "not gone, just unreadable");

            let mut bookmarks = Bookmarks::default();
            bookmarks.add(hidden);
            assert_eq!(bookmarks.prune_missing(), 0, "unknown must not be pruned");
        }

        // Let the temp dir clean itself up.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn pruning_drops_only_the_paths_that_are_gone() {
        let dir = temp();
        let real = dir.path().join("still-here");
        std::fs::write(&real, b"").unwrap();

        let mut bookmarks = Bookmarks::default();
        bookmarks.add(real.clone());
        bookmarks.add(dir.path().join("long-gone"));
        bookmarks.add(dir.path().to_path_buf());

        assert_eq!(bookmarks.prune_missing(), 1);
        assert!(bookmarks.contains(&real));
        assert_eq!(bookmarks.len(), 2);
    }
}
