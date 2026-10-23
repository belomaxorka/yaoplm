//! Disc images inside zip and 7z archives (IMP-1b).
//!
//! People keep dumps packed; YAOPLM imports them without unpacking to the
//! host disk first. An archive is listed like a folder, and an entry reads
//! as a stream that can seek: a stored zip entry directly, a compressed one
//! forward by decoding on and backward by starting over. Compressed entries
//! are decoded on a worker thread and handed over in blocks: the zip and 7z
//! readers borrow the archive file, so they cannot live in a struct.
//!
//! In a solid 7z archive every entry before the wanted one in its block is
//! decoded too (sevenz-rust2 `BlockDecoder`). Probing an image reads a few
//! megabytes from its start and a copy reads it once, front to back, so
//! both stay linear. RAR is not here: its only decoder has a licence that
//! does not fit GPL-3 (ARCHITECTURE.md §2, feature `rar` later).

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

/// What an archive is read from.
pub trait Source: Read + Seek + Send {}

impl<T: Read + Seek + Send> Source for T {}

/// Decoded bytes handed over at a time.
const BLOCK: usize = 1 << 20;
/// Blocks queued between the worker and the reader.
const IN_FLIGHT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    SevenZip,
}

impl Kind {
    /// By extension, ignoring case.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "7z" => Some(Self::SevenZip),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("zip: {0}")]
    Zip(String),
    #[error("7z: {0}")]
    SevenZip(String),
    #[error("{0} is not in the archive")]
    NotFound(String),
    #[error("{0} is encrypted")]
    Encrypted(String),
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(err: zip::result::ZipError) -> Self {
        match err {
            zip::result::ZipError::Io(err) => Self::Io(err),
            other => Self::Zip(other.to_string()),
        }
    }
}

impl From<sevenz_rust2::Error> for ArchiveError {
    fn from(err: sevenz_rust2::Error) -> Self {
        Self::SevenZip(err.to_string())
    }
}

fn into_io(err: ArchiveError) -> io::Error {
    match err {
        ArchiveError::Io(err) => err,
        ArchiveError::NotFound(name) => io::Error::new(io::ErrorKind::NotFound, name),
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// A file or folder in an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `/`-separated, without a leading or trailing slash.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

fn clean(name: &str) -> String {
    name.replace('\\', "/").trim_matches('/').to_owned()
}

/// Everything in the archive, folders included (also the ones only
/// implied by the paths of files).
pub fn list(kind: Kind, source: &mut dyn Source) -> Result<Vec<Entry>, ArchiveError> {
    source.seek(SeekFrom::Start(0))?;
    let mut entries: Vec<Entry> = match kind {
        Kind::Zip => {
            let mut archive = zip::ZipArchive::new(&mut *source)?;
            let mut out = Vec::with_capacity(archive.len());
            for index in 0..archive.len() {
                let entry = archive.by_index_raw(index)?;
                let name = clean(&entry.name()?);
                if name.is_empty() {
                    continue;
                }
                out.push(Entry {
                    name,
                    size: entry.size(),
                    is_dir: entry.is_dir(),
                });
            }
            out
        }
        Kind::SevenZip => {
            let mut reader = &mut *source;
            let archive =
                sevenz_rust2::Archive::read(&mut reader, &sevenz_rust2::Password::empty())?;
            archive
                .files
                .iter()
                .filter(|f| !f.is_anti_item)
                .map(|f| Entry {
                    name: clean(&f.name),
                    size: f.size,
                    is_dir: f.is_directory,
                })
                .filter(|e| !e.name.is_empty())
                .collect()
        }
    };
    // Folders that only appear in the paths of files.
    let mut implied = Vec::new();
    for entry in &entries {
        let mut path = entry.name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if !entries.iter().any(|e| e.name == parent) && !implied.contains(&parent.to_owned()) {
                implied.push(parent.to_owned());
            }
            path = parent;
        }
    }
    entries.extend(implied.into_iter().map(|name| Entry {
        name,
        size: 0,
        is_dir: true,
    }));
    Ok(entries)
}

/// One entry, readable and seekable.
pub struct EntryReader {
    size: u64,
    pos: u64,
    inner: Inner,
}

enum Inner {
    /// A stored zip entry: a window on the archive file.
    Window {
        file: Box<dyn Source>,
        start: u64,
    },
    Piped(Piped),
}

impl std::fmt::Debug for EntryReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryReader")
            .field("size", &self.size)
            .field("pos", &self.pos)
            .finish_non_exhaustive()
    }
}

