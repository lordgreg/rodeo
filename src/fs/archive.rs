//! Read-only access to archive contents, for the archive VFS pane and for
//! extracting selected entries into a real directory.
//!
//! The listing shape here — a flat, `/`-separated name plus an `is_dir` flag,
//! with every implicit ancestor directory synthesised — is deliberately
//! separate from `ui::popup_preview`'s archive listing. Preview only ever
//! renders a flat "name  size" text block for a human to read; this needs a
//! real hierarchy so Enter/back navigation and multi-entry extraction have
//! something to walk. Sharing one function would have forced an awkward
//! shape onto one side or the other, for ~15 lines of overlap.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use super::ops::ProgressMsg;

/// Bytes read per chunk while extracting, matching `ops::copy_file_progress`.
const COPY_CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
}

impl ArchiveKind {
    /// Recognizes an archive by extension — the same rule the preview popup
    /// uses to decide whether to show an archive listing.
    pub fn of(path: &Path) -> Option<Self> {
        let lower = path.to_string_lossy().to_lowercase();
        if lower.ends_with(".zip") {
            Some(Self::Zip)
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if lower.ends_with(".tar") {
            Some(Self::Tar)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    /// Full path from the archive root, `/`-separated, no leading or
    /// trailing slash.
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

impl ArchiveEntry {
    pub fn basename(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }

    fn parent(&self) -> &str {
        match self.name.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        }
    }
}

/// Reads every entry in `path`, synthesising any implicit directory entries
/// the format did not store explicitly (common for zips built without a
/// leading directory member for every prefix).
pub fn list_entries(path: &Path, kind: ArchiveKind) -> io::Result<Vec<ArchiveEntry>> {
    let raw = match kind {
        ArchiveKind::Zip => list_zip(path)?,
        ArchiveKind::Tar => list_tar(path, false)?,
        ArchiveKind::TarGz => list_tar(path, true)?,
    };

    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let mut files: Vec<ArchiveEntry> = Vec::new();

    for (name, is_dir, size) in raw {
        let name = name.trim_matches('/').replace('\\', "/");
        if name.is_empty() {
            continue;
        }

        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            dirs.insert(parent.to_string());
            ancestor = parent;
        }

        if is_dir {
            dirs.insert(name);
        } else {
            files.push(ArchiveEntry {
                name,
                is_dir: false,
                size,
            });
        }
    }

    let mut entries: Vec<ArchiveEntry> = dirs
        .into_iter()
        .map(|name| ArchiveEntry {
            name,
            is_dir: true,
            size: 0,
        })
        .collect();
    entries.extend(files);
    Ok(entries)
}

fn list_zip(path: &Path) -> io::Result<Vec<(String, bool, u64)>> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(io::Error::other)?;
        out.push((entry.name().to_string(), entry.is_dir(), entry.size()));
    }
    Ok(out)
}

fn list_tar(path: &Path, gzipped: bool) -> io::Result<Vec<(String, bool, u64)>> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = if gzipped {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    let mut out = Vec::new();
    for entry in archive.entries()? {
        let entry = entry?;
        let is_dir = entry.header().entry_type().is_dir();
        let name = entry.path()?.to_string_lossy().into_owned();
        out.push((name, is_dir, entry.header().size()?));
    }
    Ok(out)
}

/// Children of `dir` (`""` for the root), one level deep.
pub fn children<'a>(entries: &'a [ArchiveEntry], dir: &str) -> Vec<&'a ArchiveEntry> {
    entries.iter().filter(|e| e.parent() == dir).collect()
}

/// `true` when `name` is one of `names`, or nested under one of them.
fn is_selected(name: &str, names: &BTreeSet<String>) -> bool {
    names
        .iter()
        .any(|n| name == n || name.starts_with(&format!("{n}/")))
}

/// Total uncompressed size of `names` and everything nested under them —
/// what the extraction progress gauge counts up to.
pub fn extract_size(entries: &[ArchiveEntry], names: &BTreeSet<String>) -> u64 {
    entries
        .iter()
        .filter(|e| !e.is_dir && is_selected(&e.name, names))
        .map(|e| e.size)
        .sum()
}

