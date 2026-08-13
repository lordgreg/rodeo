//! Copy, move, delete and size calculations.
//!
//! Everything here is UI-agnostic so it can be tested directly (see
//! `tests/file_ops.rs`). Large transfers run on a worker thread and report
//! progress over an `mpsc` channel rather than blocking the event loop.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The directory `src` lands in when a transfer preserves the layout below
/// `base` — the pane's own directory.
///
/// In a flat listing every source is a direct child of `base`, so the relative
/// path is a bare file name and this is always `dest_dir` itself. A tree pane
/// can hand up rows from any depth, where flattening them all into `dest_dir`
/// would let `src/config.rs` and `tests/config.rs` land on the same name and
/// silently overwrite each other.
pub fn dest_dir_for(src: &Path, base: &Path, dest_dir: &Path) -> PathBuf {
    src.strip_prefix(base)
        .ok()
        .and_then(|rel| rel.parent())
        .filter(|rel_parent| !rel_parent.as_os_str().is_empty())
        .map(|rel_parent| dest_dir.join(rel_parent))
        .unwrap_or_else(|| dest_dir.to_path_buf())
}

/// [`dest_dir_for`], creating the directory when the layout calls for one that
/// is not there yet.
pub fn prepare_dest_dir(src: &Path, base: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let target = dest_dir_for(src, base, dest_dir);
    if target != dest_dir {
        fs::create_dir_all(&target)?;
    }

    Ok(target)
}

/// Canonicalizes `path`, or — when it does not exist yet — its nearest
/// existing ancestor with the missing tail put back on.
///
/// A structure-preserving transfer aims at directories that have still to be
/// created, and refusing to check those would drop the guards below exactly
/// where they are needed.
fn canonicalize_lenient(path: &Path) -> Option<PathBuf> {
    if let Ok(resolved) = path.canonicalize() {
        return Some(resolved);
    }

    let parent = path.parent()?;
    let name = path.file_name()?;

    Some(canonicalize_lenient(parent)?.join(name))
}

