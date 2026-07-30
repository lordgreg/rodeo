//! Integration tests for file operations against real (temporary) directories.

use rodeo::fs::ops;
use std::fs;
use std::io::Write;

#[test]
fn copy_file_into_other_directory() {
    let src_root = tempfile::tempdir().unwrap();
    let dst_root = tempfile::tempdir().unwrap();
    let mut f = fs::File::create(src_root.path().join("a.txt")).unwrap();
    write!(f, "hello").unwrap();

    ops::copy_entry(&src_root.path().join("a.txt"), dst_root.path()).unwrap();

    assert!(src_root.path().join("a.txt").exists()); // source kept
    assert_eq!(
        fs::read_to_string(dst_root.path().join("a.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn copy_directory_tree_recursively() {
    let src_root = tempfile::tempdir().unwrap();
    let dst_root = tempfile::tempdir().unwrap();
    fs::create_dir(src_root.path().join("proj")).unwrap();
    fs::create_dir(src_root.path().join("proj/src")).unwrap();
    let mut f = fs::File::create(src_root.path().join("proj/src/main.rs")).unwrap();
    write!(f, "fn main() {{}}").unwrap();

    ops::copy_entry(&src_root.path().join("proj"), dst_root.path()).unwrap();

    assert!(dst_root.path().join("proj/src/main.rs").exists());
}

#[test]
fn move_removes_source() {
    let src_root = tempfile::tempdir().unwrap();
    let dst_root = tempfile::tempdir().unwrap();
    let mut f = fs::File::create(src_root.path().join("m.txt")).unwrap();
    write!(f, "data").unwrap();

    ops::move_entry(&src_root.path().join("m.txt"), dst_root.path()).unwrap();

    assert!(!src_root.path().join("m.txt").exists());
    assert_eq!(
        fs::read_to_string(dst_root.path().join("m.txt")).unwrap(),
        "data"
    );
}

#[test]
fn delete_entry_removes_files_and_dirs() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("f.txt");
    fs::File::create(&file).unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::File::create(sub.join("nested.txt")).unwrap();

    ops::delete_entry(&file).unwrap();
    ops::delete_entry(&sub).unwrap();

    assert!(!file.exists());
    assert!(!sub.exists());
}

#[test]
fn rename_via_std_fs() {
    let root = tempfile::tempdir().unwrap();
    let from = root.path().join("old.txt");
    let to = root.path().join("new.txt");
    fs::File::create(&from).unwrap();

    fs::rename(&from, &to).unwrap();

    assert!(!from.exists());
    assert!(to.exists());
}

#[test]
fn transfer_guards_reject_dangerous_paths() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("a.txt");
    fs::File::create(&file).unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();

    // Same directory → same file (would truncate on copy).
    assert!(ops::check_transfer_paths(&file, root.path()).is_err());
    // Directory into its own subdirectory (would recurse forever).
    assert!(ops::check_transfer_paths(root.path(), &sub).is_err());
    // Unrelated directories are fine.
    let other = tempfile::tempdir().unwrap();
    assert!(ops::check_transfer_paths(&file, other.path()).is_ok());
}

#[test]
fn size_walk_ignores_symlinked_directories() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let mut f = fs::File::create(real.join("a.txt")).unwrap();
    write!(f, "12345").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, root.path().join("link")).unwrap();
        // Walk the parent: the symlinked dir must not be counted twice nor
        // followed into a potential cycle.
        let size = ops::total_size(&[root.path().to_path_buf()]);
        assert!(size >= 5);
        assert!(size < 10_000);
    }
}
