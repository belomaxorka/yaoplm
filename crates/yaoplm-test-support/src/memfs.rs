//! An in-memory device for engine tests.
//!
//! Behaves like a FAT or exFAT stick where it matters: names are matched
//! case-insensitively, FAT files stop at 4 GiB - 1, free space shrinks as
//! files grow. Files larger than [`SMALL_MAX`] keep only their length and a
//! 16-byte probe every [`PROBE_EVERY`] bytes, so multi-gigabyte imports cost
//! no memory. [`MemFs::unplug_after`] makes the device vanish mid-write, like
//! a stick pulled out: every later operation fails, so nothing can be
//! cleaned up until [`MemFs::replug`].

use std::collections::BTreeMap;
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use yaoplm_device::fs::{DeviceFile, DeviceFs, DirEntry, FileMeta, FsInfo};
use yaoplm_device::{FsKind, RelPath};

/// Files up to this size are stored byte for byte.
pub const SMALL_MAX: u64 = 8 << 20;
pub const PROBE_EVERY: u64 = 1 << 20;
pub const PROBE_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Bytes(Vec<u8>),
    /// Length plus the bytes at every multiple of [`PROBE_EVERY`].
    Sparse {
        len: u64,
        probes: BTreeMap<u64, Vec<u8>>,
    },
}

impl Data {
    pub fn len(&self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes.len() as u64,
            Self::Sparse { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn append(&mut self, buf: &[u8]) {
        if let Self::Bytes(bytes) = self {
            if bytes.len() as u64 + buf.len() as u64 <= SMALL_MAX {
                bytes.extend_from_slice(buf);
                return;
            }
            let mut probes = BTreeMap::new();
            record_probes(&mut probes, 0, bytes);
            *self = Self::Sparse {
                len: bytes.len() as u64,
                probes,
            };
        }
        if let Self::Sparse { len, probes } = self {
            record_probes(probes, *len, buf);
            *len += buf.len() as u64;
        }
    }

    /// Writes `buf` at `at` (at most the current length); what runs past
    /// the end is appended.
    fn write_at(&mut self, at: u64, buf: &[u8]) {
        let len = self.len();
        let inside = usize::try_from(len.saturating_sub(at))
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let (over, rest) = buf.split_at(inside);
        match self {
            Self::Bytes(bytes) => {
                let at = at as usize;
                bytes[at..at + over.len()].copy_from_slice(over);
            }
            Self::Sparse { probes, .. } => {
                let end = at + over.len() as u64;
                for (&start, probe) in probes.range_mut(at.saturating_sub(PROBE_LEN as u64)..end) {
                    for (i, byte) in probe.iter_mut().enumerate() {
                        let pos = start + i as u64;
                        if (at..end).contains(&pos) {
                            *byte = over[(pos - at) as usize];
                        }
                    }
                }
            }
        }
        if !rest.is_empty() {
            self.append(rest);
        }
    }
}

fn record_probes(probes: &mut BTreeMap<u64, Vec<u8>>, start: u64, buf: &[u8]) {
    let end = start + buf.len() as u64;
    let mut at = start.div_ceil(PROBE_EVERY) * PROBE_EVERY;
    while at + PROBE_LEN as u64 <= end {
        let from = (at - start) as usize;
        probes.insert(at, buf[from..from + PROBE_LEN].to_vec());
        at += PROBE_EVERY;
    }
}

#[derive(Debug, Clone)]
struct Node {
    /// The name as created; keys are lower-case.
    name: String,
    dir: bool,
    data: Data,
}

struct Fault {
    op: &'static str,
    path: String,
}

struct State {
    nodes: BTreeMap<String, Node>,
    info: FsInfo,
    unplugged: bool,
    /// Bytes still accepted before the device vanishes.
    budget: Option<u64>,
    fault: Option<Fault>,
    ops: Vec<String>,
    /// Fragment counts to report, by key; unknown otherwise.
    fragments: BTreeMap<String, u32>,
}

fn key(path: &str) -> String {
    path.to_lowercase()
}

fn parent_key(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "device removed")
}

impl State {
    fn check(&mut self, op: &'static str, path: &RelPath) -> io::Result<()> {
        if self.unplugged {
            return Err(gone());
        }
        if let Some(fault) = &self.fault
            && fault.op == op
            && path.as_str().contains(&fault.path)
        {
            self.fault = None;
            return Err(io::Error::other(format!("injected failure: {op} {path}")));
        }
        self.ops.push(format!("{op} {path}"));
        Ok(())
    }

    fn dir_exists(&self, key: &str) -> bool {
        key.is_empty() || self.nodes.get(key).is_some_and(|n| n.dir)
    }

