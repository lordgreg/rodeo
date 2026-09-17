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

use super::ops::{ProgressMsg, file_name_of};

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

/// The archive-internal name for `src`, given the directory the selection was
/// listed under. Mirrors `ops::dest_dir_for`'s collision avoidance: a source
/// nested below `base` keeps every directory above it, so two files sharing a
/// name in different subdirectories cannot collide once packed together.
fn archive_name_for(src: &Path, base: &Path) -> PathBuf {
    src.strip_prefix(base)
        .ok()
        .filter(|rel| !rel.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(file_name_of(src)))
}

/// One file or directory to place in the archive, with the name it carries
/// there. Directories are listed explicitly, even when empty — a listing
/// built only from file paths has no way to imply an empty one exists.
struct PackEntry {
    fs_path: PathBuf,
    archive_name: String,
    is_dir: bool,
}

/// Walks every source recursively, producing one [`PackEntry`] per file and
/// directory found (`src` itself included), named relative to `base`.
fn collect_pack_entries(
    sources: &[PathBuf],
    base: &Path,
    cancel: &AtomicBool,
) -> io::Result<Vec<PackEntry>> {
    let mut out = Vec::new();
    for src in sources {
        let name = archive_name_for(src, base);
        walk_into(src, &name, cancel, &mut out)?;
    }
    Ok(out)
}

fn walk_into(
    src: &Path,
    archive_name: &Path,
    cancel: &AtomicBool,
    out: &mut Vec<PackEntry>,
) -> io::Result<()> {
    check_cancel(cancel)?;
    let name = archive_name.to_string_lossy().replace('\\', "/");

    // `symlink_metadata` does not follow the symlink, so a symlink — even
    // one pointing at a directory — is treated as a leaf entry below rather
    // than something to recurse into. That is what makes a symlink cycle (a
    // directory containing a link back to itself or an ancestor) impossible
    // to loop on forever; `Path::is_dir` follows symlinks and would recurse
    // into the cycle without end. Mirrors `ops::entry_size`'s use of the
    // same call for the same reason.
    let meta = std::fs::symlink_metadata(src)?;

    if meta.is_dir() {
        out.push(PackEntry {
            fs_path: src.to_path_buf(),
            archive_name: name,
            is_dir: true,
        });

        // Sorted so the archive's contents — and which chunk of a large
        // selection a cancel lands in — are deterministic between runs.
        let mut children: Vec<_> = std::fs::read_dir(src)?.filter_map(|e| e.ok()).collect();
        children.sort_by_key(|e| e.file_name());
        for entry in children {
            walk_into(
                &entry.path(),
                &archive_name.join(entry.file_name()),
                cancel,
                out,
            )?;
        }
    } else {
        out.push(PackEntry {
            fs_path: src.to_path_buf(),
            archive_name: name,
            is_dir: false,
        });
    }
    Ok(())
}

/// Reads `reader` and writes it to `writer` in `COPY_CHUNK`-sized pieces,
/// reporting progress and checking for cancellation the same way
/// [`write_entry`] does for extraction, just in the other direction.
fn copy_into_archive<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let mut buf = vec![0u8; COPY_CHUNK];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        let _ = tx.send(ProgressMsg::Advance(n as u64));
    }
    Ok(())
}

/// Wraps a file so tar's own copy loop still advances the gauge in
/// `COPY_CHUNK`-sized steps and can be cancelled mid-read.
///
/// Unlike zip — where `start_file` hands back a `Write` to push chunks into
/// directly, mirroring `copy_into_archive` — tar-rs writes an entry's bytes
/// itself once handed a `Read`, so the chunking has to happen from this side
/// of that call instead.
struct ProgressReader<'a, R> {
    inner: R,
    cancel: &'a AtomicBool,
    tx: &'a mpsc::Sender<ProgressMsg>,
}

impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        check_cancel(self.cancel)?;
        let cap = buf.len().min(COPY_CHUNK);
        let n = self.inner.read(&mut buf[..cap])?;
        if n > 0 {
            let _ = self.tx.send(ProgressMsg::Advance(n as u64));
        }
        Ok(n)
    }
}

/// Packs `sources` (files or directories, listed under `base`) into a brand
/// new archive at `dest_path`. Reports progress the same way [`spawn_extract`]
/// does, so the UI's progress gauge needs no archive-specific code.
///
/// On cancellation, or any other error partway through, the partial archive
/// file is removed rather than left behind half-written.
pub fn spawn_create_archive(
    sources: Vec<PathBuf>,
    base: PathBuf,
    dest_path: PathBuf,
    kind: ArchiveKind,
) -> (mpsc::Receiver<ProgressMsg>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = Arc::clone(&cancel);

    thread::spawn(move || {
        let result = create(&sources, &base, &dest_path, kind, &cancel_worker, &tx);
        if result.is_err() {
            let _ = std::fs::remove_file(&dest_path);
        }
        let _ = tx.send(ProgressMsg::Done(result.map_err(|e| e.to_string())));
    });

    (rx, cancel)
}

