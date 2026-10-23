//! A FAT32 image file as a device, through the `fatfs` crate.
//!
//! Imports into it exercise a real FAT32 implementation: long file names,
//! case-insensitive lookups, cluster allocation and the on-disk directory
//! layout. Never a real drive (CLAUDE.md rule 1).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions};
use yaoplm_device::fs::{DeviceFile, DeviceFs, DirEntry, FileMeta, FsInfo};
use yaoplm_device::{FsKind, RelPath};

type Fat = FileSystem<File>;
type Job = Box<dyn FnOnce(&mut Option<Fat>) + Send>;

/// A FAT32 volume in an image file. `fatfs` volumes are not `Send`, so one
/// thread owns the mounted volume and runs every operation; clones share it.
#[derive(Clone)]
pub struct FatImage {
    path: PathBuf,
    jobs: mpsc::Sender<Job>,
}

impl std::fmt::Debug for FatImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FatImage")
            .field("path", &self.path)
            .finish()
    }
}

/// The volume ID of every image: the owner's test stick has the same one,
/// which keeps fingerprints in tests realistic.
pub const VOLUME_ID: u32 = 0x7E1C_D2C5;

fn mount(path: &Path) -> io::Result<Fat> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    FileSystem::new(file, FsOptions::new())
}

fn not_mounted() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "image not mounted")
}

impl FatImage {
    /// Formats a new FAT32 image of `size` bytes (at least 40 MiB, so FAT32
    /// gets its 65,525 clusters with 512-byte clusters).
    pub fn create(path: &Path, size: u64) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.set_len(size)?;
        fatfs::format_volume(
            &mut file,
            FormatVolumeOptions::new()
                .fat_type(FatType::Fat32)
                .bytes_per_cluster(512)
                .volume_id(VOLUME_ID)
                .volume_label(*b"YAOPLM TEST"),
        )?;
        drop(file);
        Self::open(path)
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let image = path.to_path_buf();
        thread::spawn(move || {
            let mut fs = match mount(&image) {
                Ok(fs) => {
                    let _ = ready_tx.send(Ok(()));
                    Some(fs)
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            for job in inbox {
                job(&mut fs);
            }
            if let Some(fs) = fs {
                let _ = fs.unmount();
            }
        });
        ready_rx.recv().map_err(|_| not_mounted())??;
        Ok(Self {
            path: path.to_path_buf(),
            jobs,
        })
    }

    fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Option<Fat>) -> io::Result<T> + Send + 'static,
    ) -> io::Result<T> {
        let (reply, answer) = mpsc::channel();
        let job: Job = Box::new(move |fs| {
            let _ = reply.send(job(fs));
        });
        self.jobs.send(job).map_err(|_| not_mounted())?;
        answer.recv().map_err(|_| not_mounted())?
    }

    fn with<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Fat) -> io::Result<T> + Send + 'static,
    ) -> io::Result<T> {
        self.run(move |fs| f(fs.as_ref().ok_or_else(not_mounted)?))
    }

    /// Unmounts and mounts again: anything not flushed to the image is lost.
    pub fn remount(&self) -> io::Result<()> {
        let path = self.path.clone();
        self.run(move |fs| {
            if let Some(old) = fs.take() {
                old.unmount()?;
            }
            *fs = Some(mount(&path)?);
            Ok(())
        })
    }

    /// Reads a whole file, for assertions.
    pub fn read_file(&self, path: &str) -> io::Result<Vec<u8>> {
        let path = path.to_owned();
        self.with(move |fs| {
            let mut out = Vec::new();
            fs.root_dir().open_file(&path)?.read_to_end(&mut out)?;
            Ok(out)
        })
    }
}

fn find(fs: &Fat, path: &RelPath) -> io::Result<Option<FileMeta>> {
    if path.is_root() {
        return Ok(Some(FileMeta {
            is_dir: true,
            len: 0,
        }));
    }
    let parent = path.parent().unwrap_or_default();
    let dir = if parent.is_root() {
        fs.root_dir()
    } else {
        match fs.root_dir().open_dir(parent.as_str()) {
            Ok(dir) => dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        }
    };
    let name = path.file_name();
    for entry in dir.iter() {
        let entry = entry?;
        if entry.file_name().eq_ignore_ascii_case(name) {
            return Ok(Some(FileMeta {
                is_dir: entry.is_dir(),
                len: entry.len(),
            }));
        }
    }
    Ok(None)
}

struct FatFile {
    image: FatImage,
    path: String,
    pos: u64,
}

