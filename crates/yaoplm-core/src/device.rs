//! What planners read from a device root before they plan a change.

use std::io;

use yaoplm_device::fs::{DeviceFs, DirEntry, find_entry, list_or_empty};
use yaoplm_device::{FsInfo, RelPath};
use yaoplm_formats::Media;
use yaoplm_formats::games_bin;
use yaoplm_formats::ul_cfg::UlCfg;

use crate::journal;
use crate::plan::{Notice, Removal, Step};
use crate::scan::OPL_FOLDERS;

pub const UL_CFG: &str = "ul.cfg";

pub fn rel(path: &str) -> io::Result<RelPath> {
    RelPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

pub fn join(dir: &RelPath, name: &str) -> io::Result<RelPath> {
    dir.join(name)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// A snapshot of the parts of a root that planners care about.
pub struct DeviceState {
    pub info: FsInfo,
    pub top: Vec<DirEntry>,
    pub cd: Vec<DirEntry>,
    pub dvd: Vec<DirEntry>,
    /// `ul.cfg` exactly as stored, when present.
    pub ul_bytes: Option<Vec<u8>>,
    pub ul: UlCfg,
    /// Journals of operations that did not finish.
    pub unfinished: Vec<String>,
}

impl DeviceState {
    pub fn read(fs: &dyn DeviceFs) -> io::Result<Self> {
        let info = fs.info()?;
        let top = list_or_empty(fs, &RelPath::root())?;
        let folder = |media: Media| -> io::Result<Vec<DirEntry>> {
            match find_entry(&top, media.folder()).filter(|e| e.is_dir) {
                Some(dir) => list_or_empty(fs, &rel(&dir.name)?),
                None => Ok(Vec::new()),
            }
        };
        let (cd, dvd) = (folder(Media::Cd)?, folder(Media::Dvd)?);
        let ul_bytes = match find_entry(&top, UL_CFG).filter(|e| !e.is_dir) {
            Some(entry) => Some(fs.read(&rel(&entry.name)?)?),
            None => None,
        };
        let ul = ul_bytes.as_deref().map(UlCfg::parse).unwrap_or_default();
        let (journals, unreadable) = journal::load_all(fs)?;
        let mut unfinished: Vec<String> = journals.into_iter().map(|j| j.id).collect();
        unfinished.extend(unreadable.into_iter().map(|u| u.file));
        Ok(Self {
            info,
            top,
            cd,
            dvd,
            ul_bytes,
            ul,
            unfinished,
        })
    }

    pub fn entries(&self, media: Media) -> &[DirEntry] {
        match media {
            Media::Cd => &self.cd,
            Media::Dvd => &self.dvd,
        }
    }

    /// The image folder as it is named on the device (`cd` stays `cd`).
    pub fn folder(&self, media: Media) -> io::Result<RelPath> {
        let name = find_entry(&self.top, media.folder())
            .filter(|e| e.is_dir)
            .map_or(media.folder(), |e| e.name.as_str());
        rel(name)
    }

    pub fn has_folder(&self, name: &str) -> bool {
        find_entry(&self.top, name).is_some_and(|e| e.is_dir)
    }

    /// `CreateDir` steps for the standard OPL folders that are missing (LIB-3).
    pub fn missing_folders(&self) -> io::Result<Vec<Step>> {
        OPL_FOLDERS
            .iter()
            .filter(|name| !self.has_folder(name))
            .map(|name| Ok(Step::CreateDir { path: rel(name)? }))
            .collect()
    }

    /// Removing `games.bin` from an image folder (IMP-8), if it is there.
    pub fn drop_cache(&self, media: Media) -> io::Result<Option<Step>> {
        let Some(entry) = find_entry(self.entries(media), games_bin::FILE_NAME) else {
            return Ok(None);
        };
        Ok(Some(Step::Remove {
            path: join(&self.folder(media)?, &entry.name)?,
            len: entry.len,
            what: Removal::Cache,
        }))
    }

    pub fn ul_path(&self) -> io::Result<RelPath> {
        let name = find_entry(&self.top, UL_CFG).map_or(UL_CFG, |e| e.name.as_str());
        rel(name)
    }

    /// A blocker when a journal says an earlier operation is unfinished.
    pub fn unfinished_notice(&self) -> Option<Notice> {
        self.unfinished.first().map(|id| {
            Notice::error(
                "unfinishedOperation",
                format!(
                    "an earlier operation on this device did not finish (journal {id}); recover it first"
                ),
            )
        })
    }
}

/// Rounds `len` up to whole clusters.
pub fn on_disk(len: u64, cluster: u32) -> u64 {
    let cluster = u64::from(cluster.max(1));
    len.div_ceil(cluster) * cluster
}