/// Opens `name` (as [`list`] spells it) for reading.
pub fn open_entry(
    kind: Kind,
    mut file: Box<dyn Source>,
    name: &str,
) -> Result<EntryReader, ArchiveError> {
    file.seek(SeekFrom::Start(0))?;
    let wanted = clean(name);
    match kind {
        Kind::Zip => {
            let (size, stored_at) = {
                let mut archive = zip::ZipArchive::new(&mut *file)?;
                let index = (0..archive.len())
                    .find(|&i| {
                        archive
                            .by_index_raw(i)
                            .ok()
                            .and_then(|e| e.name().ok().map(|n| clean(&n) == wanted))
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| ArchiveError::NotFound(wanted.clone()))?;
                let entry = archive.by_index_raw(index)?;
                if entry.encrypted() {
                    return Err(ArchiveError::Encrypted(wanted));
                }
                let stored = entry.compression() == zip::CompressionMethod::Stored;
                (entry.size(), entry.data_start().filter(|_| stored))
            };
            Ok(EntryReader {
                size,
                pos: 0,
                inner: match stored_at {
                    Some(start) => Inner::Window { file, start },
                    None => Inner::Piped(Piped::new(kind, wanted, file)),
                },
            })
        }
        Kind::SevenZip => {
            let size = {
                let archive =
                    sevenz_rust2::Archive::read(&mut file, &sevenz_rust2::Password::empty())?;
                archive
                    .files
                    .iter()
                    .find(|f| !f.is_directory && clean(&f.name) == wanted)
                    .map(|f| f.size)
                    .ok_or_else(|| ArchiveError::NotFound(wanted.clone()))?
            };
            Ok(EntryReader {
                size,
                pos: 0,
                inner: Inner::Piped(Piped::new(kind, wanted, file)),
            })
        }
    }
}

impl EntryReader {
    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

impl Read for EntryReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let want = usize::try_from(self.size - self.pos)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = match &mut self.inner {
            Inner::Window { file, start } => {
                file.seek(SeekFrom::Start(*start + self.pos))?;
                file.read(&mut buf[..want])?
            }
            Inner::Piped(piped) => piped.read_at(self.pos, &mut buf[..want])?,
        };
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for EntryReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.size.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        Ok(self.pos)
    }
}

/// An entry decoded on a worker thread.
struct Piped {
    kind: Kind,
    name: String,
    /// The archive file while no worker runs.
    file: Option<Box<dyn Source>>,
    worker: Option<Worker>,
    /// Decoded bytes not read yet, and where they start in the entry.
    chunk: Vec<u8>,
    chunk_at: u64,
}

struct Worker {
    handle: JoinHandle<Box<dyn Source>>,
    rx: Receiver<io::Result<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    /// Entry offset of the next block the worker sends.
    next: u64,
}

impl Piped {
    fn new(kind: Kind, name: String, file: Box<dyn Source>) -> Self {
        Self {
            kind,
            name,
            file: Some(file),
            worker: None,
            chunk: Vec::new(),
            chunk_at: 0,
        }
    }