    fn used(&self) -> u64 {
        let cluster = u64::from(self.info.cluster_size.max(1));
        self.nodes
            .values()
            .map(|n| n.data.len().div_ceil(cluster) * cluster)
            .sum()
    }
}

/// See the module docs.
#[derive(Clone)]
pub struct MemFs {
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for MemFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemFs")
            .field("files", &self.files())
            .finish()
    }
}

impl MemFs {
    /// An empty 64 GiB device with 32 KiB clusters.
    pub fn new(kind: FsKind) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                nodes: BTreeMap::new(),
                info: FsInfo {
                    kind,
                    free_bytes: 64 << 30,
                    cluster_size: 32 * 1024,
                    smb: false,
                    mmce: false,
                    fingerprint: "MEM-0001".into(),
                },
                unplugged: false,
                budget: None,
                fault: None,
                ops: Vec::new(),
                fragments: BTreeMap::new(),
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Total free space before any file is written.
    pub fn with_free_bytes(self, bytes: u64) -> Self {
        self.lock().info.free_bytes = bytes;
        self
    }

    pub fn with_smb(self, smb: bool) -> Self {
        self.lock().info.smb = smb;
        self
    }

    /// Read through a memory card emulator's firmware (ADR-0048).
    pub fn with_mmce(self, mmce: bool) -> Self {
        self.lock().info.mmce = mmce;
        self
    }

    pub fn set_fingerprint(&self, fingerprint: &str) {
        self.lock().info.fingerprint = fingerprint.to_owned();
    }

    /// The device disappears once `bytes` more bytes have been written.
    pub fn unplug_after(&self, bytes: u64) {
        self.lock().budget = Some(bytes);
    }

    pub fn unplug(&self) {
        self.lock().unplugged = true;
    }

    /// Brings the device back with whatever was written before it vanished.
    pub fn replug(&self) {
        let mut state = self.lock();
        state.unplugged = false;
        state.budget = None;
    }

    /// The next `op` (`create`, `rename`, `remove`, `write`, ...) on a path
    /// containing `path` fails once.
    pub fn fail_once(&self, op: &'static str, path: &str) {
        self.lock().fault = Some(Fault {
            op,
            path: path.to_owned(),
        });
    }

    /// Makes [`DeviceFs::fragments`] report `runs` for `path`, as if the
    /// volume's free space were cut up.
    pub fn set_fragments(&self, path: &str, runs: u32) {
        self.lock().fragments.insert(key(path), runs);
    }

    /// Adds a file directly, creating folders as needed.
    pub fn insert_file(&self, path: &str, bytes: &[u8]) {
        let mut state = self.lock();
        let mut prefix = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for dir in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(dir);
            state.nodes.entry(key(&prefix)).or_insert(Node {
                name: (*dir).to_owned(),
                dir: true,
                data: Data::Bytes(Vec::new()),
            });
        }
        state.nodes.insert(
            key(path),
            Node {
                name: parts[parts.len() - 1].to_owned(),
                dir: false,
                data: Data::Bytes(bytes.to_vec()),
            },
        );
    }

    pub fn insert_dir(&self, path: &str) {
        self.insert_file(&format!("{path}/.keep"), b"");
        self.lock().nodes.remove(&key(&format!("{path}/.keep")));
    }

    /// The content of a file, by path in any case.
    pub fn data(&self, path: &str) -> Option<Data> {
        self.lock()
            .nodes
            .get(&key(path))
            .filter(|n| !n.dir)
            .map(|n| n.data.clone())
    }

    pub fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        match self.data(path)? {
            Data::Bytes(bytes) => Some(bytes),
            Data::Sparse { .. } => None,
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.lock().nodes.contains_key(&key(path))
    }

    /// Every file as `(path as created, length)`, sorted by path.
    pub fn files(&self) -> Vec<(String, u64)> {
        let state = self.lock();
        let mut files: Vec<(String, u64)> = state
            .nodes
            .iter()
            .filter(|(_, n)| !n.dir)
            .map(|(k, n)| {
                let parent = parent_key(k);
                let prefix: Vec<String> = if parent.is_empty() {
                    Vec::new()
                } else {
                    let mut names = Vec::new();
                    let mut at = parent;
                    while !at.is_empty() {
                        names.push(state.nodes.get(at).map_or(at, |p| &p.name).to_owned());
                        at = parent_key(at);
                    }
                    names.reverse();
                    names
                };
                let mut path = prefix.join("/");
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(&n.name);
                (path, n.data.len())
            })
            .collect();
        files.sort();
        files
    }

    /// Operations performed so far, e.g. `rename CD/a.iso`.
    pub fn ops(&self) -> Vec<String> {
        self.lock().ops.clone()
    }
}

