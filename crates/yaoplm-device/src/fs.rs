//! The file system of a device root, as everything else in YAOPLM sees it.
//!
//! Paths are [`RelPath`]s: `/`-separated, relative to the root, with no
//! `.`, `..`, drive or stream syntax, so no operation can reach outside the
//! root (CLAUDE.md rule 1). [`HostFs`] maps them onto a real folder and also
//! refuses to follow a symlink out of it. Tests use in-memory and FAT32
//! image implementations from `yaoplm-test-support`.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use yaoplm_i18n::Phrase;

use crate::mode::DeviceMode;
use crate::source::ReadSeek;
use crate::volume::{FsKind, VolumeKind, volume_for_path};

/// Suffix of a file still being written. OPL lists neither `*.iso.yaoplm-part`
/// nor `ul.*.yaoplm-part`, so a half-written game never shows up.
pub const PART_SUFFIX: &str = ".yaoplm-part";
/// Suffix of the temporary file behind an atomic small-file write.
pub const TMP_SUFFIX: &str = ".yaoplm-tmp";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("path {0:?} has an empty, \".\" or \"..\" component")]
    BadComponent(String),
    #[error("path {0:?} contains a character that is not allowed in a device path")]
    BadCharacter(String),
}

impl PathError {
    /// What front ends show (ADR-0029).
    pub fn phrase(&self) -> Phrase {
        match self {
            Self::BadComponent(path) => Phrase::error("badPathPart").with("path", path),
            Self::BadCharacter(path) => Phrase::error("badPathCharacter").with("path", path),
        }
    }
}

/// A path inside a device root. The empty path is the root itself.
/// Serialised as its string; deserialising validates it again.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl TryFrom<String> for RelPath {
    type Error = PathError;

    fn try_from(path: String) -> Result<Self, PathError> {
        Self::new(&path)
    }
}

impl From<RelPath> for String {
    fn from(path: RelPath) -> Self {
        path.0
    }
}

fn check_component(path: &str, part: &str) -> Result<(), PathError> {
    if part.is_empty() || part == "." || part == ".." {
        return Err(PathError::BadComponent(path.to_owned()));
    }
    if part
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control())
    {
        return Err(PathError::BadCharacter(path.to_owned()));
    }
    Ok(())
}

impl RelPath {
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parses `CD/Game.iso`. Leading or doubled slashes are rejected.
    pub fn new(path: &str) -> Result<Self, PathError> {
        if path.is_empty() {
            return Ok(Self::root());
        }
        for part in path.split('/') {
            check_component(path, part)?;
        }
        Ok(Self(path.to_owned()))
    }

    /// This path with one more component.
    pub fn join(&self, name: &str) -> Result<Self, PathError> {
        check_component(name, name)?;
        Ok(if self.0.is_empty() {
            Self(name.to_owned())
        } else {
            Self(format!("{}/{name}", self.0))
        })
    }