/// Rejects transfer when the destination is the source itself (would truncate
/// the file on copy) or lies inside the source directory (infinite recursion).
pub fn check_transfer_paths(src: &Path, dest_dir: &Path) -> Result<(), String> {
    let (Ok(src_c), Some(dest_c)) = (src.canonicalize(), canonicalize_lenient(dest_dir)) else {
        return Ok(()); // cannot verify — let the fs operation surface any error
    };

    if src_c.parent() == Some(dest_c.as_path()) {
        return Err("Source and destination are the same.".to_string());
    }

    if src.is_dir() && dest_c.starts_with(&src_c) {
        return Err("Cannot copy a directory into itself.".to_string());
    }

    Ok(())
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies a file or directory (recursively) into `dest_dir`.
pub fn copy_entry(src: &Path, dest_dir: &Path) -> io::Result<()> {
    let dst = dest_dir.join(file_name_of(src));
    if src.is_dir() {
        copy_dir_recursive(src, &dst)
    } else {
        fs::copy(src, &dst).map(|_| ())
    }
}

/// Deletes a file or directory (recursively). Permanent — no trash.
pub fn delete_entry(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Whether a failed `rename` is one that copy + delete can still get past.
///
/// Two cases, and only two — anything else (no permission, source gone, read
/// only filesystem) is a real error that a copy would only hit again, more
/// slowly, after having written half a tree:
///
/// * `CrossesDevices` (EXDEV) — the destination is on another filesystem, so
///   the bytes genuinely have to be moved.
/// * The destination is already taken. `rename` replaces a file, and an
///   *empty* directory, but refuses a non-empty one (ENOTEMPTY/EEXIST) — and
///   merging into it is exactly what the overwrite prompt promised.
fn rename_needs_fallback(err: &io::Error, dst: &Path) -> bool {
    err.kind() == io::ErrorKind::CrossesDevices || fs::symlink_metadata(dst).is_ok()
}

/// Attempts to move `src` into `dest_dir` with `rename` alone — one syscall,
/// no matter how large the tree is.
///
/// `Ok(false)` means `rename` cannot do it and a copy + delete has to; an
/// `Err` is a failure worth reporting rather than working around.
fn try_rename_into(src: &Path, dest_dir: &Path) -> io::Result<bool> {
    let dst = dest_dir.join(file_name_of(src));
    match fs::rename(src, &dst) {
        Ok(()) => Ok(true),
        Err(e) if rename_needs_fallback(&e, &dst) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Moves a file or directory into `dest_dir`, falling back to copy + delete
/// only where [`rename_needs_fallback`] says one is needed.
pub fn move_entry(src: &Path, dest_dir: &Path) -> io::Result<()> {
    if try_rename_into(src, dest_dir)? {
        return Ok(());
    }
    copy_entry(src, dest_dir)?;
    delete_entry(src)
}

/// Moves every source `rename` can take on its own, returning those left over
/// for a real copy.
///
/// This runs before a transfer sizes itself up, because within one filesystem
/// it is the whole operation: a 32 GiB tree of 100k files moves in one syscall
/// per source, and never reaches [`total_size`]'s recursive walk — let alone
/// the byte-for-byte copy loop. Only what comes back needs either.
///
/// An `Err` can leave earlier sources already moved, the same as the transfer
/// loops themselves; the caller reloads the panes to show what happened.
pub fn rename_movable(
    sources: &[PathBuf],
    base: &Path,
    dest_dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut remaining = Vec::new();

    for src in sources {
        let target = prepare_dest_dir(src, base, dest_dir)?;
        if !try_rename_into(src, &target)? {
            remaining.push(src.clone());
        }
    }

    Ok(remaining)
}

/// Sets `path`'s Unix permission bits (e.g. `0o755`). Pure `std`: follows a
/// symlink to its target, the same as a plain `chmod path` in a shell (there
/// is no `lchmod` on Linux, so a symlink's own mode cannot be changed there
/// regardless).
#[cfg(unix)]
pub fn chmod_entry(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
pub fn chmod_entry(_path: &Path, _mode: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "changing permissions is not supported on this platform",
    ))
}

/// Changes `path`'s owning user and/or group. `None` leaves that half
/// unchanged. Follows a symlink to its target, matching plain `chown path`.
///
/// `libc` is already in the dependency tree (`ui::header::free_space` uses
/// it for `statvfs`) — `std` has no owner-change API at all, so this is the
/// only way to chown without shelling out.
#[cfg(unix)]
pub fn chown_entry(path: &Path, uid: Option<u32>, gid: Option<u32>) -> io::Result<()> {
    use std::ffi::CString;

    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // `-1` (as uid_t/gid_t) tells chown(2) to leave that id unchanged.
    let uid = uid.map(|u| u as libc::uid_t).unwrap_or(u32::MAX);
    let gid = gid.map(|g| g as libc::gid_t).unwrap_or(u32::MAX);

    // SAFETY: c_path is a valid NUL-terminated string for the duration of the
    // call, and the return code is checked before treating the call as having
    // succeeded.
    if unsafe { libc::chown(c_path.as_ptr(), uid, gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn chown_entry(_path: &Path, _uid: Option<u32>, _gid: Option<u32>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "changing ownership is not supported on this platform",
    ))
}

/// Creates a symlink at `link` pointing at `target`. `target` is used
/// verbatim (relative or absolute) — same as `ln -s`.
#[cfg(unix)]
pub fn create_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
pub fn create_symlink(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlinks are not supported on this platform",
    ))
}

/// Total size in bytes of all paths (directories walked recursively).
/// Symlinks are *not* followed (matches `du` behavior and, crucially,
/// makes symlink cycles impossible). Unreadable entries count as zero.
pub fn total_size(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|p| entry_size(p)).sum()
}

/// Result of a capped size walk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeEstimate {
    pub bytes: u64,
    /// Filesystem entries visited during the walk.
    pub entries: u64,
    /// True when the walk hit the entry cap — the value is a lower bound.
    pub truncated: bool,
}