struct MemFile {
    state: Arc<Mutex<State>>,
    key: String,
    path: RelPath,
    /// Where the next write goes.
    pos: u64,
}

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.unplugged {
            return Err(gone());
        }
        if let Some(fault) = &state.fault
            && fault.op == "write"
            && self.path.as_str().contains(&fault.path)
        {
            state.fault = None;
            return Err(io::Error::other("injected write failure"));
        }
        let mut accepted = buf;
        let mut vanish = false;
        if let Some(budget) = state.budget {
            if (buf.len() as u64) > budget {
                accepted = &buf[..budget as usize];
                vanish = true;
            }
            state.budget = Some(budget - accepted.len() as u64);
        }
        let limit = state.info.kind.max_file_size();
        let free = state.info.free_bytes.saturating_sub(state.used());
        let node = state
            .nodes
            .get_mut(&self.key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "file was removed"))?;
        let old_len = node.data.len();
        let new_len = old_len.max(self.pos + accepted.len() as u64);
        if limit.is_some_and(|max| new_len > max) {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "FAT files are limited to 4 GiB - 1",
            ));
        }
        if new_len - old_len > free {
            return Err(io::Error::new(io::ErrorKind::StorageFull, "device is full"));
        }
        if self.pos == old_len {
            node.data.append(accepted);
        } else {
            node.data.write_at(self.pos, accepted);
        }
        self.pos += accepted.len() as u64;
        if vanish {
            state.unplugged = true;
            return Err(gone());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for MemFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.unplugged {
            return Err(gone());
        }
        let len = state.nodes.get(&self.key).map_or(0, |n| n.data.len());
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        match target {
            Some(t) if t <= len => {
                self.pos = t;
                Ok(t)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MemFs files cannot seek before the start or past the end",
            )),
        }
    }
}

impl DeviceFile for MemFile {
    fn preallocate(&mut self, _len: u64) -> io::Result<()> {
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.unplugged { Err(gone()) } else { Ok(()) }
    }
}

fn not_found(path: &RelPath) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{path} not found"))
}

impl DeviceFs for MemFs {
    fn describe(&self) -> String {
        "memory device".into()
    }

    fn info(&self) -> io::Result<FsInfo> {
        let mut state = self.lock();
        state.check("info", &RelPath::root())?;
        let mut info = state.info.clone();
        info.free_bytes = info.free_bytes.saturating_sub(state.used());
        Ok(info)
    }

    fn fragments(&self, path: &RelPath) -> io::Result<Option<u32>> {
        let state = self.lock();
        if state.unplugged {
            return Err(gone());
        }
        Ok(state.fragments.get(&key(path.as_str())).copied())
    }

    fn metadata(&self, path: &RelPath) -> io::Result<Option<FileMeta>> {
        let state = self.lock();
        if state.unplugged {
            return Err(gone());
        }
        if path.is_root() {
            return Ok(Some(FileMeta {
                is_dir: true,
                len: 0,
            }));
        }
        Ok(state.nodes.get(&key(path.as_str())).map(|n| FileMeta {
            is_dir: n.dir,
            len: n.data.len(),
        }))
    }

    fn list(&self, dir: &RelPath) -> io::Result<Vec<DirEntry>> {
        let state = self.lock();
        if state.unplugged {
            return Err(gone());
        }
        let dir_key = key(dir.as_str());
        if !state.dir_exists(&dir_key) {
            return Err(not_found(dir));
        }
        let mut entries: Vec<DirEntry> = state
            .nodes
            .iter()
            .filter(|(k, _)| parent_key(k) == dir_key && !k.is_empty())
            .map(|(_, n)| DirEntry {
                name: n.name.clone(),
                is_dir: n.dir,
                len: n.data.len(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn read(&self, path: &RelPath) -> io::Result<Vec<u8>> {
        let mut state = self.lock();
        state.check("read", path)?;
        match state.nodes.get(&key(path.as_str())) {
            Some(Node {
                dir: false,
                data: Data::Bytes(bytes),
                ..
            }) => Ok(bytes.clone()),
            Some(Node { dir: false, .. }) => {
                Err(io::Error::other("large file content is not kept"))
            }
            _ => Err(not_found(path)),
        }
    }

    fn create(&self, path: &RelPath) -> io::Result<Box<dyn DeviceFile>> {
        let mut state = self.lock();
        state.check("create", path)?;
        let k = key(path.as_str());
        if !state.dir_exists(parent_key(&k)) {
            return Err(not_found(path));
        }
        if state.nodes.contains_key(&k) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path} exists"),
            ));
        }
        state.nodes.insert(
            k.clone(),
            Node {
                name: path.file_name().to_owned(),
                dir: false,
                data: Data::Bytes(Vec::new()),
            },
        );
        Ok(Box::new(MemFile {
            state: Arc::clone(&self.state),
            key: k,
            path: path.clone(),
            pos: 0,
        }))
    }