    /// The path with `suffix` appended to its last component.
    pub fn with_suffix(&self, suffix: &str) -> Result<Self, PathError> {
        Self::new(&format!("{}{suffix}", self.0))
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    /// The containing folder; the root for a top-level name.
    pub fn parent(&self) -> Option<Self> {
        if self.0.is_empty() {
            return None;
        }
        Some(match self.0.rsplit_once('/') {
            Some((parent, _)) => Self(parent.to_owned()),
            None => Self::root(),
        })
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("/")
        } else {
            f.write_str(&self.0)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub is_dir: bool,
    pub len: u64,
}

/// What the planner needs to know about the volume behind a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsInfo {
    pub kind: FsKind,
    pub free_bytes: u64,
    /// Allocation unit; 4 KiB when the OS does not say.
    pub cluster_size: u32,
    /// OPL reads the device over SMB (a network share, or a folder in SMB
    /// mode, ADR-0043): SMB's limits apply, BDM's fragment rule does not.
    pub smb: bool,
    /// Changes when the stick is swapped or reformatted ([`crate::Volume::fingerprint`]).
    pub fingerprint: String,
}

/// A file being written on the device. Seeking back is for formats that
/// write an index after the data (ZSO, GAME-4).
pub trait DeviceFile: Write + Seek + Send {
    /// Reserves space for `len` bytes. Best effort: errors are for logs.
    fn preallocate(&mut self, len: u64) -> io::Result<()>;
    /// Flushes data and metadata to the device.
    fn sync(&mut self) -> io::Result<()>;
}

/// File operations on one device root.
pub trait DeviceFs: Send + Sync {
    /// A human-readable location, e.g. `E:\` or `fat32.img`.
    fn describe(&self) -> String;
    fn info(&self) -> io::Result<FsInfo>;
    /// `None` when nothing exists at `path`.
    fn metadata(&self, path: &RelPath) -> io::Result<Option<FileMeta>>;
    /// Entries of a folder, sorted by name. A missing folder is `NotFound`.
    fn list(&self, dir: &RelPath) -> io::Result<Vec<DirEntry>>;
    fn read(&self, path: &RelPath) -> io::Result<Vec<u8>>;
    /// Opens a file for reading, e.g. an image to read its game ID.
    fn open(&self, path: &RelPath) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(io::Cursor::new(self.read(path)?)))
    }
    /// Creates a new file; fails with `AlreadyExists` if anything is there.
    fn create(&self, path: &RelPath) -> io::Result<Box<dyn DeviceFile>>;
    /// Creates a folder; succeeds if it already exists.
    fn create_dir(&self, path: &RelPath) -> io::Result<()>;
    /// Renames a file or folder, replacing an existing file at `to`.
    fn rename(&self, from: &RelPath, to: &RelPath) -> io::Result<()>;
    fn remove_file(&self, path: &RelPath) -> io::Result<()>;
    /// Removes an empty folder.
    fn remove_dir(&self, path: &RelPath) -> io::Result<()>;
    /// Makes renames and removals in `dir` durable (directory fsync on Unix).
    fn sync_dir(&self, _dir: &RelPath) -> io::Result<()> {
        Ok(())
    }
    /// How many runs of contiguous clusters the file at `path` occupies,
    /// or `None` when the OS cannot tell. OPL mounts a VMC only when it is
    /// one run (research/02 §9).
    fn fragments(&self, _path: &RelPath) -> io::Result<Option<u32>> {
        Ok(None)
    }
}

/// Finds an entry by name the way FAT, exFAT and SMB do: ignoring case.
pub fn find_entry<'a>(entries: &'a [DirEntry], name: &str) -> Option<&'a DirEntry> {
    entries
        .iter()
        .find(|e| e.name == name)
        .or_else(|| entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
        .or_else(|| {
            let lower = name.to_lowercase();
            entries.iter().find(|e| e.name.to_lowercase() == lower)
        })
}

/// Lists a folder, treating a missing one as empty.
pub fn list_or_empty(fs: &dyn DeviceFs, dir: &RelPath) -> io::Result<Vec<DirEntry>> {
    match fs.list(dir) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        other => other,
    }
}

