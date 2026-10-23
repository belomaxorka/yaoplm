//! Picture sets onto the device (ART-5): OPL Manager's art backups (zip or
//! 7z from archive.org, which the user downloads), folders of pictures,
//! and RiptOPL's and wOPL's `art.tar`.
//!
//! A set is read once, front to back, and only the pictures of games on
//! the device are taken (the owner's decision, 2026-10-23): a backup holds
//! tens of thousands of games. Names are read as OPL names art and as OPL
//! Manager numbers screenshots (`yaoplm_formats::art::pack_picture`),
//! wherever they sit in the set. A file that is not wanted (a type not
//! asked for, a game not on the device, a slot the device fills, a better
//! picture found already) is passed over unread; in a folder it is not
//! even opened.
//!
//! What cannot be read (ADR-0033). Lenient, in a folder and a zip, where
//! each picture is read on its own: a picture that cannot be opened, read
//! or unpacked (a damaged entry, a compression method there is no decoder
//! for, encryption) and a subfolder that cannot be listed are skipped with
//! their path, and the rest of the set is taken. Fatal, ending the job: a
//! tar or 7z stream that breaks, since nothing after the break can be
//! found; a zip whose central directory is broken; a set that cannot be
//! opened at all.
//!
//! Every picture is transcoded to the profile's size like a downloaded
//! one, and the job ends with a plan: nothing is written before the user
//! confirms it, and a replaced picture keeps a copy in the device's backup
//! folder (ADR-0025). OPL Manager's private art server is never asked
//! (rule 3): the files come from the user.

use std::collections::{HashMap, HashSet};
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

/// A file of a folder set, opened on its first read. A file that is not
/// wanted (a type not asked for, another game, a slot the device fills, a
/// better picture found already) is never read, so one of those that
/// cannot be opened (locked, no access) is counted or passed over like the
/// rest instead of reported (ADR-0033).
struct OpenOnRead<'a> {
    path: &'a Path,
    file: Option<File>,
}

impl<'a> OpenOnRead<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, file: None }
    }
}

impl Read for OpenOnRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let file = match &mut self.file {
            Some(file) => file,
            None => self.file.insert(File::open(self.path)?),
        };
        file.read(buf)
    }
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
    /// Slots the device fills already (kept), whatever number of sets
    /// hold a picture for them.
    present: HashSet<(String, ArtKind)>,
    files: u64,
    /// A picture that cannot be read is skipped rather than ending the
    /// set: in a folder and a zip each file is read on its own, while in a
    /// tar or a solid 7z the stream itself is broken.
    lenient: bool,
}

impl Collector<'_> {
    /// Looks at one file of a set; `name` is its path inside the set.
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
        // OPL Manager's backups keep PS1 pictures in `PS1/`, under IDs that
        // PS2 games can have too (research/13 §3); OPL does not look PS1
        // games' pictures up by ID, so they are never the device's.
        if name
            .split(['/', '\\'])
            .any(|part| part.eq_ignore_ascii_case("PS1"))
        {
            self.other_games += 1;
            return Ok(());
        }
        let Some(target) = self.games.get(&serial::lookup_key(&picture.key)) else {
            self.other_games += 1;
            return Ok(());
        };
        let slot = (target.startup.to_lowercase(), picture.kind);
        if !self.params.overwrite && target.present.contains(&picture.kind) {
            self.present.insert(slot);
            return Ok(());
        }
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
        if let Err(err) = data.take(MAX_PICTURE).read_to_end(&mut bytes) {
            if !self.lenient {
                return Err(err);
            }
            self.skip(
                &label,
                Said::notice("pictureUnreadable").with("error", err.to_string()),
            );
            return Ok(());
        }
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

    /// Reads the folder `dir` of the folder set at `base`. Only the set's
    /// own folder must be readable: a subfolder or a file that is not (the
    /// "System Volume Information" of an NTFS drive) is skipped with its
    /// path.
    fn folder(&mut self, set: &str, base: &Path, dir: &Path, depth: usize) -> io::Result<()> {
        // `ART/SLUS_200.62_COV.png`: where a file is in the set.
        let inside = |path: &Path| {
            path.strip_prefix(base)
                .unwrap_or(path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        };
        let listing = match std::fs::read_dir(dir) {
            Ok(listing) => listing,
            Err(err) if depth > 0 => {
                self.skip(
                    &format!("{set}: {}", inside(dir)),
                    Said::notice("unreadable").with("error", err.to_string()),
                );
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let mut entries: Vec<PathBuf> = listing
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                !p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        entries.sort();
        for path in entries {
            let name = inside(&path);
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(err) => {
                    self.skip(
                        &format!("{set}: {name}"),
                        Said::notice("unreadable").with("error", err.to_string()),
                    );
                    continue;
                }
            };
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    self.folder(set, base, &path, depth + 1)?;
                }
            } else if meta.is_file() {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // Only what may be a picture: a folder set can hold the
                // user's whole collection.
                if pack_picture(&file_name).is_some() {
                    self.file(set, &name, meta.len(), &mut OpenOnRead::new(&path))?;
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
        let kind = set_kind(path);
        self.lenient = matches!(kind, Some(SetKind::Folder | SetKind::Archive(Kind::Zip)));
        let result: Result<(), String> = match kind {
            Some(SetKind::Folder) => self.folder(&set, path, path, 0).map_err(|e| e.to_string()),
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
        present: HashSet::new(),
        files: 0,
        lenient: false,
    };
    for path in &params.sources {
        collector.read_set(path)?;
    }
    let present = collector.present.len();
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
    if present > 0 {
        plan.notices
            .push(Notice::info(Said::notice("artPresent").count(present)));
    }
    if collector.other_games > 0 {
        plan.notices.push(Notice::info(
            Said::notice("otherGamesLeftOut").count(collector.other_games),
        ));
    }
    if items.is_empty() && present == 0 {
        plan.notices.retain(|n| n.code != "nothingToWrite");
        plan.notices
            .push(Notice::error(Said::notice("nothingForTheseGames")));
    }
    Ok(JobResult::Plan(Box::new(plans.insert(params.root, plan))))
}