    fn create_dir(&self, path: &RelPath) -> io::Result<()> {
        let mut state = self.lock();
        state.check("mkdir", path)?;
        let k = key(path.as_str());
        if !state.dir_exists(parent_key(&k)) {
            return Err(not_found(path));
        }
        match state.nodes.get(&k) {
            Some(n) if n.dir => Ok(()),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{path} is a file"),
            )),
            None => {
                state.nodes.insert(
                    k,
                    Node {
                        name: path.file_name().to_owned(),
                        dir: true,
                        data: Data::Bytes(Vec::new()),
                    },
                );
                Ok(())
            }
        }
    }

    fn rename(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        let mut state = self.lock();
        state.check("rename", from)?;
        let (from_key, to_key) = (key(from.as_str()), key(to.as_str()));
        let node = state
            .nodes
            .get(&from_key)
            .cloned()
            .ok_or_else(|| not_found(from))?;
        if node.dir {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "MemFs renames files only",
            ));
        }
        if !state.dir_exists(parent_key(&to_key)) {
            return Err(not_found(to));
        }
        if state.nodes.get(&to_key).is_some_and(|n| n.dir) {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("{to} is a folder"),
            ));
        }
        state.nodes.remove(&from_key);
        state.nodes.insert(
            to_key,
            Node {
                name: to.file_name().to_owned(),
                ..node
            },
        );
        Ok(())
    }

    fn remove_file(&self, path: &RelPath) -> io::Result<()> {
        let mut state = self.lock();
        state.check("remove", path)?;
        let k = key(path.as_str());
        match state.nodes.get(&k) {
            Some(n) if !n.dir => {
                state.nodes.remove(&k);
                Ok(())
            }
            _ => Err(not_found(path)),
        }
    }

    fn remove_dir(&self, path: &RelPath) -> io::Result<()> {
        let mut state = self.lock();
        state.check("rmdir", path)?;
        let k = key(path.as_str());
        if !state.nodes.get(&k).is_some_and(|n| n.dir) {
            return Err(not_found(path));
        }
        if state.nodes.keys().any(|c| parent_key(c) == k && *c != k) {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                format!("{path} is not empty"),
            ));
        }
        state.nodes.remove(&k);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use yaoplm_device::fs::write_atomic;

    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn behaves_like_a_fat_stick() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.create_dir(&rel("CD")).unwrap();
        let mut file = fs.create(&rel("cd/Game.iso")).unwrap();
        file.write_all(b"data").unwrap();
        drop(file);
        assert!(fs.create(&rel("CD/GAME.ISO")).is_err());
        assert_eq!(fs.files(), vec![("CD/Game.iso".to_owned(), 4)]);
        assert_eq!(fs.list(&rel("cd")).unwrap()[0].name, "Game.iso");
        write_atomic(&fs, &rel("ul.cfg"), b"x").unwrap();
        assert_eq!(fs.bytes("UL.CFG").unwrap(), b"x");
        assert!(fs.create(&rel("DVD/x.iso")).is_err());
    }

    #[test]
    fn large_files_keep_probes() {
        let fs = MemFs::new(FsKind::ExFat);
        let mut file = fs.create(&rel("big")).unwrap();
        let block: Vec<u8> = (0..(3 << 20)).map(|i| (i % 253) as u8).collect();
        for _ in 0..4 {
            file.write_all(&block).unwrap();
        }
        let Some(Data::Sparse { len, probes }) = fs.data("big") else {
            panic!("expected sparse data");
        };
        assert_eq!(len, 12 << 20);
        let at = 5 * PROBE_EVERY;
        let expected: Vec<u8> = (at..at + PROBE_LEN as u64)
            .map(|i| ((i % (3 << 20)) % 253) as u8)
            .collect();
        assert_eq!(probes[&at], expected);
    }

    #[test]
    fn fat_limit_and_unplug() {
        let fs = MemFs::new(FsKind::Fat32);
        let mut file = fs.create(&rel("a")).unwrap();
        fs.unplug_after(10);
        assert_eq!(
            file.write(&[0u8; 16]).unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        assert!(fs.metadata(&rel("a")).is_err());
        fs.replug();
        assert_eq!(fs.data("a").unwrap().len(), 10);

        let mut file = fs.create(&rel("b")).unwrap();
        if let Some(node) = fs.lock().nodes.get_mut("b") {
            node.data = Data::Sparse {
                len: u64::from(u32::MAX),
                probes: BTreeMap::new(),
            };
        }
        file.seek(SeekFrom::End(0)).unwrap();
        assert_eq!(
            file.write(&[0]).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
        // Overwriting inside the file is fine at the limit.
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&[1, 2, 3]).unwrap();
    }
}