/// Replaces `path` with `bytes` so that a crash leaves either the old or the
/// new content: write a temporary file, sync it, rename it over `path`.
pub fn write_atomic(fs: &dyn DeviceFs, path: &RelPath, bytes: &[u8]) -> io::Result<()> {
    let tmp = path
        .with_suffix(TMP_SUFFIX)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if fs.metadata(&tmp)?.is_some() {
        fs.remove_file(&tmp)?;
    }
    let mut file = fs.create(&tmp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync()?;
    drop(file);
    fs.rename(&tmp, path)?;
    fs.sync_dir(&path.parent().unwrap_or_default())
}

/// A folder on the host: a mounted stick, a share or a test directory.
#[derive(Debug, Clone)]
pub struct HostFs {
    root: PathBuf,
    /// The device's mode when the front end knows it; else a network share
    /// counts as SMB.
    smb: Option<bool>,
}

struct HostFile(File);

impl Write for HostFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Seek for HostFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl DeviceFile for HostFile {
    fn preallocate(&mut self, len: u64) -> io::Result<()> {
        crate::sys::preallocate(&self.0, len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.0.sync_all()
    }
}

fn outside_root(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{} is outside the device root", path.display()),
    )
}

impl HostFs {
    /// `root` must be an existing folder; it is canonicalised.
    pub fn new(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a folder", root.display()),
            ));
        }
        Ok(Self { root, smb: None })
    }

    /// The same root as OPL reads it in `mode` (ADR-0043).
    #[must_use]
    pub fn with_mode(mut self, mode: DeviceMode) -> Self {
        self.smb = Some(mode == DeviceMode::Smb);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The host path of `path`.
    pub fn resolve(&self, path: &RelPath) -> PathBuf {
        let mut out = self.root.clone();
        out.extend(path.components());
        out
    }

    /// Like [`Self::resolve`], but the existing part of the path must not
    /// lead outside the root through a symlink.
    fn resolve_checked(&self, path: &RelPath) -> io::Result<PathBuf> {
        let full = self.resolve(path);
        let mut probe = full.as_path();
        loop {
            match fs::canonicalize(probe) {
                Ok(real) if real.starts_with(&self.root) => break,
                Ok(_) => return Err(outside_root(&full)),
                Err(_) => match probe.parent() {
                    Some(parent) if parent.starts_with(&self.root) => probe = parent,
                    _ => break,
                },
            }
        }
        // The last component itself may be a symlink pointing out; only
        // the link is renamed or removed then, never its target.
        Ok(full)
    }
}

impl DeviceFs for HostFs {
    fn describe(&self) -> String {
        self.root.display().to_string()
    }

    fn info(&self) -> io::Result<FsInfo> {
        let volume = volume_for_path(&self.root)?;
        Ok(FsInfo {
            kind: volume.fs,
            free_bytes: volume.free_bytes,
            cluster_size: volume.cluster_size.unwrap_or(4096),
            smb: self.smb.unwrap_or(volume.kind == VolumeKind::Remote),
            fingerprint: volume.fingerprint(),
        })
    }

