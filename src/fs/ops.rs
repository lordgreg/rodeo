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

/// Rejects transfer when the destination is the source itself (would truncate
/// the file on copy) or lies inside the source directory (infinite recursion).
pub fn check_transfer_paths(src: &Path, dest_dir: &Path) -> Result<(), String> {
    let (Ok(src_c), Ok(dest_c)) = (src.canonicalize(), dest_dir.canonicalize()) else {
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

/// Moves a file or directory into `dest_dir`, falling back to copy + delete
/// when `rename` fails (e.g., cross-device EXDEV).
pub fn move_entry(src: &Path, dest_dir: &Path) -> io::Result<()> {
    let dst = dest_dir.join(file_name_of(src));
    if fs::rename(src, &dst).is_ok() {
        return Ok(());
    }
    copy_entry(src, dest_dir)?;
    delete_entry(src)
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
pub fn spawn_transfer(
    sources: Vec<PathBuf>,
    dest_dir: PathBuf,
    cut: bool,
) -> (mpsc::Receiver<ProgressMsg>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    thread::spawn(move || {
        let result = if cut {
            move_with_progress(&sources, &dest_dir, &cancel_worker, &tx)
        } else {
            copy_with_progress(&sources, &dest_dir, &cancel_worker, &tx)
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
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    for src in sources {
        check_cancel(cancel)?;
        copy_entry_progress(src, dest_dir, cancel, tx)?;
    }
    Ok(())
}

fn move_with_progress(
    sources: &[PathBuf],
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    copy_with_progress(sources, dest_dir, cancel, tx)?;
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

    #[test]
    fn spawn_transfer_copies_and_reports_done() {
        let src_root = tempfile::tempdir().unwrap();
        let dst_root = tempfile::tempdir().unwrap();
        let mut f = fs::File::create(src_root.path().join("big.bin")).unwrap();
        f.write_all(&vec![7u8; 1024 * 1024]).unwrap();

        let (rx, _cancel) = spawn_transfer(
            vec![src_root.path().join("big.bin")],
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
            dst_root.path(),
            &cancel,
            &tx,
        );

        assert!(result.is_err());
        assert!(!dst_root.path().join("a.bin").exists());
    }
}