    /// Stops the worker and takes the archive file back.
    fn stop(&mut self) -> io::Result<()> {
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::SeqCst);
            drop(worker.rx);
            let file = worker
                .handle
                .join()
                .map_err(|_| io::Error::other("the archive decoder panicked"))?;
            self.file = Some(file);
        }
        self.chunk.clear();
        self.chunk_at = 0;
        Ok(())
    }

    fn start(&mut self) -> io::Result<()> {
        self.stop()?;
        let file = self
            .file
            .take()
            .ok_or_else(|| io::Error::other("the archive file is gone"))?;
        let (tx, rx) = sync_channel(IN_FLIGHT);
        let stop = Arc::new(AtomicBool::new(false));
        let (kind, name, flag) = (self.kind, self.name.clone(), Arc::clone(&stop));
        let handle = thread::spawn(move || {
            let mut file = file;
            if let Err(err) = decode(kind, &name, &mut file, &tx, &flag) {
                let _ = tx.send(Err(into_io(err)));
            }
            file
        });
        self.worker = Some(Worker {
            handle,
            rx,
            stop,
            next: 0,
        });
        Ok(())
    }

    /// Reads at entry offset `pos`, restarting the decoder for a backward
    /// seek and decoding on for a forward one.
    fn read_at(&mut self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        let behind = self
            .worker
            .as_ref()
            .is_none_or(|w| pos < self.chunk_at.min(w.next));
        if behind {
            self.start()?;
        }
        loop {
            let end = self.chunk_at + self.chunk.len() as u64;
            if pos >= self.chunk_at && pos < end {
                let from = (pos - self.chunk_at) as usize;
                let n = (self.chunk.len() - from).min(buf.len());
                buf[..n].copy_from_slice(&self.chunk[from..from + n]);
                return Ok(n);
            }
            let Some(worker) = self.worker.as_mut() else {
                return Ok(0);
            };
            match worker.rx.recv() {
                Ok(Ok(block)) => {
                    self.chunk_at = worker.next;
                    worker.next += block.len() as u64;
                    self.chunk = block;
                }
                Ok(Err(err)) => return Err(err),
                // The worker finished: the entry ended early.
                Err(_) => return Ok(0),
            }
        }
    }
}

impl Drop for Piped {
    fn drop(&mut self) {
        // Detach: a solid 7z decoder notices the flag between blocks.
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::SeqCst);
        }
    }
}

