//! Converting games on the device between ISO, ZSO and `ul` (GAME-4).
//!
//! One transaction per game. The new form is written next to the old one:
//! a ZSO compressed from the ISO, an ISO decompressed from the ZSO, `ul`
//! chunks cut from the image plus their `ul.cfg` record, or an image joined
//! from the chunks with the record taken out. The old form is deleted only
//! after the commit point: a crash before it removes the half-made new
//! file and leaves the game as it was, a crash after it finishes the
//! deletion (ADR-0021). Deleting the old image is deleting the user's data,
//! so the plan needs `confirmDelete` like any other.
//!
//! The game ID never changes, so CFG, art, cheats and VMC keep working
//! (research/02 §3). Rules from OPL (research/02 §4-§5, §12,
//! ps2homebrew/Open-PS2-Loader@b3a21997): ZSO needs OPL 1.2.0 beta; a ZSO
//! is read by its `ZISO` magic and needs the lowercase `.zso` extension
//! for its label; `ul` chunks are exactly 1 GiB but the last, named by
//! `USBA_crc32` of a title of at most 32 ASCII characters, at most 10 over
//! SMB; FAT32 holds files up to 4 GiB - 1. What Orbit's `zso.service.ts`
//! does without a journal (compress, then unlink the ISO) is the same
//! order, with a rollback on top.

use std::collections::HashSet;
use std::io::{self, Read};

use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, find_entry};
use yaoplm_formats::image::DiscImage;
use yaoplm_formats::iso_name::{self, ImageExtension, NameStyle, sanitize_title};
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_formats::ul_cfg::{self, UlRecord, split_plan};
use yaoplm_formats::usba_crc32::usba_crc32;
use yaoplm_formats::{LoaderProfile, Media, zso};

use crate::device::{DeviceState, join, on_disk, rel};
use crate::import::{image_name, ul_title};
use crate::ops::{Found, FoundKind, GameRef, OpError, find_game, game_info, new_plan};
use crate::plan::{
    Bytes, Expect, Notice, Plan, PlanKind, Removal, Skipped, Source, SourceFile, SourceKind, Step,
    Transaction,
};

/// What a game becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Iso,
    Zso,
    Ul,
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Self::Iso => "ISO",
            Self::Zso => "ZSO",
            Self::Ul => "UL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConvertOptions {
    pub profile: LoaderProfile,
    /// How an image made from `ul` chunks is named; `ID.Title` by default.
    pub name_style: Option<NameStyle>,
}

/// Kept free for the journal and directory growth, as in imports.
const RESERVE: u64 = 1 << 20;

struct Planner<'a> {
    fs: &'a dyn DeviceFs,
    state: DeviceState,
    options: &'a ConvertOptions,
    plan: Plan,
    /// `ul.cfg` after the transactions planned so far.
    ul_next: Option<Vec<u8>>,
    /// Names claimed by this plan, lower case.
    claimed: HashSet<String>,
}

fn skip(what: &str, code: &str, message: impl Into<String>) -> Skipped {
    Skipped {
        path: what.into(),
        reason: Notice::warning(code, message),
    }
}

fn device_source(kind: SourceKind, files: Vec<(RelPath, u64)>, len: u64) -> Source {
    Source {
        path: files
            .first()
            .map(|(p, _)| p.as_str().into())
            .unwrap_or_default(),
        kind,
        len,
        files: files
            .into_iter()
            .map(|(path, len)| SourceFile {
                path: path.as_str().into(),
                len,
                modified: None,
            })
            .collect(),
        on_device: true,
    }
}

