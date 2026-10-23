//! Picture sets onto the device (ART-5): OPL Manager's art backups (zip or
//! 7z from archive.org, which the user downloads), folders of pictures,
//! and RiptOPL's and wOPL's `art.tar`.
//!
//! A set is read once, front to back, and only the pictures of games on
//! the device are taken (the owner's decision, 2026-10-23): a backup holds
//! tens of thousands of games. Names are read as OPL names art and as OPL
//! Manager numbers screenshots (`yaoplm_formats::art::pack_picture`),
//! wherever they sit in the set. Every picture is transcoded to the
//! profile's size like a downloaded one, and the job ends with a plan:
//! nothing is written before the user confirms it, and a replaced picture
//! keeps a copy in the device's backup folder (ADR-0025). OPL Manager's
//! private art server is never asked (rule 3): the files come from the
//! user.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use yaoplm_core::art::{ArtItem, plan_set_art};
use yaoplm_core::plan::{Notice, Skipped};
use yaoplm_core::{LoaderProfile, Phrase as Said};
use yaoplm_device::{HostFs, HostSources};
use yaoplm_formats::archive::{self, Kind};
use yaoplm_formats::art::{ArtKind, ArtSubject, pack_picture, recommended_size};
use yaoplm_formats::{serial, tar};

use crate::art::{ArtLogState, Net, Target, targets};
use crate::jobs::{JobContext, JobLogEntry, JobResult, now_ms};
use crate::library::LoaderProfile as WireProfile;
use crate::paths::display_path;
use crate::plans::PlanStore;
use crate::{ApiError, ErrorCode};

/// A picture larger than this is not a cover or a screenshot.
const MAX_PICTURE: u64 = 16 << 20;
/// How deep a folder set is searched.
const MAX_DEPTH: usize = 8;

pub(crate) struct PackParams {
    pub root: PathBuf,
    pub profile: WireProfile,
    /// Folders, archives or single pictures, as the user picked them.
    pub sources: Vec<PathBuf>,
    pub kinds: Vec<ArtKind>,
    /// Replace pictures the device has (they move to the backup folder).
    pub overwrite: bool,
}

/// What a set holds, in the order to look at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetKind {
    Folder,
    Archive(Kind),
    Tar,
    Picture,
}

fn set_kind(path: &Path) -> Option<SetKind> {
    if path.is_dir() {
        return Some(SetKind::Folder);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "tar" => Some(SetKind::Tar),
        e if yaoplm_formats::art::PACK_EXTENSIONS.contains(&e) => Some(SetKind::Picture),
        e => Kind::from_extension(e).map(SetKind::Archive),
    }
}

/// Checks the sources a request names before the job starts.
pub(crate) fn check_sources(sources: &[PathBuf]) -> Result<(), ApiError> {
    if sources.is_empty() {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("noPictureSet"),
        ));
    }
    for path in sources {
        if set_kind(path).is_none() {
            return Err(ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("notAPictureSet").with("path", display_path(path)),
            ));
        }
    }
    Ok(())
}

/// Lower is better: what OPL 1.2 loads first, then lossless.
fn rank(extension: &str) -> u8 {
    match extension {
        "png" => 0,
        "bmp" => 1,
        "jpg" | "jpeg" => 2,
        _ => 3,
    }
}

struct Chosen {
    rank: u8,
    item: ArtItem,
}

/// Collects the pictures of a set while it is read.
struct Collector<'a> {
    ctx: &'a JobContext,
    net: &'a Net,
    profile: LoaderProfile,
    params: &'a PackParams,
    games: HashMap<String, Target>,
    chosen: HashMap<(String, ArtKind), Chosen>,
    skipped: Vec<Skipped>,
    /// Pictures of games that are not on the device.
    other_games: usize,
    /// Pictures the device has already (kept).
    present: usize,
    files: u64,
}