/// The destination path for an archive entry named `name`, if it falls under
/// one of the requested `names`. Mirrors `ops::copy_entry`'s behaviour: the
/// requested item's own basename becomes the top-level component under
/// `dest_dir`, with the rest of its subtree kept intact underneath.
fn dest_for(name: &str, names: &BTreeSet<String>, dest_dir: &Path) -> Option<PathBuf> {
    for n in names {
        let base = n.rsplit('/').next().unwrap_or(n);
        if name == n {
            return Some(dest_dir.join(base));
        }
        if let Some(rest) = name.strip_prefix(&format!("{n}/")) {
            return Some(dest_dir.join(base).join(rest));
        }
    }
    None
}

fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    Ok(())
}

fn write_entry<R: Read>(
    mut reader: R,
    dest: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest)?;
    let mut buf = vec![0u8; COPY_CHUNK];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        let _ = tx.send(ProgressMsg::Advance(n as u64));
    }
    Ok(())
}

/// Extracts `names` (files or directories, given as full archive-root-
/// relative paths) from `path` into `dest_dir`, preserving the structure
/// under each requested directory. Reports progress the same way
/// [`super::ops::spawn_transfer`] does, so the UI's progress gauge needs no
/// archive-specific code.
pub fn spawn_extract(
    path: PathBuf,
    kind: ArchiveKind,
    names: BTreeSet<String>,
    dest_dir: PathBuf,
) -> (mpsc::Receiver<ProgressMsg>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    thread::spawn(move || {
        let result = extract(&path, kind, &names, &dest_dir, &cancel_worker, &tx);
        let _ = tx.send(ProgressMsg::Done(result.map_err(|e| e.to_string())));
    });

    (rx, cancel)
}

fn extract(
    path: &Path,
    kind: ArchiveKind,
    names: &BTreeSet<String>,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    match kind {
        ArchiveKind::Zip => extract_zip(path, names, dest_dir, cancel, tx),
        ArchiveKind::Tar => extract_tar(path, false, names, dest_dir, cancel, tx),
        ArchiveKind::TarGz => extract_tar(path, true, names, dest_dir, cancel, tx),
    }
}

fn extract_zip(
    path: &Path,
    names: &BTreeSet<String>,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    for i in 0..archive.len() {
        check_cancel(cancel)?;
        let entry = archive.by_index(i).map_err(io::Error::other)?;
        let name = entry.name().trim_matches('/').to_string();
        let Some(dest) = dest_for(&name, names, dest_dir) else {
            continue;
        };
        if entry.is_dir() {
            std::fs::create_dir_all(dest)?;
        } else {
            write_entry(entry, &dest, cancel, tx)?;
        }
    }
    Ok(())
}