impl Planner<'_> {
    fn ul_current(&self) -> Vec<u8> {
        self.ul_next
            .clone()
            .or_else(|| self.state.ul_bytes.clone())
            .unwrap_or_default()
    }

    /// `ul.cfg` with `record` inserted before a ragged tail.
    fn ul_with(&mut self, record: &UlRecord) -> io::Result<Step> {
        let current = self.ul_current();
        let mut next = current.clone();
        let aligned = next.len() - next.len() % ul_cfg::RECORD_SIZE;
        next.splice(aligned..aligned, record.as_bytes().iter().copied());
        self.ul_step(current, next)
    }

    /// `ul.cfg` without `record`, found by its bytes: earlier transactions
    /// may have moved it.
    fn ul_without(&mut self, record: &UlRecord) -> Result<Step, OpError> {
        let current = self.ul_current();
        let at = current
            .as_chunks::<{ ul_cfg::RECORD_SIZE }>()
            .0
            .iter()
            .position(|r| r == record.as_bytes())
            .ok_or_else(|| {
                OpError::NotFound(format!("ul.cfg no longer lists {}", record.name()))
            })?;
        let mut next = current.clone();
        let start = at * ul_cfg::RECORD_SIZE;
        next.drain(start..start + ul_cfg::RECORD_SIZE);
        Ok(self.ul_step(current, next)?)
    }

    fn ul_step(&mut self, current: Vec<u8>, next: Vec<u8>) -> io::Result<Step> {
        let expect = if self.ul_next.is_none() && self.state.ul_bytes.is_none() {
            Expect::Absent
        } else {
            Expect::Content(Bytes(current))
        };
        self.ul_next = Some(next.clone());
        Ok(Step::PutFile {
            path: self.state.ul_path()?,
            content: Bytes(next),
            expect,
        })
    }

    fn claim(&mut self, path: &RelPath) -> bool {
        self.claimed.insert(path.as_str().to_lowercase())
    }

    /// Drops `games.bin` of `media`'s folder once per plan (IMP-8).
    fn drop_cache(&mut self, tx: &mut Transaction, media: Media) -> io::Result<()> {
        if self.claimed.insert(format!("drop:{}", media.folder()))
            && let Some(step) = self.state.drop_cache(media)?
        {
            tx.then(step);
        }
        Ok(())
    }

    /// The ISO bytes a ZSO on the device yields.
    fn zso_iso_len(&self, path: &RelPath) -> Result<u64, String> {
        let reader = self.fs.open(path).map_err(|e| e.to_string())?;
        let image = DiscImage::open(reader).map_err(|e| e.to_string())?;
        Ok(image.sector_count() * SECTOR_SIZE as u64)
    }

    fn convert(&mut self, game: &GameRef, target: Target) -> Result<Transaction, Skipped> {
        let found = find_game(self.fs, &self.state, game)
            .map_err(|e| skip(&describe(game), "notFound", e.to_string()))?;
        let what = describe(game);
        if target == Target::Zso && !self.options.profile.reads_zso() {
            return Err(skip(
                &what,
                "zsoUnsupportedByProfile",
                "OPL 1.1.0 cannot read ZSO",
            ));
        }
        let Found {
            media,
            kind,
            startup,
        } = found;
        let startup = startup
            .ok_or_else(|| skip(&what, "noStartup", "cannot read the game ID from the image"))?;
        match kind {
            FoundKind::Image { path, name, len } => {
                let current = match name.extension {
                    ImageExtension::Iso => Target::Iso,
                    ImageExtension::Zso => Target::Zso,
                };
                if current == target {
                    return Err(skip(
                        &what,
                        "alreadyThatFormat",
                        format!("the game is already a {}", target.label()),
                    ));
                }
                let is_zso = self
                    .fs
                    .open(&path)
                    .ok()
                    .and_then(|mut r| {
                        let mut magic = [0u8; 4];
                        r.read_exact(&mut magic).ok().map(|()| magic == zso::MAGIC)
                    })
                    .unwrap_or(false);
                let (source, iso_len) = if is_zso {
                    let iso_len = self.zso_iso_len(&path).map_err(|e| {
                        skip(&what, "unreadable", format!("cannot read the ZSO: {e}"))
                    })?;
                    (
                        device_source(SourceKind::Zso, vec![(path.clone(), len)], iso_len),
                        iso_len,
                    )
                } else {
                    (
                        device_source(SourceKind::Raw, vec![(path.clone(), len)], len),
                        len,
                    )
                };
                if !iso_len.is_multiple_of(SECTOR_SIZE as u64) || iso_len == 0 {
                    return Err(skip(
                        &what,
                        "partialSector",
                        format!("{iso_len} bytes is not a whole number of 2048-byte sectors"),
                    ));
                }
                let title = name.title.clone();
                let mut tx = match target {
                    Target::Zso | Target::Iso => {
                        self.image_to_image(&what, &path, &startup, &title, media, target, source)?
                    }
                    Target::Ul => {
                        self.image_to_ul(&what, &startup, &title, media, iso_len, source)?
                    }
                };
                tx.then(Step::Remove {
                    path: path.clone(),
                    len,
                    what: Removal::UserData,
                });
                self.drop_cache(&mut tx, media)
                    .map_err(|e| skip(&what, "badName", e.to_string()))?;
                Ok(tx)
            }
            FoundKind::Ul { record, .. } => {
                if target == Target::Ul {
                    return Err(skip(&what, "alreadyThatFormat", "the game is already ul"));
                }
                self.ul_to_image(&what, &record, &startup, target)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn image_to_image(
        &mut self,
        what: &str,
        old: &RelPath,
        startup: &str,
        title: &str,
        media: Media,
        target: Target,
        source: Source,
    ) -> Result<Transaction, Skipped> {
        let ext = if target == Target::Zso {
            ".zso"
        } else {
            ".iso"
        };
        let file_name = old.file_name();
        let stem = &file_name[..file_name.len().saturating_sub(4)];
        let new_name = format!("{stem}{ext}");
        let folder = old.parent().unwrap_or_default();
        let new_path =
            join(&folder, &new_name).map_err(|e| skip(what, "badName", e.to_string()))?;
        if find_entry(self.state.entries(media), &new_name).is_some() || !self.claim(&new_path) {
            return Err(skip(
                what,
                "nameTaken",
                format!("{new_path} already exists on the device"),
            ));
        }
        let iso_len = source.len;
        let mut tx = Transaction::new(format!("Convert {file_name} to {}", target.label()));
        tx.game = Some(game_info(title, Some(startup), media, target.label()));
        self.plan.sources.push(source);
        let index = self.plan.sources.len() - 1;
        match target {
            Target::Zso => {
                tx.push(Step::WriteZso {
                    path: new_path,
                    source: index,
                    max_len: zso::max_output(iso_len),
                });
                if self
                    .state
                    .info
                    .kind
                    .max_file_size()
                    .is_some_and(|m| iso_len > m)
                {
                    tx.notices.push(Notice::warning(
                        "zsoMayNotFit",
                        "FAT32 holds files up to 4 GiB: if the ZSO comes out larger, the conversion is undone",
                    ));
                }
            }
            Target::Iso => {
                if self
                    .state
                    .info
                    .kind
                    .max_file_size()
                    .is_some_and(|m| iso_len > m)
                {
                    return Err(skip(
                        what,
                        "tooLargeForFat32",
                        "the ISO would be over 4 GiB, more than FAT32 holds; convert to ul instead",
                    ));
                }
                tx.push(Step::WriteFile {
                    path: new_path,
                    source: index,
                    offset: 0,
                    len: iso_len,
                });
            }
            Target::Ul => {}
        }
        tx.notices.push(Notice::info(
            "oldImageDeleted",
            format!(
                "{file_name} is deleted once the {} is complete",
                target.label()
            ),
        ));
        Ok(tx)
    }

    fn image_to_ul(
        &mut self,
        what: &str,
        startup: &str,
        title: &str,
        media: Media,
        iso_len: u64,
        source: Source,
    ) -> Result<Transaction, Skipped> {
        let mut name = ul_title(title);
        if name.is_empty() {
            name = ul_title(startup);
        }
        let chunks = split_plan(iso_len).map_err(|e| skip(what, "cannotSplit", e.to_string()))?;
        if self.state.info.remote && !ul_cfg::fits_smb(chunks.len()) {
            return Err(skip(
                what,
                "ulTooManyPartsForSmb",
                format!("{} ul chunks: OPL opens at most 10 over SMB", chunks.len()),
            ));
        }
        let record = UlRecord::new(&name, startup, chunks.len(), media)
            .map_err(|e| skip(what, "cannotSplit", format!("cannot store as ul: {e}")))?;
        let crc = usba_crc32(name.as_bytes());
        let current = ul_cfg::UlCfg::parse(&self.ul_current());
        if current
            .records
            .iter()
            .any(|r| r.name_crc() == crc && r.startup_bytes() == startup.as_bytes())
        {
            return Err(skip(
                what,
                "alreadyOnDevice",
                format!("ul.cfg already lists {name} ({startup})"),
            ));
        }
        let mut tx = Transaction::new(format!("Convert {title} into {} ul chunks", chunks.len()));
        tx.game = Some(game_info(&name, Some(startup), media, "UL"));
        if name != title {
            tx.notices.push(Notice::info(
                "ulTitle",
                format!("ul.cfg holds at most 32 ASCII characters: the title is {name:?}"),
            ));
        }
        self.plan.sources.push(source);
        let index = self.plan.sources.len() - 1;
        for chunk in &chunks {
            let path = rel(&record.chunk_name(chunk.part))
                .map_err(|e| skip(what, "badName", e.to_string()))?;
            if find_entry(&self.state.top, path.as_str()).is_some() || !self.claim(&path) {
                return Err(skip(
                    what,
                    "nameTaken",
                    format!("{path} already exists on the device"),
                ));
            }
            tx.push(Step::WriteFile {
                path,
                source: index,
                offset: chunk.offset,
                len: chunk.len,
            });
        }
        let step = self
            .ul_with(&record)
            .map_err(|e| skip(what, "badName", e.to_string()))?;
        tx.push(step);
        tx.notices.push(Notice::info(
            "oldImageDeleted",
            "the image is deleted once the chunks and ul.cfg are complete",
        ));
        Ok(tx)
    }

    fn ul_to_image(
        &mut self,
        what: &str,
        record: &UlRecord,
        startup: &str,
        target: Target,
    ) -> Result<Transaction, Skipped> {
        let mut parts = Vec::new();
        for part in 0..record.parts() {
            let chunk = record.chunk_name(part);
            let entry = find_entry(&self.state.top, &chunk)
                .filter(|e| !e.is_dir)
                .ok_or_else(|| skip(what, "ulPartMissing", format!("chunk {chunk} is missing")))?;
            let path = rel(&entry.name).map_err(|e| skip(what, "badName", e.to_string()))?;
            parts.push((path, entry.len));
        }
        let iso_len: u64 = parts.iter().map(|(_, len)| len).sum();
        if target == Target::Iso
            && self
                .state
                .info
                .kind
                .max_file_size()
                .is_some_and(|m| iso_len > m)
        {
            return Err(skip(
                what,
                "tooLargeForFat32",
                "the ISO would be over 4 GiB, more than FAT32 holds",
            ));
        }
        let media = record.media();
        let title = sanitize_title(&record.name());
        let title = if title.is_empty() {
            startup.to_owned()
        } else {
            title
        };
        let ext = if target == Target::Zso {
            ImageExtension::Zso
        } else {
            ImageExtension::Iso
        };
        let mut notices = Vec::new();
        let style = self.options.name_style.unwrap_or(NameStyle::WithStartup);
        let name = image_name(startup, &title, ext, style, &mut notices)
            .map_err(|e| skip(what, "badName", e.to_string()))?;
        let folder = self
            .state
            .folder(media)
            .map_err(|e| skip(what, "badName", e.to_string()))?;
        let target_path = join(&folder, &name).map_err(|e| skip(what, "badName", e.to_string()))?;
        if find_entry(self.state.entries(media), &name).is_some() || !self.claim(&target_path) {
            return Err(skip(
                what,
                "nameTaken",
                format!("{target_path} already exists on the device"),
            ));
        }
        let mut tx = Transaction::new(format!(
            "Convert {} ({} ul chunks) to {}",
            record.name(),
            parts.len(),
            target.label()
        ));
        tx.game = Some(game_info(
            &iso_name::classify(&name).map_or(title.clone(), |n| n.title),
            Some(startup),
            media,
            target.label(),
        ));
        tx.notices = notices;
        if !self.state.has_folder(folder.as_str()) && self.claim(&folder) {
            tx.push(Step::CreateDir {
                path: folder.clone(),
            });
        }
        self.plan
            .sources
            .push(device_source(SourceKind::Joined, parts.clone(), iso_len));
        let index = self.plan.sources.len() - 1;
        match target {
            Target::Zso => tx.push(Step::WriteZso {
                path: target_path,
                source: index,
                max_len: zso::max_output(iso_len),
            }),
            _ => tx.push(Step::WriteFile {
                path: target_path,
                source: index,
                offset: 0,
                len: iso_len,
            }),
        }
        let step = self
            .ul_without(record)
            .map_err(|e| skip(what, "notFound", e.to_string()))?;
        tx.push(step);
        for (path, len) in parts {
            tx.then(Step::Remove {
                path,
                len,
                what: Removal::UserData,
            });
        }
        self.drop_cache(&mut tx, media)
            .map_err(|e| skip(what, "badName", e.to_string()))?;
        tx.notices.push(Notice::info(
            "oldImageDeleted",
            "the chunks are deleted once the image and ul.cfg are complete",
        ));
        Ok(tx)
    }
}

fn describe(game: &GameRef) -> String {
    match game {
        GameRef::Image { path } => path.to_string(),
        GameRef::Ul { name, startup, .. } => format!("ul.cfg: {name} ({startup})"),
    }
}

/// Plans converting `games` to `target`. Games that cannot be converted
/// are listed as skipped with the reason.
pub fn plan_convert(
    fs: &dyn DeviceFs,
    games: &[GameRef],
    target: Target,
    options: &ConvertOptions,
) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let plan = new_plan(
        PlanKind::Convert,
        format!("Convert to {}", target.label()),
        &state,
    );
    let mut planner = Planner {
        fs,
        state,
        options,
        plan,
        ul_next: None,
        claimed: HashSet::new(),
    };
    for game in games {
        match planner.convert(game, target) {
            Ok(tx) => planner.plan.transactions.push(tx),
            Err(skipped) => planner.plan.skipped.push(skipped),
        }
    }
    let Planner {
        state, mut plan, ..
    } = planner;
    let count = plan.transactions.len();
    plan.title = match count {
        1 => format!("Convert 1 game to {}", target.label()),
        n => format!("Convert {n} games to {}", target.label()),
    };
    if count == 0 && plan.notices.iter().all(|n| n.code != "unfinishedOperation") {
        plan.notices
            .push(Notice::error("nothingToConvert", "no game to convert"));
    }
    let cluster = state.info.cluster_size;
    let needed = on_disk(plan.peak_write_bytes(), cluster) + RESERVE;
    if count > 0 && needed > state.info.free_bytes {
        plan.notices.push(Notice::error(
            "noSpace",
            format!(
                "not enough free space: {needed} bytes needed, {} available",
                state.info.free_bytes
            ),
        ));
    }
    if count > 0 {
        plan.notices.push(Notice::info(
            "idKept",
            "game IDs do not change: settings, covers, cheats and memory cards keep working",
        ));
    }
    Ok(plan)
}