/// Like `total_size`, but stops after `max_entries` filesystem entries and
/// reports truncation. Use for UI display (directory preview) where a huge
/// tree must not block; transfers need the exact `total_size` instead.
pub fn total_size_capped(paths: &[PathBuf], max_entries: u64) -> SizeEstimate {
    let mut bytes = 0u64;
    let mut entries = 0u64;
    let mut truncated = false;
    for path in paths {
        entry_size_capped(path, &mut bytes, &mut entries, max_entries, &mut truncated);
    }
    SizeEstimate {
        bytes,
        entries,
        truncated,
    }
}

/// Progress reports from a background transfer worker.
#[derive(Debug)]
pub enum ProgressMsg {
    /// Bytes copied since the last report.
    Advance(u64),
    /// Transfer finished (Ok) or failed (Err with context).
    Done(Result<(), String>),
}

/// Spawns a background copy (cut=false) or move (cut=true) transfer.
/// Returns the progress channel and a cancellation flag: set it to `true` to
/// abort (the partially written file is removed).
///
/// `base` is the directory the sources are listed under; their layout below it
/// is recreated under `dest_dir`. See [`dest_dir_for`].
pub fn spawn_transfer(
    sources: Vec<PathBuf>,
    base: PathBuf,
    dest_dir: PathBuf,
    cut: bool,
) -> (mpsc::Receiver<ProgressMsg>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    thread::spawn(move || {
        let result = if cut {
            move_with_progress(&sources, &base, &dest_dir, &cancel_worker, &tx)
        } else {
            copy_with_progress(&sources, &base, &dest_dir, &cancel_worker, &tx)
        };
        let _ = tx.send(ProgressMsg::Done(result.map_err(|e| e.to_string())));
    });

    (rx, cancel)
}

fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    Ok(())
}

fn copy_with_progress(
    sources: &[PathBuf],
    base: &Path,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    for src in sources {
        check_cancel(cancel)?;
        let target = prepare_dest_dir(src, base, dest_dir)?;
        copy_entry_progress(src, &target, cancel, tx)?;
    }
    Ok(())
}

fn move_with_progress(
    sources: &[PathBuf],
    base: &Path,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    copy_with_progress(sources, base, dest_dir, cancel, tx)?;
    check_cancel(cancel)?;
    for src in sources {
        delete_entry(src)?;
    }
    Ok(())
}

fn copy_entry_progress(
    src: &Path,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let dst = dest_dir.join(file_name_of(src));
    if src.is_dir() {
        fs::create_dir_all(&dst)?;
        for entry in fs::read_dir(src)? {
            check_cancel(cancel)?;
            copy_entry_progress(&entry?.path(), &dst, cancel, tx)?;
        }
        Ok(())
    } else {
        copy_file_progress(src, &dst, cancel, tx)
    }
}

fn copy_file_progress(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let mut reader = fs::File::open(src)?;
    let mut writer = fs::File::create(dst)?;
    let mut buf = vec![0u8; 256 * 1024];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = fs::remove_file(dst); // remove the partial file
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        writer.write_all(&buf[..n])?;
        let _ = tx.send(ProgressMsg::Advance(n as u64));
    }
    Ok(())
}

fn entry_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| entry_size(&e.path()))
                    .sum()
            })
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