impl Collector<'_> {
    /// Looks at one file of a set: `label` names it for the log.
    fn file(&mut self, set: &str, name: &str, size: u64, data: &mut dyn Read) -> io::Result<()> {
        self.files += 1;
        if self.ctx.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let file_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let Some(picture) = pack_picture(file_name) else {
            return Ok(());
        };
        if !self.params.kinds.contains(&picture.kind) {
            return Ok(());
        }
        let Some(target) = self.games.get(&serial::lookup_key(&picture.key)) else {
            self.other_games += 1;
            return Ok(());
        };
        if !self.params.overwrite && target.present.contains(&picture.kind) {
            self.present += 1;
            return Ok(());
        }
        let slot = (target.startup.to_lowercase(), picture.kind);
        let rank = rank(&picture.extension);
        if self.chosen.get(&slot).is_some_and(|c| c.rank <= rank) {
            return Ok(());
        }
        self.ctx.progress(self.files, None, Some(&target.title));
        let label = format!("{set}: {name}");
        let mut bytes = Vec::new();
        if size > MAX_PICTURE {
            self.skip(&label, Said::error("imageTooLarge").with("path", &label));
            return Ok(());
        }
        data.take(MAX_PICTURE).read_to_end(&mut bytes)?;
        let size = recommended_size(
            self.profile,
            ArtSubject::Game {
                startup: &target.startup,
            },
            picture.kind,
        );
        let service = self
            .net
            .service()
            .map_err(|e| io::Error::other(e.message))?;
        match service.import_local(&bytes, picture.kind, size, &label) {
            Ok(cover) => {
                self.ctx.log(JobLogEntry::Art {
                    at: now_ms(),
                    startup: target.startup.clone(),
                    title: target.title.clone(),
                    art: picture.kind.suffix().to_owned(),
                    state: ArtLogState::Found,
                    source: Some(set.to_owned()),
                    detail: Some(name.to_owned()),
                    detail_phrase: None,
                });
                let item = ArtItem {
                    startup: target.startup.clone(),
                    title: target.title.clone(),
                    media: target.media,
                    format: target.format.clone(),
                    kind: picture.kind,
                    file: cover.png,
                    origin: label,
                };
                self.chosen.insert(slot, Chosen { rank, item });
            }
            Err(err) => self.skip(
                &label,
                Said::notice("pictureUnreadable").with("error", err.to_string()),
            ),
        }
        Ok(())
    }

    fn skip(&mut self, label: &str, reason: Said) {
        self.skipped.push(Skipped {
            path: PathBuf::from(label),
            reason: Notice::warning(reason),
        });
    }

    fn folder(&mut self, set: &str, dir: &Path, depth: usize) -> io::Result<()> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                !p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        entries.sort();
        for path in entries {
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    self.folder(set, &path, depth + 1)?;
                }
            } else if meta.is_file() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // Open only what may be a picture: a folder set can hold
                // the user's whole collection.
                if pack_picture(&name).is_some() {
                    let mut file = File::open(&path)?;
                    self.file(set, &name, meta.len(), &mut file)?;
                }
            }
        }
        Ok(())
    }

    fn read_set(&mut self, path: &Path) -> Result<(), ApiError> {
        let set = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display_path(path));
        let unreadable = |err: &dyn std::fmt::Display| {
            ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("pictureSetUnreadable")
                    .with("path", display_path(path))
                    .with("error", err.to_string()),
            )
        };
        let result: Result<(), String> = match set_kind(path) {
            Some(SetKind::Folder) => self.folder(&set, path, 0).map_err(|e| e.to_string()),
            Some(SetKind::Picture) => File::open(path)
                .and_then(|mut f| {
                    let size = f.metadata()?.len();
                    self.file(&set, &set, size, &mut f)
                })
                .map_err(|e| e.to_string()),
            Some(SetKind::Tar) => File::open(path)
                .map_err(tar::TarError::Io)
                .and_then(|f| {
                    tar::for_each_file(BufReader::new(f), &mut |entry, data| {
                        self.file(&set, &entry.name, entry.size, data)
                    })
                })
                .map_err(|e| e.to_string()),
            Some(SetKind::Archive(kind)) => File::open(path)
                .map_err(archive::ArchiveError::from)
                .and_then(|mut f| {
                    archive::for_each_file(kind, &mut f, &mut |entry, data| {
                        self.file(&set, &entry.name, entry.size, data)
                    })
                })
                .map_err(|e| e.to_string()),
            None => Err("not a picture set".to_owned()),
        };
        if self.ctx.is_cancelled() {
            return Err(ApiError::cancelled());
        }
        result.map_err(|e| unreadable(&e))
    }
}

/// The body of an `ImportArt` job.
pub(crate) fn import_job(
    ctx: &JobContext,
    net: &Net,
    plans: &PlanStore,
    params: PackParams,
) -> Result<JobResult, ApiError> {
    let profile: LoaderProfile = params.profile.into();
    let games: HashMap<String, Target> = targets(&params.root, profile)?
        .into_iter()
        .map(|t| (serial::lookup_key(&t.startup), t))
        .collect();
    ctx.progress(0, None, None);
    let mut collector = Collector {
        ctx,
        net,
        profile,
        params: &params,
        games,
        chosen: HashMap::new(),
        skipped: Vec::new(),
        other_games: 0,
        present: 0,
        files: 0,
    };
    for path in &params.sources {
        collector.read_set(path)?;
    }
    let mut chosen: Vec<Chosen> = collector.chosen.into_values().collect();
    chosen.sort_by(|a, b| {
        (a.item.title.as_str(), a.item.kind.suffix())
            .cmp(&(b.item.title.as_str(), b.item.kind.suffix()))
    });
    let items: Vec<ArtItem> = chosen.into_iter().map(|c| c.item).collect();
    let fs = HostFs::new(&params.root).map_err(ApiError::internal_error)?;
    let mut plan = plan_set_art(&fs, &HostSources, &items, params.overwrite)
        .map_err(ApiError::internal_error)?;
    plan.skipped.extend(collector.skipped);
    if collector.present > 0 {
        plan.notices.push(Notice::info(
            Said::notice("artPresent").count(collector.present),
        ));
    }
    if collector.other_games > 0 {
        plan.notices.push(Notice::info(
            Said::notice("otherGamesLeftOut").count(collector.other_games),
        ));
    }
    if items.is_empty() && collector.present == 0 {
        plan.notices.retain(|n| n.code != "nothingToWrite");
        plan.notices
            .push(Notice::error(Said::notice("nothingForTheseGames")));
    }
    Ok(JobResult::Plan(Box::new(plans.insert(params.root, plan))))
}