    fn metadata(&self, path: &RelPath) -> io::Result<Option<FileMeta>> {
        match fs::symlink_metadata(self.resolve(path)) {
            Ok(meta) => Ok(Some(FileMeta {
                is_dir: meta.is_dir(),
                len: meta.len(),
            })),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn list(&self, dir: &RelPath) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(self.resolve_checked(dir)?)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let meta = entry.metadata()?;
            entries.push(DirEntry {
                name,
                is_dir: meta.is_dir(),
                len: meta.len(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read(&self, path: &RelPath) -> io::Result<Vec<u8>> {
        fs::read(self.resolve_checked(path)?)
    }

    fn open(&self, path: &RelPath) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(File::open(self.resolve_checked(path)?)?))
    }

    fn create(&self, path: &RelPath) -> io::Result<Box<dyn DeviceFile>> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.resolve_checked(path)?)?;
        Ok(Box::new(HostFile(file)))
    }

    fn create_dir(&self, path: &RelPath) -> io::Result<()> {
        let full = self.resolve_checked(path)?;
        match fs::create_dir(&full) {
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists && full.is_dir() => Ok(()),
            other => other,
        }
    }

    fn rename(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        fs::rename(self.resolve_checked(from)?, self.resolve_checked(to)?)
    }

    fn remove_file(&self, path: &RelPath) -> io::Result<()> {
        fs::remove_file(self.resolve_checked(path)?)
    }

    fn remove_dir(&self, path: &RelPath) -> io::Result<()> {
        fs::remove_dir(self.resolve_checked(path)?)
    }

    fn fragments(&self, path: &RelPath) -> io::Result<Option<u32>> {
        let file = File::open(self.resolve_checked(path)?)?;
        match crate::sys::fragments(&file) {
            Ok(runs) => Ok(Some(runs)),
            Err(err) if err.kind() == io::ErrorKind::Unsupported => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn sync_dir(&self, dir: &RelPath) -> io::Result<()> {
        #[cfg(unix)]
        {
            File::open(self.resolve_checked(dir)?)?.sync_all()
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn rel_paths() {
        assert!(RelPath::new("").unwrap().is_root());
        assert_eq!(rel("CD/Game.iso").file_name(), "Game.iso");
        assert_eq!(rel("CD/Game.iso").parent(), Some(rel("CD")));
        assert_eq!(rel("ul.cfg").parent(), Some(RelPath::root()));
        assert_eq!(rel("CD").join("x.iso").unwrap(), rel("CD/x.iso"));
        assert_eq!(RelPath::root().join("CD").unwrap(), rel("CD"));
        assert_eq!(
            rel("ul.cfg").with_suffix(TMP_SUFFIX).unwrap().as_str(),
            "ul.cfg.yaoplm-tmp"
        );
        for bad in [
            "/CD", "CD/", "CD//x", "../x", "CD/./x", "C:x", "a\\b", "x\0", "a\nb",
        ] {
            assert!(RelPath::new(bad).is_err(), "{bad:?}");
        }
        assert!(RelPath::root().join("..").is_err());
        assert!(RelPath::root().join("a/b").is_err());
    }

    #[test]
    fn host_fs_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let fs = HostFs::new(dir.path()).unwrap();
        fs.create_dir(&rel("CD")).unwrap();
        fs.create_dir(&rel("CD")).unwrap();
        let mut file = fs.create(&rel("CD/a.iso")).unwrap();
        file.preallocate(4096).unwrap();
        file.write_all(b"hello").unwrap();
        file.sync().unwrap();
        drop(file);
        assert_eq!(
            fs.create(&rel("CD/a.iso")).err().map(|e| e.kind()),
            Some(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(
            fs.metadata(&rel("CD/a.iso")).unwrap(),
            Some(FileMeta {
                is_dir: false,
                len: 5
            })
        );
        fs.rename(&rel("CD/a.iso"), &rel("CD/b.iso")).unwrap();
        assert_eq!(fs.list(&rel("CD")).unwrap()[0].name, "b.iso");
        assert_eq!(fs.metadata(&rel("CD/a.iso")).unwrap(), None);

        write_atomic(&fs, &rel("ul.cfg"), b"one").unwrap();
        write_atomic(&fs, &rel("ul.cfg"), b"two").unwrap();
        assert_eq!(fs.read(&rel("ul.cfg")).unwrap(), b"two");
        assert!(fs.metadata(&rel("ul.cfg.yaoplm-tmp")).unwrap().is_none());

        // A small file is one run where the OS can tell at all.
        assert!(matches!(
            fs.fragments(&rel("ul.cfg")).unwrap(),
            None | Some(0..=1)
        ));
        let mut big = fs.create(&rel("CD/big.bin")).unwrap();
        big.preallocate(1 << 20).unwrap();
        big.write_all(&vec![0xFF; 1 << 20]).unwrap();
        big.sync().unwrap();
        drop(big);
        #[cfg(windows)]
        assert!(
            fs.fragments(&rel("CD/big.bin"))
                .unwrap()
                .is_some_and(|n| n >= 1)
        );
        fs.remove_file(&rel("CD/big.bin")).unwrap();

        fs.remove_file(&rel("CD/b.iso")).unwrap();
        fs.remove_dir(&rel("CD")).unwrap();
        assert_eq!(
            fs.list(&rel("CD")).err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
        assert!(list_or_empty(&fs, &rel("CD")).unwrap().is_empty());
        let info = fs.info().unwrap();
        assert!(info.free_bytes > 0);
    }

    #[test]
    fn case_insensitive_lookup() {
        let entries = vec![
            DirEntry {
                name: "cd".into(),
                is_dir: true,
                len: 0,
            },
            DirEntry {
                name: "Игра.iso".into(),
                is_dir: false,
                len: 0,
            },
        ];
        assert_eq!(find_entry(&entries, "CD").unwrap().name, "cd");
        assert_eq!(find_entry(&entries, "игра.ISO").unwrap().name, "Игра.iso");
        assert!(find_entry(&entries, "DVD").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape() {
        let outside = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("CD")).unwrap();
        let fs = HostFs::new(dir.path()).unwrap();
        let err = fs.create(&rel("CD/evil.iso")).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(!outside.path().join("evil.iso").exists());
    }
}