fn extract_tar(
    path: &Path,
    gzipped: bool,
    names: &BTreeSet<String>,
    dest_dir: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = if gzipped {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        check_cancel(cancel)?;
        let mut entry = entry?;
        let is_dir = entry.header().entry_type().is_dir();
        let name = entry
            .path()?
            .to_string_lossy()
            .trim_matches('/')
            .to_string();
        let Some(dest) = dest_for(&name, names, dest_dir) else {
            continue;
        };
        if is_dir {
            std::fs::create_dir_all(dest)?;
        } else {
            write_entry(&mut entry, &dest, cancel, tx)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write_tar(path: &Path, gzip: bool, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let writer: Box<dyn Write> = if gzip {
            Box::new(flate2::write::GzEncoder::new(
                file,
                flate2::Compression::default(),
            ))
        } else {
            Box::new(file)
        };
        let mut builder = tar::Builder::new(writer);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *content).unwrap();
        }
        builder.into_inner().unwrap();
    }

    mod kind_of {
        use super::*;

        #[test]
        fn recognizes_every_supported_extension() {
            assert_eq!(ArchiveKind::of(Path::new("a.zip")), Some(ArchiveKind::Zip));
            assert_eq!(ArchiveKind::of(Path::new("a.tar")), Some(ArchiveKind::Tar));
            assert_eq!(
                ArchiveKind::of(Path::new("a.tar.gz")),
                Some(ArchiveKind::TarGz)
            );
            assert_eq!(
                ArchiveKind::of(Path::new("a.tgz")),
                Some(ArchiveKind::TarGz)
            );
            assert_eq!(ArchiveKind::of(Path::new("a.txt")), None);
        }
    }

    mod listing {
        use super::*;

        #[test]
        fn zip_without_explicit_dirs_gets_implicit_ancestors() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.zip");
            write_zip(&path, &[("src/main.rs", b"fn main() {}")]);

            let entries = list_entries(&path, ArchiveKind::Zip).unwrap();

            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"src"));
            assert!(names.contains(&"src/main.rs"));
            let src = entries.iter().find(|e| e.name == "src").unwrap();
            assert!(src.is_dir);
            let main = entries.iter().find(|e| e.name == "src/main.rs").unwrap();
            assert!(!main.is_dir);
            assert_eq!(main.size, 12);
        }

        #[test]
        fn tar_without_explicit_dirs_gets_implicit_ancestors() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.tar");
            write_tar(&path, false, &[("sub/dir/file.txt", b"hello")]);

            let entries = list_entries(&path, ArchiveKind::Tar).unwrap();

            assert!(entries.iter().any(|e| e.name == "sub" && e.is_dir));
            assert!(entries.iter().any(|e| e.name == "sub/dir" && e.is_dir));
            assert!(
                entries
                    .iter()
                    .any(|e| e.name == "sub/dir/file.txt" && !e.is_dir)
            );
        }

        #[test]
        fn tar_gz_round_trips() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.tar.gz");
            write_tar(&path, true, &[("a.txt", b"hi")]);

            let entries = list_entries(&path, ArchiveKind::TarGz).unwrap();

            assert!(entries.iter().any(|e| e.name == "a.txt" && !e.is_dir));
        }

        #[test]
        fn children_of_root_are_one_level_deep() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.zip");
            write_zip(
                &path,
                &[("top.txt", b"x"), ("src/main.rs", b"fn main() {}")],
            );
            let entries = list_entries(&path, ArchiveKind::Zip).unwrap();

            let root: Vec<&str> = children(&entries, "")
                .iter()
                .map(|e| e.name.as_str())
                .collect();
            assert!(root.contains(&"top.txt"));
            assert!(root.contains(&"src"));
            assert!(!root.contains(&"src/main.rs"));

            let inside: Vec<&str> = children(&entries, "src")
                .iter()
                .map(|e| e.name.as_str())
                .collect();
            assert_eq!(inside, vec!["src/main.rs"]);
        }
    }

    mod extraction {
        use super::*;

        #[test]
        fn extracts_a_single_selected_file() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.zip");
            write_zip(&path, &[("src/main.rs", b"fn main() {}")]);
            let dest = dir.path().join("dest");
            std::fs::create_dir(&dest).unwrap();

            let mut names = BTreeSet::new();
            names.insert("src/main.rs".to_string());
            let (rx, _cancel) = spawn_extract(path, ArchiveKind::Zip, names, dest.clone());

            let mut done = false;
            while let Ok(msg) = rx.recv() {
                if let ProgressMsg::Done(result) = msg {
                    result.unwrap();
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(
                std::fs::read_to_string(dest.join("main.rs")).unwrap(),
                "fn main() {}"
            );
        }

        #[test]
        fn extracts_a_selected_directory_recursively() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("a.tar");
            write_tar(
                &path,
                false,
                &[
                    ("src/main.rs", b"fn main() {}"),
                    ("src/lib.rs", b"pub fn f(){}"),
                ],
            );
            let dest = dir.path().join("dest");
            std::fs::create_dir(&dest).unwrap();

            let mut names = BTreeSet::new();
            names.insert("src".to_string());
            let (rx, _cancel) = spawn_extract(path, ArchiveKind::Tar, names, dest.clone());

            while let Ok(msg) = rx.recv() {
                if let ProgressMsg::Done(result) = msg {
                    result.unwrap();
                    break;
                }
            }
            assert_eq!(
                std::fs::read_to_string(dest.join("src").join("main.rs")).unwrap(),
                "fn main() {}"
            );
            assert_eq!(
                std::fs::read_to_string(dest.join("src").join("lib.rs")).unwrap(),
                "pub fn f(){}"
            );
        }

        #[test]
        fn extract_size_counts_only_matched_files() {
            let entries = vec![
                ArchiveEntry {
                    name: "src".to_string(),
                    is_dir: true,
                    size: 0,
                },
                ArchiveEntry {
                    name: "src/a.rs".to_string(),
                    is_dir: false,
                    size: 10,
                },
                ArchiveEntry {
                    name: "other.txt".to_string(),
                    is_dir: false,
                    size: 5,
                },
            ];
            let mut names = BTreeSet::new();
            names.insert("src".to_string());

            assert_eq!(extract_size(&entries, &names), 10);
        }
    }
}
