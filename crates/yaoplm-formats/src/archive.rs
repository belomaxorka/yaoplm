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
//!
//! Entry names are made safe the way 7-Zip extracts them: drive letters,
//! `.` and `..` are dropped, so no name leads out of the archive (a 7z made
//! with `-spf` stores `C:\Games\x.iso`). Every entry is checked against
//! its CRC when read to the end: zip by its reader for compressed entries
//! and here for stored ones, 7z by its decoder.

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

/// `C:` at the start of a component: a drive, which would make the
/// component replace the path it is joined to.
fn strip_drive(part: &str) -> &str {
    match part.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => &part[2..],
        _ => part,
    }
}

/// The name as `/`-separated components that stay inside the archive, or
/// `None` when nothing is left.
fn safe_name(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw
        .split(['/', '\\'])
        .map(strip_drive)
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Everything in the archive, folders included (also the ones only
/// implied by the paths of files).
pub fn list(kind: Kind, source: &mut dyn Source) -> Result<Vec<Entry>, ArchiveError> {
    source.seek(SeekFrom::Start(0))?;
    let mut entries: Vec<Entry> = match kind {
        Kind::Zip => {
            let archive = zip::ZipArchive::new(&mut *source)?;
            let mut out = Vec::with_capacity(archive.len());
            for index in 0..archive.len() {
                // The central directory alone: one damaged local header must
                // not hide the whole archive.
                let entry = archive.by_index_data(index)?;
                let Some(name) = safe_name(&entry.name()?) else {
                    continue;
                };
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
                .filter_map(|f| {
                    Some(Entry {
                        name: safe_name(&f.name)?,
                        size: f.size,
                        is_dir: f.is_directory,
                    })
                })
                .collect()
        }
    };
    // Folders that only appear in the paths of files. A picture set holds
    // tens of thousands of entries: look names up, do not scan for them.
    let known: std::collections::HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    let mut implied = Vec::new();
    for entry in &entries {
        let mut path = entry.name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if !known.contains(parent) && seen.insert(parent) {
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

/// Hands every file of the archive to `visit`, in stored order, with a
/// reader of its bytes; what `visit` leaves unread is skipped. One pass
/// over the archive: a picture set (ART-5) holds thousands of small files,
/// and opening them one by one would decode a solid 7z block again for
/// each.
///
/// A zip entry that cannot be read is handed over too, with a reader whose
/// every read fails: [`io::ErrorKind::Unsupported`] when it is encrypted or
/// packed with a method there is no decoder for here (bzip2, zstd,
/// PPMd...), [`io::ErrorKind::InvalidData`] when its local header is
/// damaged; the message says which. A `visit` that never reads it (a
/// readme among pictures) is not affected, and one that does decides
/// whether the error ends the walk: one such file must not cost a whole
/// set, nor vanish without a word. The walk itself fails on a broken
/// central directory and on I/O errors of `source`.
pub fn for_each_file(
    kind: Kind,
    source: &mut dyn Source,
    visit: &mut dyn FnMut(&Entry, &mut dyn Read) -> io::Result<()>,
) -> Result<(), ArchiveError> {
    source.seek(SeekFrom::Start(0))?;
    match kind {
        Kind::Zip => {
            let mut archive = zip::ZipArchive::new(&mut *source)?;
            for index in 0..archive.len() {
                // From the central directory alone: a damaged local header
                // must not hide the entry.
                let (entry, encrypted) = {
                    let meta = archive.by_index_data(index)?;
                    if meta.is_dir() {
                        continue;
                    }
                    let Some(name) = safe_name(&meta.name()?) else {
                        continue;
                    };
                    let entry = Entry {
                        name,
                        size: meta.size(),
                        is_dir: false,
                    };
                    (entry, meta.encrypted())
                };
                if encrypted {
                    visit(&entry, &mut Unreadable::encrypted())?;
                    continue;
                }
                match archive.by_index(index) {
                    Ok(mut file) => visit(&entry, &mut file)?,
                    Err(err) => visit(&entry, &mut Unreadable::from_zip(err)?)?,
                }
            }
        }
        Kind::SevenZip => {
            let mut reader =
                sevenz_rust2::ArchiveReader::new(&mut *source, sevenz_rust2::Password::empty())?;
            reader.for_each_entries(|file, data| {
                if file.is_directory || file.is_anti_item {
                    return Ok(true);
                }
                if let Some(name) = safe_name(&file.name) {
                    let entry = Entry {
                        name,
                        size: file.size,
                        is_dir: false,
                    };
                    visit(&entry, data)?;
                }
                // The rest of the file is decoded and dropped by the block.
                io::copy(data, &mut io::sink())?;
                Ok(true)
            })?;
        }
    }
    Ok(())
}

/// The reader of an entry that is in the archive but cannot be read: every
/// read fails, saying why.
struct Unreadable {
    kind: io::ErrorKind,
    why: String,
}

impl Unreadable {
    fn encrypted() -> Self {
        Self {
            kind: io::ErrorKind::Unsupported,
            why: "the entry is encrypted".to_owned(),
        }
    }

    /// Why zip cannot open one entry, or the error of the whole walk when
    /// it is not about that entry (an I/O error of the archive file).
    fn from_zip(err: zip::result::ZipError) -> Result<Self, ArchiveError> {
        use zip::result::ZipError;
        let (kind, why) = match err {
            ZipError::CompressionMethodNotSupported(id) => {
                let why = match zip::CompressionMethod::name_from_u16(id) {
                    "Unknown" => format!("unsupported compression method {id}"),
                    name => format!("unsupported compression method {id} ({name})"),
                };
                (io::ErrorKind::Unsupported, why)
            }
            ZipError::InvalidPassword => return Ok(Self::encrypted()),
            ZipError::UnsupportedArchive(why) => (
                io::ErrorKind::Unsupported,
                format!("the entry cannot be read: {why}"),
            ),
            // A local header that is missing, cut short or out of range.
            ZipError::InvalidArchive(why) => (
                io::ErrorKind::InvalidData,
                format!("the entry is damaged: {why}"),
            ),
            other => return Err(other.into()),
        };
        Ok(Self { kind, why })
    }
}

impl Read for Unreadable {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(self.kind, self.why.clone()))
    }
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
        crc: StoredCrc,
    },
    Piped(Piped),
}

/// The CRC of a stored entry, checked when it is read front to back.
struct StoredCrc {
    expected: u32,
    /// The bytes hashed so far; `None` once a read skipped some.
    hashed: Option<(u64, crc32fast::Hasher)>,
}

impl StoredCrc {
    fn new(expected: u32) -> Self {
        Self {
            expected,
            hashed: Some((0, crc32fast::Hasher::new())),
        }
    }

    /// Takes the bytes read at `pos`; at the end of the entry, whether they
    /// match.
    fn update(&mut self, pos: u64, bytes: &[u8], size: u64) -> bool {
        let Some((done, hasher)) = &mut self.hashed else {
            return true;
        };
        if *done != pos {
            self.hashed = None;
            return true;
        }
        hasher.update(bytes);
        *done += bytes.len() as u64;
        if *done < size {
            return true;
        }
        let Some((_, hasher)) = self.hashed.take() else {
            return true;
        };
        hasher.finalize() == self.expected
    }
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
    let wanted = safe_name(name).ok_or_else(|| ArchiveError::NotFound(name.to_owned()))?;
    match kind {
        Kind::Zip => {
            let (size, stored_at, crc) = {
                let mut archive = zip::ZipArchive::new(&mut *file)?;
                let index = (0..archive.len())
                    .find(|&i| {
                        archive
                            .by_index_data(i)
                            .ok()
                            .and_then(|e| {
                                e.name()
                                    .ok()
                                    .map(|n| safe_name(&n).as_ref() == Some(&wanted))
                            })
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| ArchiveError::NotFound(wanted.clone()))?;
                let entry = archive.by_index_raw(index)?;
                if entry.encrypted() {
                    return Err(ArchiveError::Encrypted(wanted));
                }
                let stored = entry.compression() == zip::CompressionMethod::Stored;
                (
                    entry.size(),
                    entry.data_start().filter(|_| stored),
                    entry.crc32(),
                )
            };
            Ok(EntryReader {
                size,
                pos: 0,
                inner: match stored_at {
                    Some(start) => Inner::Window {
                        file,
                        start,
                        crc: StoredCrc::new(crc),
                    },
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
                    .find(|f| !f.is_directory && safe_name(&f.name).as_ref() == Some(&wanted))
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
        if buf.is_empty() {
            return Ok(0);
        }
        if self.pos >= self.size {
            // An error the decoder found after the last byte (zip checks
            // the CRC on the read past the end) must not get lost.
            if let Inner::Piped(piped) = &mut self.inner {
                piped.finish(self.size)?;
            }
            return Ok(0);
        }
        let want = usize::try_from(self.size - self.pos)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = match &mut self.inner {
            Inner::Window { file, start, crc } => {
                file.seek(SeekFrom::Start(*start + self.pos))?;
                let n = file.read(&mut buf[..want])?;
                if !crc.update(self.pos, &buf[..n], self.size) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the archive entry is damaged: its CRC does not match",
                    ));
                }
                n
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
            self.file = Some(join(worker.handle)?);
        }
        self.chunk.clear();
        self.chunk_at = 0;
        Ok(())
    }

    /// At the end of the entry: waits for the worker and reports what it
    /// found after the last byte, a CRC error or more data than listed.
    fn finish(&mut self, size: u64) -> io::Result<()> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        let mut result = Ok(());
        while let Ok(message) = worker.rx.recv() {
            match message {
                Ok(block) if block.is_empty() => {}
                Ok(_) => {
                    result = Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} holds more than the {size} bytes it lists", self.name),
                    ));
                    worker.stop.store(true, Ordering::SeqCst);
                    break;
                }
                Err(err) => {
                    result = Err(err);
                    break;
                }
            }
        }
        drop(worker.rx);
        self.file = Some(join(worker.handle)?);
        self.chunk.clear();
        self.chunk_at = 0;
        result
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
        let handle = thread::Builder::new()
            .name("archive-decoder".into())
            .spawn(move || {
                let mut file = file;
                if let Err(err) = decode(kind, &name, &mut file, &tx, &flag) {
                    let _ = tx.send(Err(into_io(err)));
                }
                file
            })?;
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
                // The worker is gone before the entry's end: it panicked,
                // or the entry is shorter than listed.
                Err(_) => {
                    if let Some(worker) = self.worker.take() {
                        self.file = Some(join(worker.handle)?);
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!("{} ends before its listed size", self.name),
                    ));
                }
            }
        }
    }
}