fn create(
    sources: &[PathBuf],
    base: &Path,
    dest_path: &Path,
    kind: ArchiveKind,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    match kind {
        ArchiveKind::Zip => create_zip(sources, base, dest_path, cancel, tx),
        ArchiveKind::Tar => create_tar(sources, base, dest_path, false, cancel, tx),
        ArchiveKind::TarGz => create_tar(sources, base, dest_path, true, cancel, tx),
    }
}

fn create_zip(
    sources: &[PathBuf],
    base: &Path,
    dest_path: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let entries = collect_pack_entries(sources, base, cancel)?;
    let file = File::create(dest_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in &entries {
        check_cancel(cancel)?;
        if entry.is_dir {
            zip.add_directory(format!("{}/", entry.archive_name), options)
                .map_err(io::Error::other)?;
        } else {
            zip.start_file(entry.archive_name.clone(), options)
                .map_err(io::Error::other)?;
            let reader = File::open(&entry.fs_path)?;
            copy_into_archive(reader, &mut zip, cancel, tx)?;
        }
    }

    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

fn create_tar(
    sources: &[PathBuf],
    base: &Path,
    dest_path: &Path,
    gzipped: bool,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ProgressMsg>,
) -> io::Result<()> {
    let entries = collect_pack_entries(sources, base, cancel)?;
    let file = File::create(dest_path)?;
    let writer: Box<dyn Write> = if gzipped {
        Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        ))
    } else {
        Box::new(file)
    };
    let mut builder = tar::Builder::new(writer);

    for entry in &entries {
        check_cancel(cancel)?;
        if entry.is_dir {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            builder.append_data(&mut header, format!("{}/", entry.archive_name), io::empty())?;
        } else {
            let size = std::fs::metadata(&entry.fs_path)?.len();
            let mut header = tar::Header::new_gnu();
            header.set_size(size);
            header.set_mode(0o644);
            let reader = ProgressReader {
                inner: File::open(&entry.fs_path)?,
                cancel,
                tx,
            };
            builder.append_data(&mut header, &entry.archive_name, reader)?;
        }
    }

    // Finishes the tar layer (trailer blocks) and, for `.tar.gz`, drops the
    // `GzEncoder` beneath it — which is what actually flushes the gzip
    // trailer, the same way the test helper below relies on `Drop` to do it.
    builder.into_inner()?;
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

    mod creation {
        use super::*;

        /// Drains a worker's channel to completion, panicking with its error
        /// message if the operation failed.
        fn settle(rx: &mpsc::Receiver<ProgressMsg>) {
            while let Ok(msg) = rx.recv() {
                if let ProgressMsg::Done(result) = msg {
                    result.unwrap();
                    return;
                }
            }
            panic!("worker channel closed without a Done message");
        }

        #[test]
        fn zip_round_trips_through_list_entries() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("src")).unwrap();
            std::fs::write(dir.path().join("src/main.rs"), b"fn main() {}").unwrap();
            std::fs::write(dir.path().join("top.txt"), b"hi").unwrap();
            let dest = dir.path().join("out.zip");

            let (rx, _cancel) = spawn_create_archive(
                vec![dir.path().join("src"), dir.path().join("top.txt")],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::Zip,
            );
            settle(&rx);

            let entries = list_entries(&dest, ArchiveKind::Zip).unwrap();
            let main = entries.iter().find(|e| e.name == "src/main.rs").unwrap();
            assert!(!main.is_dir);
            assert_eq!(main.size, 12);
            let top = entries.iter().find(|e| e.name == "top.txt").unwrap();
            assert!(!top.is_dir);
            assert_eq!(top.size, 2);
        }

        #[test]
        fn tar_gz_round_trips_through_list_entries() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("src")).unwrap();
            std::fs::write(dir.path().join("src/lib.rs"), b"pub fn f(){}").unwrap();
            let dest = dir.path().join("out.tar.gz");

            let (rx, _cancel) = spawn_create_archive(
                vec![dir.path().join("src")],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::TarGz,
            );
            settle(&rx);

            let entries = list_entries(&dest, ArchiveKind::TarGz).unwrap();
            let lib = entries.iter().find(|e| e.name == "src/lib.rs").unwrap();
            assert!(!lib.is_dir);
            assert_eq!(lib.size, 12);
            let bytes = std::fs::read(&dest).unwrap();
            assert!(
                bytes.starts_with(&[0x1f, 0x8b]),
                "a .tar.gz must actually be gzip-compressed"
            );
        }

        #[test]
        fn cancelling_leaves_no_partial_archive_file() {
            let dir = tempfile::tempdir().unwrap();
            let mut big = Vec::new();
            big.resize(4 * COPY_CHUNK, 7u8);
            std::fs::write(dir.path().join("big.bin"), &big).unwrap();
            let dest = dir.path().join("out.zip");

            let (rx, cancel) = spawn_create_archive(
                vec![dir.path().join("big.bin")],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::Zip,
            );
            cancel.store(true, Ordering::Relaxed);

            let mut done = None;
            while let Ok(msg) = rx.recv() {
                if let ProgressMsg::Done(result) = msg {
                    done = Some(result);
                    break;
                }
            }
            assert!(
                done.unwrap().is_err(),
                "a cancelled pack must report an error"
            );
            assert!(!dest.exists(), "the partial archive must be removed");
        }

        /// Two subdirectories both containing a file named `config.rs` — the
        /// point of naming archive entries relative to `base` rather than by
        /// bare file name.
        #[test]
        fn same_named_files_from_different_subdirectories_do_not_collide() {
            let dir = tempfile::tempdir().unwrap();
            for sub in ["one", "two"] {
                std::fs::create_dir(dir.path().join(sub)).unwrap();
                std::fs::write(dir.path().join(sub).join("config.rs"), sub).unwrap();
            }
            let dest = dir.path().join("out.zip");

            let (rx, _cancel) = spawn_create_archive(
                vec![dir.path().join("one"), dir.path().join("two")],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::Zip,
            );
            settle(&rx);

            let entries = list_entries(&dest, ArchiveKind::Zip).unwrap();
            assert!(entries.iter().any(|e| e.name == "one/config.rs"));
            assert!(entries.iter().any(|e| e.name == "two/config.rs"));
        }

        #[test]
        fn a_directory_only_selection_preserves_empty_directories() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("empty")).unwrap();
            let dest = dir.path().join("out.tar");

            let (rx, _cancel) = spawn_create_archive(
                vec![dir.path().join("empty")],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::Tar,
            );
            settle(&rx);

            let entries = list_entries(&dest, ArchiveKind::Tar).unwrap();
            let empty = entries.iter().find(|e| e.name == "empty").unwrap();
            assert!(empty.is_dir);
        }

        /// Mirrors `ops::total_size_terminates_on_symlink_cycles`: a
        /// selection containing a symlink cycle must not send `walk_into`
        /// into unbounded recursion.
        #[cfg(unix)]
        #[test]
        fn walking_a_symlink_cycle_terminates_and_treats_the_link_as_a_leaf() {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir(&src).unwrap();
            std::fs::File::create(src.join("a.txt")).unwrap();
            // Cycle: link points back at the directory containing it.
            std::os::unix::fs::symlink(&src, src.join("cycle")).unwrap();

            let cancel = AtomicBool::new(false);
            // Must terminate (not recurse forever into the cycle).
            let entries =
                collect_pack_entries(std::slice::from_ref(&src), dir.path(), &cancel).unwrap();

            // Exactly `src` itself, `a.txt` and the symlink — never an
            // unbounded chain of `cycle/cycle/cycle/...`.
            let names: Vec<&str> = entries.iter().map(|e| e.archive_name.as_str()).collect();
            assert_eq!(names.len(), 3, "{names:?}");
            assert!(names.contains(&"src"));
            assert!(names.contains(&"src/a.txt"));
            assert!(names.contains(&"src/cycle"));
            // The symlink was not followed into, so it carries no children.
            assert!(!names.iter().any(|n| n.starts_with("src/cycle/")));
        }

        /// End-to-end: the same symlink cycle fed through the full archive
        /// creation pipeline must terminate rather than hang or crash, and
        /// must not leave a partial archive behind.
        #[cfg(unix)]
        #[test]
        fn creating_an_archive_terminates_on_symlink_cycles() {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir(&src).unwrap();
            std::fs::File::create(src.join("a.txt")).unwrap();
            // Cycle: link points back at the directory containing it.
            std::os::unix::fs::symlink(&src, src.join("cycle")).unwrap();
            let dest = dir.path().join("out.zip");

            let (rx, _cancel) = spawn_create_archive(
                vec![src.clone()],
                dir.path().to_path_buf(),
                dest.clone(),
                ArchiveKind::Zip,
            );

            // Must terminate (not hang or crash on the cycle).
            let mut done = None;
            while let Ok(msg) = rx.recv() {
                if let ProgressMsg::Done(result) = msg {
                    done = Some(result);
                    break;
                }
            }
            let done = done.expect("worker must report completion, not hang");

            // The symlink resolves to a directory, so trying to read it as
            // a file's contents fails — that is an unrelated, well-behaved
            // error, not the crash/hang this test guards against.
            match done {
                Ok(()) => {
                    let entries = list_entries(&dest, ArchiveKind::Zip).unwrap();
                    assert!(entries.len() <= 3, "{entries:?}");
                }
                Err(_) => assert!(!dest.exists(), "no partial archive is left behind"),
            }
        }
    }
}