/// Reads `reader` to the end in blocks; `send` gets each one. Returns
/// early when the reader side is gone or `stop` is set.
fn pump(
    reader: &mut dyn Read,
    mut send: Option<&SyncSender<io::Result<Vec<u8>>>>,
    stop: &AtomicBool,
) -> io::Result<()> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut block = vec![0u8; BLOCK];
        let mut filled = 0;
        while filled < BLOCK {
            match reader.read(&mut block[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
        if filled == 0 {
            return Ok(());
        }
        block.truncate(filled);
        if let Some(tx) = send
            && tx.send(Ok(block)).is_err()
        {
            send = None;
            stop.store(true, Ordering::SeqCst);
        }
    }
}

fn decode(
    kind: Kind,
    name: &str,
    file: &mut Box<dyn Source>,
    tx: &SyncSender<io::Result<Vec<u8>>>,
    stop: &AtomicBool,
) -> Result<(), ArchiveError> {
    file.seek(SeekFrom::Start(0))?;
    match kind {
        Kind::Zip => {
            let mut archive = zip::ZipArchive::new(&mut **file)?;
            let index = (0..archive.len())
                .find(|&i| {
                    archive
                        .by_index_raw(i)
                        .ok()
                        .and_then(|e| e.name().ok().map(|n| clean(&n) == name))
                        .unwrap_or(false)
                })
                .ok_or_else(|| ArchiveError::NotFound(name.to_owned()))?;
            let mut entry = archive.by_index(index)?;
            pump(&mut entry, Some(tx), stop)?;
        }
        Kind::SevenZip => {
            let password = sevenz_rust2::Password::empty();
            let archive = sevenz_rust2::Archive::read(file, &password)?;
            let index = archive
                .files
                .iter()
                .position(|f| !f.is_directory && clean(&f.name) == name)
                .ok_or_else(|| ArchiveError::NotFound(name.to_owned()))?;
            let Some(block) = archive.stream_map.file_block_index[index] else {
                // An empty file has no block.
                return Ok(());
            };
            let decoder = sevenz_rust2::BlockDecoder::new(1, block, &archive, &password, file);
            decoder.for_each_entries(&mut |entry, reader| {
                if clean(&entry.name) == name {
                    pump(reader, Some(tx), stop)?;
                    return Ok(false);
                }
                // Entries in front of it in a solid block are decoded and
                // dropped; the flag stops a long skip.
                pump(reader, None, stop)?;
                Ok(!stop.load(Ordering::SeqCst))
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn zip_of(files: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, bytes) in files {
            zip.start_file(
                *name,
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    fn check_reads(mut reader: EntryReader, expected: &[u8]) {
        assert_eq!(reader.len(), expected.len() as u64);
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert!(all == expected, "whole entry differs");
        // Back to the start, then a jump forward.
        reader.seek(SeekFrom::Start(10)).unwrap();
        let mut ten = [0u8; 10];
        reader.read_exact(&mut ten).unwrap();
        assert_eq!(ten, expected[10..20]);
        let far = expected.len() as u64 - 5;
        reader.seek(SeekFrom::Start(far)).unwrap();
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, expected[far as usize..]);
    }

    #[test]
    fn zip_entries_stored_and_deflated() {
        let big = data(3 * BLOCK + 123);
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let archive = zip_of(&[("games/Game.iso", &big), ("readme.txt", b"hi")], method);
            let entries = list(Kind::Zip, &mut Cursor::new(archive.clone())).unwrap();
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, ["games/Game.iso", "readme.txt", "games"]);
            assert!(entries[2].is_dir);
            let reader = open_entry(
                Kind::Zip,
                Box::new(Cursor::new(archive.clone())),
                "games/Game.iso",
            )
            .unwrap();
            check_reads(reader, &big);
            assert!(matches!(
                open_entry(Kind::Zip, Box::new(Cursor::new(archive)), "nope.iso"),
                Err(ArchiveError::NotFound(_))
            ));
        }
    }

    #[test]
    fn seven_zip_entries() {
        let big = data(2 * BLOCK + 77);
        let small = data(1000);
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = sevenz_rust2::ArchiveWriter::new(&mut out).unwrap();
            writer
                .push_archive_entry(
                    sevenz_rust2::ArchiveEntry::new_file("first.bin"),
                    Some(Cursor::new(small.clone())),
                )
                .unwrap();
            writer
                .push_archive_entry(
                    sevenz_rust2::ArchiveEntry::new_file("dir/Game.iso"),
                    Some(Cursor::new(big.clone())),
                )
                .unwrap();
            writer.finish().unwrap();
        }
        let archive = out.into_inner();
        let entries = list(Kind::SevenZip, &mut Cursor::new(archive.clone())).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.name == "dir/Game.iso" && e.size == big.len() as u64)
        );
        assert!(entries.iter().any(|e| e.name == "dir" && e.is_dir));
        let reader = open_entry(
            Kind::SevenZip,
            Box::new(Cursor::new(archive)),
            "dir/Game.iso",
        )
        .unwrap();
        check_reads(reader, &big);
    }

    #[test]
    fn garbage_is_not_an_archive() {
        let mut junk = Cursor::new(vec![0u8; 4096]);
        assert!(list(Kind::Zip, &mut junk).is_err());
        assert!(list(Kind::SevenZip, &mut junk).is_err());
        assert_eq!(Kind::from_extension("7Z"), Some(Kind::SevenZip));
        assert_eq!(Kind::from_extension("rar"), None);
    }
}