/// The archive file back from a finished worker.
fn join(handle: JoinHandle<Box<dyn Source>>) -> io::Result<Box<dyn Source>> {
    handle
        .join()
        .map_err(|_| io::Error::other("the archive decoder panicked"))
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
                        .and_then(|e| {
                            e.name()
                                .ok()
                                .map(|n| safe_name(&n).as_deref() == Some(name))
                        })
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
                .position(|f| !f.is_directory && safe_name(&f.name).as_deref() == Some(name))
                .ok_or_else(|| ArchiveError::NotFound(name.to_owned()))?;
            let Some(block) = archive.stream_map.file_block_index[index] else {
                // An empty file has no block.
                return Ok(());
            };
            let decoder = sevenz_rust2::BlockDecoder::new(1, block, &archive, &password, file);
            decoder.for_each_entries(&mut |entry, reader| {
                if safe_name(&entry.name).as_deref() == Some(name) {
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

    /// Names and bytes of every file, read in one pass.
    fn walk(kind: Kind, archive: &[u8], read: impl Fn(&str) -> bool) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for_each_file(
            kind,
            &mut Cursor::new(archive.to_vec()),
            &mut |entry, data| {
                let mut bytes = Vec::new();
                if read(&entry.name) {
                    data.read_to_end(&mut bytes)?;
                }
                out.push((entry.name.clone(), bytes));
                Ok(())
            },
        )
        .unwrap();
        out
    }

    #[test]
    fn every_file_in_one_pass() {
        let files: Vec<(String, Vec<u8>)> = (0..40)
            .map(|i| {
                (
                    format!("PS2/SLUS_200.{i:02}/SLUS_200.{i:02}_COV.jpg"),
                    data(500 + i),
                )
            })
            .collect();
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let archive = zip_of(&refs, method);
            // Only every fifth file read; the rest are passed over.
            let seen = walk(Kind::Zip, &archive, |name| {
                name.contains("0_COV") || name.contains("5_COV")
            });
            assert_eq!(seen.len(), 40);
            assert_eq!(seen[5].0, files[5].0);
            assert_eq!(seen[5].1, files[5].1);
            assert!(seen[6].1.is_empty());
        }
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = sevenz_rust2::ArchiveWriter::new(&mut out).unwrap();
            for (name, bytes) in &files {
                writer
                    .push_archive_entry(
                        sevenz_rust2::ArchiveEntry::new_file(name),
                        Some(Cursor::new(bytes.clone())),
                    )
                    .unwrap();
            }
            writer.finish().unwrap();
        }
        let seen = walk(Kind::SevenZip, out.get_ref(), |name| {
            name.ends_with("39_COV.jpg")
        });
        assert_eq!(seen.len(), 40);
        assert_eq!(seen[39].1, files[39].1);
    }

    type Outcome = Result<Vec<u8>, (io::ErrorKind, String)>;

    /// What reading every file in one pass gives: its bytes, or the kind
    /// and message of the error.
    fn read_each(kind: Kind, archive: &[u8]) -> Vec<(String, Outcome)> {
        let mut out = Vec::new();
        for_each_file(
            kind,
            &mut Cursor::new(archive.to_vec()),
            &mut |entry, data| {
                let mut bytes = Vec::new();
                let read = match data.read_to_end(&mut bytes) {
                    Ok(_) => Ok(bytes),
                    Err(err) => Err((err.kind(), err.to_string())),
                };
                out.push((entry.name.clone(), read));
                Ok(())
            },
        )
        .unwrap();
        out
    }

    /// A stored zip of a readme and a cover, and where the central
    /// directory header of the readme starts.
    fn readme_and_cover() -> (Vec<u8>, usize) {
        let archive = zip_of(
            &[
                ("readme.txt", b"read me"),
                ("SLUS_200.62_COV.png", b"cover"),
            ],
            zip::CompressionMethod::Stored,
        );
        assert_eq!(&archive[..4], b"PK\x03\x04");
        let central = archive.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        (archive, central)
    }

    /// The readme fails with `kind` and a message that holds `why`, the
    /// cover next to it reads.
    fn check_unreadable_readme(archive: &[u8], kind: io::ErrorKind, why: &str) {
        let seen = read_each(Kind::Zip, archive);
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0].0, "readme.txt");
        let Err((got, message)) = &seen[0].1 else {
            panic!("the readme read: {:?}", seen[0].1);
        };
        assert_eq!(*got, kind, "{message}");
        assert!(message.contains(why), "{message}");
        assert_eq!(
            seen[1],
            ("SLUS_200.62_COV.png".to_owned(), Ok(b"cover".to_vec()))
        );

        // A visit that does not read it is not affected.
        let seen = walk(Kind::Zip, archive, |name| name.ends_with(".png"));
        assert_eq!(
            seen,
            [
                ("readme.txt".to_owned(), Vec::new()),
                ("SLUS_200.62_COV.png".to_owned(), b"cover".to_vec())
            ]
        );
        // One that passes the error on ends the walk with it.
        let err = for_each_file(
            Kind::Zip,
            &mut Cursor::new(archive.to_vec()),
            &mut |_, data| data.read_to_end(&mut Vec::new()).map(drop),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ArchiveError::Io(e) if e.kind() == kind),
            "{err}"
        );
    }

    #[test]
    fn entries_without_a_decoder_are_handed_over_unreadable() {
        // Method 0xBEEF, which no zip tool assigns, in the local header and
        // the central directory alike.
        let (mut archive, central) = readme_and_cover();
        archive[8..10].copy_from_slice(&0xBEEFu16.to_le_bytes());
        archive[central + 10..central + 12].copy_from_slice(&0xBEEFu16.to_le_bytes());
        check_unreadable_readme(&archive, io::ErrorKind::Unsupported, "48879");
        // Still listed: it is there, only not readable.
        let entries = list(Kind::Zip, &mut Cursor::new(archive)).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn encrypted_entries_are_handed_over_unreadable() {
        // Bit 0 of the flags in both headers.
        let (mut archive, central) = readme_and_cover();
        archive[6] |= 1;
        archive[central + 8] |= 1;
        check_unreadable_readme(&archive, io::ErrorKind::Unsupported, "encrypted");
    }

    #[test]
    fn a_damaged_local_header_costs_only_its_entry() {
        // The central directory is whole, the readme's local header is not.
        let (mut archive, _) = readme_and_cover();
        archive[..4].copy_from_slice(b"XXXX");
        check_unreadable_readme(&archive, io::ErrorKind::InvalidData, "damaged");
        // Listing the archive needs no local header at all.
        let names: Vec<String> = list(Kind::Zip, &mut Cursor::new(archive.clone()))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        // Its header out of the archive's range altogether.
        let (mut archive, central) = readme_and_cover();
        archive[central + 42..central + 46].copy_from_slice(&0x7FFF_0000u32.to_le_bytes());
        check_unreadable_readme(&archive, io::ErrorKind::InvalidData, "damaged");
    }

    /// Reads fail below `from`: the disk goes away under the local headers.
    struct FailsBelow {
        inner: Cursor<Vec<u8>>,
        from: u64,
    }

    impl Read for FailsBelow {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.inner.position() < self.from {
                return Err(io::Error::other("disk gone"));
            }
            self.inner.read(buf)
        }
    }

    impl Seek for FailsBelow {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    #[test]
    fn io_errors_of_the_archive_end_the_walk() {
        let (archive, central) = readme_and_cover();
        let mut source = FailsBelow {
            inner: Cursor::new(archive),
            from: central as u64,
        };
        let mut visited = 0;
        let err = for_each_file(Kind::Zip, &mut source, &mut |_, _| {
            visited += 1;
            Ok(())
        })
        .unwrap_err();
        assert!(
            matches!(&err, ArchiveError::Io(e) if e.to_string().contains("disk gone")),
            "{err}"
        );
        assert_eq!(visited, 0);
    }

    #[test]
    fn seven_zip_anti_items_are_not_files() {
        // An anti item marks a file deleted in an update archive.
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = sevenz_rust2::ArchiveWriter::new(&mut out).unwrap();
            writer
                .push_archive_entry(
                    sevenz_rust2::ArchiveEntry::new_file("SLUS_200.62_COV.jpg"),
                    Some(Cursor::new(b"cover".to_vec())),
                )
                .unwrap();
            let mut anti = sevenz_rust2::ArchiveEntry::new_file("SLUS_200.63_COV.jpg");
            anti.is_anti_item = true;
            writer
                .push_archive_entry(anti, None::<Cursor<Vec<u8>>>)
                .unwrap();
            writer.finish().unwrap();
        }
        let archive = out.into_inner();
        let seen = walk(Kind::SevenZip, &archive, |_| true);
        assert_eq!(
            seen,
            [("SLUS_200.62_COV.jpg".to_owned(), b"cover".to_vec())]
        );
        let entries = list(Kind::SevenZip, &mut Cursor::new(archive)).unwrap();
        assert_eq!(entries.len(), 1);
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

    #[test]
    fn names_stay_inside_the_archive() {
        assert_eq!(
            safe_name("C:\\Games\\x.iso").as_deref(),
            Some("Games/x.iso")
        );
        assert_eq!(safe_name("/abs/../x.iso").as_deref(), Some("abs/x.iso"));
        assert_eq!(safe_name("./a//b.iso").as_deref(), Some("a/b.iso"));
        assert_eq!(safe_name("d:x.iso").as_deref(), Some("x.iso"));
        assert_eq!(
            safe_name("Game: Part 2.iso").as_deref(),
            Some("Game: Part 2.iso")
        );
        assert_eq!(safe_name("C:"), None);
        assert_eq!(safe_name("/../"), None);

        let archive = zip_of(
            &[("C:\\Games\\x.iso", b"x"), ("../up.iso", b"u")],
            zip::CompressionMethod::Stored,
        );
        let entries = list(Kind::Zip, &mut Cursor::new(archive.clone())).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Games/x.iso", "up.iso", "Games"]);
        let mut x = Vec::new();
        open_entry(Kind::Zip, Box::new(Cursor::new(archive)), "Games/x.iso")
            .unwrap()
            .read_to_end(&mut x)
            .unwrap();
        assert_eq!(x, b"x");
    }

    /// The archive with every copy of `content`'s CRC changed, so the data
    /// reads fine but does not match.
    fn bad_crc(archive: Vec<u8>, content: &[u8]) -> Vec<u8> {
        let crc = crc32fast::hash(content).to_le_bytes();
        let mut out = archive;
        let mut at = 0;
        while let Some(found) = out[at..].windows(4).position(|w| w == crc) {
            out[at + found] ^= 0xFF;
            at += found + 4;
        }
        out
    }

    #[test]
    fn damaged_entries_are_reported() {
        // A whole number of blocks: zip reports the CRC on the read past the
        // last byte, which must not get lost.
        let big = data(2 * BLOCK);
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let archive = bad_crc(zip_of(&[("Game.iso", &big)], method), &big);
            let mut reader =
                open_entry(Kind::Zip, Box::new(Cursor::new(archive)), "Game.iso").unwrap();
            let mut all = Vec::new();
            let err = reader.read_to_end(&mut all).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{method:?}: {err}");
        }
    }
}