fn entry_size_capped(
    path: &Path,
    bytes: &mut u64,
    entries: &mut u64,
    max_entries: u64,
    truncated: &mut bool,
) {
    if *entries >= max_entries {
        *truncated = true;
        return;
    }
    *entries += 1;

    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };

    if meta.is_dir() {
        if let Ok(rd) = fs::read_dir(path) {
            for entry in rd.filter_map(|e| e.ok()) {
                entry_size_capped(&entry.path(), bytes, entries, max_entries, truncated);
                if *truncated {
                    return;
                }
            }
        }
    } else {
        *bytes += meta.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(unix)]
    mod permissions_and_ownership {
        use super::*;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        #[test]
        fn chmod_sets_the_requested_mode() {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("a.txt");
            fs::File::create(&file).unwrap();

            chmod_entry(&file, 0o640).unwrap();

            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }

        #[test]
        fn chmod_follows_a_symlink_to_its_target() {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("a.txt");
            let link = dir.path().join("link");
            fs::File::create(&file).unwrap();
            std::os::unix::fs::symlink(&file, &link).unwrap();

            chmod_entry(&link, 0o600).unwrap();

            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        #[test]
        fn chown_leaves_the_group_alone_when_not_given() {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("a.txt");
            fs::File::create(&file).unwrap();
            let before = fs::metadata(&file).unwrap().gid();

            // Owning uid is the current user either way — this only proves
            // the call succeeds and the gid this process cannot touch stays
            // put, without requiring root.
            let uid = fs::metadata(&file).unwrap().uid();
            chown_entry(&file, Some(uid), None).unwrap();

            assert_eq!(fs::metadata(&file).unwrap().gid(), before);
        }

        #[test]
        fn chown_rejects_a_path_that_does_not_exist() {
            let dir = tempfile::tempdir().unwrap();
            let missing = dir.path().join("nope.txt");

            assert!(chown_entry(&missing, Some(0), None).is_err());
        }
    }

    #[cfg(unix)]
    mod symlink_creation {
        use super::*;

        #[test]
        fn create_symlink_points_at_the_target() {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("a.txt");
            let link = dir.path().join("link");
            fs::File::create(&target).unwrap();

            create_symlink(&target, &link).unwrap();

            assert_eq!(fs::read_link(&link).unwrap(), target);
        }

        #[test]
        fn create_symlink_fails_when_the_link_name_is_taken() {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("a.txt");
            let link = dir.path().join("link");
            fs::File::create(&target).unwrap();
            fs::File::create(&link).unwrap();

            assert!(create_symlink(&target, &link).is_err());
        }
    }

    #[test]
    fn copy_dir_recursive_copies_nested_tree() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();

        fs::create_dir(src_dir.path().join("sub")).unwrap();
        let mut f = fs::File::create(src_dir.path().join("top.txt")).unwrap();
        write!(f, "top").unwrap();
        let mut f = fs::File::create(src_dir.path().join("sub").join("nested.txt")).unwrap();
        write!(f, "nested").unwrap();

        let dst = dst_root.path().join("copy");
        copy_dir_recursive(src_dir.path(), &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("nested.txt")).unwrap(),
            "nested"
        );
    }

    #[test]
    fn check_transfer_rejects_same_source_and_dest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::File::create(&file).unwrap();

        assert!(check_transfer_paths(&file, dir.path()).is_err());
    }

    #[test]
    fn check_transfer_rejects_dest_inside_source() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();

        assert!(check_transfer_paths(dir.path(), &sub).is_err());
    }

    #[test]
    fn check_transfer_allows_normal_transfer() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let file = src_root.path().join("a.txt");
        fs::File::create(&file).unwrap();

        assert!(check_transfer_paths(&file, dst_root.path()).is_ok());
    }

    #[test]
    fn move_entry_moves_file_to_dest_dir() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let file = src_root.path().join("a.txt");
        fs::File::create(&file).unwrap();

        move_entry(&file, dst_root.path()).unwrap();

        assert!(!file.exists());
        assert!(dst_root.path().join("a.txt").exists());
    }

    /// The inode survives a `rename` and cannot survive a copy, so this is the
    /// one assertion that tells the two apart — and the whole point of the
    /// fast path, since the alternative reads and rewrites every byte below.
    #[cfg(unix)]
    #[test]
    fn move_entry_renames_a_directory_instead_of_copying_it() {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src");
        let dst_root = root.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst_root).unwrap();
        fs::File::create(src.join("a.txt")).unwrap();
        let before = fs::metadata(&src).unwrap().ino();

        move_entry(&src, &dst_root).unwrap();

        let after = fs::metadata(dst_root.join("src")).unwrap().ino();
        assert_eq!(before, after, "directory was copied, not renamed");
    }

    /// `rename` refuses a non-empty destination directory, and merging into it
    /// is what the overwrite prompt promised — so that error still falls back.
    #[test]
    fn move_entry_merges_into_an_existing_directory() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("src/d");
        let dst_root = root.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dst_root.join("d")).unwrap();
        fs::File::create(src.join("new.txt")).unwrap();
        fs::File::create(dst_root.join("d/old.txt")).unwrap();

        move_entry(&src, &dst_root).unwrap();

        assert!(!src.exists());
        assert!(dst_root.join("d/new.txt").exists());
        assert!(dst_root.join("d/old.txt").exists());
    }

    /// Anything else is reported as-is rather than retried as a copy, which
    /// would only fail again after writing half a tree.
    #[test]
    fn move_entry_reports_a_missing_source() {
        let root = tempfile::tempdir().unwrap();
        let dst_root = root.path().join("dst");
        fs::create_dir(&dst_root).unwrap();

        let err = move_entry(&root.path().join("gone.txt"), &dst_root).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn rename_movable_takes_every_source_within_one_filesystem() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let dst_root = root.path().join("dst");
        fs::create_dir_all(base.join("d")).unwrap();
        fs::create_dir(&dst_root).unwrap();
        fs::File::create(base.join("a.txt")).unwrap();
        let sources = vec![base.join("a.txt"), base.join("d")];

        let remaining = rename_movable(&sources, &base, &dst_root).unwrap();

        assert!(remaining.is_empty(), "nothing should be left to copy");
        assert!(dst_root.join("a.txt").exists());
        assert!(dst_root.join("d").exists());
    }

    #[test]
    fn rename_movable_hands_back_a_directory_that_needs_merging() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let dst_root = root.path().join("dst");
        fs::create_dir_all(base.join("d")).unwrap();
        fs::create_dir_all(dst_root.join("d")).unwrap();
        fs::File::create(base.join("d/new.txt")).unwrap();
        fs::File::create(dst_root.join("d/old.txt")).unwrap();
        let sources = vec![base.join("a.txt"), base.join("d")];
        fs::File::create(&sources[0]).unwrap();

        let remaining = rename_movable(&sources, &base, &dst_root).unwrap();

        assert_eq!(remaining, vec![base.join("d")]);
        assert!(dst_root.join("a.txt").exists(), "the file still moved");
    }

    #[test]
    fn rename_movable_keeps_the_layout_below_the_base() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let dst_root = root.path().join("dst");
        fs::create_dir_all(base.join("a/b")).unwrap();
        fs::create_dir(&dst_root).unwrap();
        let src = base.join("a/b/deep.txt");
        fs::File::create(&src).unwrap();

        let remaining = rename_movable(std::slice::from_ref(&src), &base, &dst_root).unwrap();

        assert!(remaining.is_empty());
        assert!(!src.exists());
        assert!(dst_root.join("a/b/deep.txt").exists());
    }

    #[test]
    fn delete_entry_removes_directory_tree() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::File::create(sub.join("f.txt")).unwrap();

        delete_entry(&sub).unwrap();
        assert!(!sub.exists());
    }

    #[test]
    fn total_size_sums_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fs::File::create(dir.path().join("a.txt")).unwrap();
        write!(f, "12345").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let mut f = fs::File::create(sub.join("b.txt")).unwrap();
        write!(f, "123").unwrap();

        let size = total_size(&[dir.path().join("a.txt"), sub]);
        assert_eq!(size, 8);
    }

    #[cfg(unix)]
    #[test]
    fn total_size_terminates_on_symlink_cycles() {
        let dir = tempfile::tempdir().unwrap();
        fs::File::create(dir.path().join("a.txt")).unwrap();
        // Cycle: link points back at the directory containing it.
        std::os::unix::fs::symlink(dir.path(), dir.path().join("cycle")).unwrap();

        // Must terminate (not hang on the cycle).
        let size = total_size(&[dir.path().to_path_buf()]);
        assert!(size > 0);
    }

    #[test]
    fn total_size_capped_reports_truncation() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            fs::File::create(dir.path().join(format!("f{i}.txt"))).unwrap();
        }

        let full = total_size_capped(&[dir.path().to_path_buf()], 100);
        assert!(!full.truncated);

        let capped = total_size_capped(&[dir.path().to_path_buf()], 5);
        assert!(capped.truncated);
        assert!(capped.bytes <= full.bytes);
    }

    /// A flat listing only ever yields direct children, so preserving the
    /// layout must be a no-op there — this is what keeps the old behaviour.
    #[test]
    fn a_direct_child_lands_straight_in_the_destination() {
        let dest = Path::new("/dest");

        assert_eq!(
            dest_dir_for(Path::new("/base/a.txt"), Path::new("/base"), dest),
            dest
        );
    }

    #[test]
    fn a_nested_source_keeps_the_directories_above_it() {
        assert_eq!(
            dest_dir_for(
                Path::new("/base/a/b/c.txt"),
                Path::new("/base"),
                Path::new("/dest")
            ),
            Path::new("/dest/a/b")
        );
    }

    /// Nothing sensible to preserve, so fall back to the destination itself
    /// rather than inventing a path.
    #[test]
    fn a_source_outside_the_base_lands_in_the_destination() {
        assert_eq!(
            dest_dir_for(
                Path::new("/elsewhere/a.txt"),
                Path::new("/base"),
                Path::new("/dest")
            ),
            Path::new("/dest")
        );
    }

    /// Two files sharing a name, as a tree pane can easily offer, must not
    /// collide in the destination.
    #[test]
    fn same_named_files_from_different_directories_do_not_overwrite() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        for sub in ["one", "two"] {
            fs::create_dir(src.path().join(sub)).unwrap();
            fs::write(src.path().join(sub).join("config.rs"), sub).unwrap();
        }

        for sub in ["one", "two"] {
            let file = src.path().join(sub).join("config.rs");
            let target = prepare_dest_dir(&file, src.path(), dst.path()).unwrap();
            copy_entry(&file, &target).unwrap();
        }

        assert_eq!(
            fs::read_to_string(dst.path().join("one/config.rs")).unwrap(),
            "one"
        );
        assert_eq!(
            fs::read_to_string(dst.path().join("two/config.rs")).unwrap(),
            "two"
        );
    }

    /// With the layout preserved, a nested source copied into a pane rooted at
    /// its own ancestor lands back on itself — `fs::copy` would truncate it.
    #[test]
    fn a_transfer_onto_itself_is_refused_even_before_the_directory_exists() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        let file = root.path().join("a/b.txt");
        fs::write(&file, "keep").unwrap();

        let target = dest_dir_for(&file, root.path(), root.path());

        assert_eq!(target, root.path().join("a"));
        assert!(check_transfer_paths(&file, &target).is_err());
    }

    #[test]
    fn spawn_transfer_copies_and_reports_done() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let mut f = fs::File::create(src_root.path().join("big.bin")).unwrap();
        f.write_all(&vec![7u8; 1024 * 1024]).unwrap();

        let (rx, _cancel) = spawn_transfer(
            vec![src_root.path().join("big.bin")],
            src_root.path().to_path_buf(),
            dst_root.path().to_path_buf(),
            false,
        );

        let mut advanced = 0u64;
        let mut done = None;
        for msg in rx {
            match msg {
                ProgressMsg::Advance(n) => advanced += n,
                ProgressMsg::Done(result) => {
                    done = Some(result);
                    break;
                }
            }
        }

        assert!(done.unwrap().is_ok());
        assert_eq!(advanced, 1024 * 1024);
        assert_eq!(
            fs::read(dst_root.path().join("big.bin")).unwrap().len(),
            1024 * 1024
        );
    }

    #[test]
    fn copy_with_progress_aborts_when_cancelled() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let mut f = fs::File::create(src_root.path().join("a.bin")).unwrap();
        f.write_all(&vec![1u8; 4096]).unwrap();

        let cancel = AtomicBool::new(true); // pre-cancelled
        let (tx, _rx) = mpsc::channel();

        let result = copy_with_progress(
            &[src_root.path().join("a.bin")],
            src_root.path(),
            dst_root.path(),
            &cancel,
            &tx,
        );

        assert!(result.is_err());
        assert!(!dst_root.path().join("a.bin").exists());
    }
}