impl Write for FatFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (path, pos, data) = (self.path.clone(), self.pos, buf.to_vec());
        self.image.with(move |fs| {
            let mut file = fs.root_dir().open_file(&path)?;
            file.seek(SeekFrom::Start(pos))?;
            file.write_all(&data)?;
            file.flush()
        })?;
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for FatFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let path = self.path.clone();
        let len = self
            .image
            .with(move |fs| fs.root_dir().open_file(&path)?.seek(SeekFrom::End(0)))?;
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        let target = target
            .filter(|t| *t <= len)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad seek"))?;
        self.pos = target;
        Ok(target)
    }
}

impl DeviceFile for FatFile {
    fn preallocate(&mut self, _len: u64) -> io::Result<()> {
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl DeviceFs for FatImage {
    fn describe(&self) -> String {
        self.path.display().to_string()
    }

    fn info(&self) -> io::Result<FsInfo> {
        self.with(|fs| {
            let stats = fs.stats()?;
            Ok(FsInfo {
                kind: FsKind::Fat32,
                free_bytes: u64::from(stats.free_clusters()) * u64::from(stats.cluster_size()),
                cluster_size: stats.cluster_size(),
                remote: false,
                fingerprint: format!("{:08X}|FAT32|fatfs", fs.volume_id()),
            })
        })
    }

    fn metadata(&self, path: &RelPath) -> io::Result<Option<FileMeta>> {
        let path = path.clone();
        self.with(move |fs| find(fs, &path))
    }

    fn list(&self, dir: &RelPath) -> io::Result<Vec<DirEntry>> {
        let dir = dir.clone();
        self.with(move |fs| {
            let dir = if dir.is_root() {
                fs.root_dir()
            } else {
                fs.root_dir().open_dir(dir.as_str())?
            };
            let mut entries = Vec::new();
            for entry in dir.iter() {
                let entry = entry?;
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                entries.push(DirEntry {
                    name,
                    is_dir: entry.is_dir(),
                    len: entry.len(),
                });
            }
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(entries)
        })
    }

    fn read(&self, path: &RelPath) -> io::Result<Vec<u8>> {
        self.read_file(path.as_str())
    }

    fn create(&self, path: &RelPath) -> io::Result<Box<dyn DeviceFile>> {
        let target = path.clone();
        self.with(move |fs| {
            let path = &target;
            if find(fs, path)?.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{path} exists"),
                ));
            }
            fs.root_dir().create_file(path.as_str()).map(drop)
        })?;
        Ok(Box::new(FatFile {
            image: self.clone(),
            path: path.as_str().to_owned(),
            pos: 0,
        }))
    }

    fn create_dir(&self, path: &RelPath) -> io::Result<()> {
        let path = path.clone();
        self.with(move |fs| fs.root_dir().create_dir(path.as_str()).map(drop))
    }

    fn rename(&self, from: &RelPath, to: &RelPath) -> io::Result<()> {
        let (from, to) = (from.clone(), to.clone());
        self.with(move |fs| {
            let root = fs.root_dir();
            let same_entry = from.as_str().eq_ignore_ascii_case(to.as_str());
            if !same_entry && find(fs, &to)?.is_some() {
                root.remove(to.as_str())?;
            }
            root.rename(from.as_str(), &root, to.as_str())
        })
    }

    fn remove_file(&self, path: &RelPath) -> io::Result<()> {
        let path = path.clone();
        self.with(move |fs| fs.root_dir().remove(path.as_str()))
    }

    fn remove_dir(&self, path: &RelPath) -> io::Result<()> {
        self.remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use yaoplm_device::fs::write_atomic;

    use super::*;

    #[test]
    fn writes_survive_a_remount() {
        let dir = tempfile::tempdir().unwrap();
        let image = FatImage::create(&dir.path().join("fat32.img"), 48 << 20).unwrap();
        let rel = |p: &str| RelPath::new(p).unwrap();
        image.create_dir(&rel("DVD")).unwrap();
        let name = rel("DVD/SLUS_200.62.Grand Theft Auto III.iso");
        let mut file = image.create(&name).unwrap();
        file.write_all(&[7u8; 5000]).unwrap();
        drop(file);
        write_atomic(&image, &rel("ul.cfg"), &[1u8; 64]).unwrap();
        image.remount().unwrap();
        assert_eq!(image.read_file(name.as_str()).unwrap(), vec![7u8; 5000]);
        assert_eq!(
            image
                .metadata(&rel("dvd/slus_200.62.grand theft auto iii.ISO"))
                .unwrap(),
            Some(FileMeta {
                is_dir: false,
                len: 5000
            })
        );
        let info = image.info().unwrap();
        assert_eq!(info.kind, FsKind::Fat32);
        assert!(info.fingerprint.starts_with("7E1CD2C5"));
        assert_eq!(image.list(&RelPath::root()).unwrap().len(), 2);
    }
}
