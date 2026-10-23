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
use yaoplm_i18n::Phrase;

use crate::device::{DeviceState, join, on_disk, rel};
use crate::import::{image_name, ul_title};
use crate::multipart::Joined;
use crate::ops::{Found, FoundKind, GameRef, OpError, find_game, game_info, new_plan};
use crate::plan::{
    Bytes, Expect, Notice, Plan, PlanKind, Removal, Skipped, Source, SourceFile, SourceKind, Step,
    Transaction,
};
use crate::reasons;

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

fn skip(what: &str, reason: Phrase) -> Skipped {
    Skipped {
        path: what.into(),
        reason: Notice::warning(reason),
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

    /// `ul.cfg` without the records of `record`'s game: chunk names depend
    /// only on the name's CRC and the ID (`sbCreatePath_name()`), so every
    /// record with both names the same files, which go with it. OPL's own
    /// `sbDelete()` drops just one record (`sbRebuildULCfg(.., excludeID)`,
    /// src/supportbase.c @b3a21997) and leaves a duplicate listing deleted
    /// chunks. Found by their bytes, as earlier transactions may have moved
    /// them. Returns how many records went.
    fn ul_without(&mut self, record: &UlRecord) -> Result<(Step, usize), OpError> {
        let current = self.ul_current();
        let (records, tail) = current.as_chunks::<{ ul_cfg::RECORD_SIZE }>();
        let same = |r: &[u8; ul_cfg::RECORD_SIZE]| {
            let r = UlRecord::from_bytes(*r);
            r.name_crc() == record.name_crc() && r.startup_bytes() == record.startup_bytes()
        };
        let removed = records.iter().filter(|r| same(r)).count();
        if removed == 0 {
            return Err(OpError::NotFound(
                Phrase::error("ulRecordGone").with("name", record.name()),
            ));
        }
        let mut next: Vec<u8> = records
            .iter()
            .filter(|r| !same(r))
            .flatten()
            .copied()
            .collect();
        next.extend_from_slice(tail);
        Ok((self.ul_step(current, next)?, removed))
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

    /// The game ID OPL would read from SYSTEM.CNF in `ul` chunks.
    fn chunks_startup(&self, parts: &[(RelPath, u64)]) -> Option<String> {
        let mut readers = Vec::with_capacity(parts.len());
        for (path, len) in parts {
            readers.push((self.fs.open(path).ok()?, *len));
        }
        let mut image = DiscImage::open(Joined::new(readers)).ok()?;
        yaoplm_formats::image::opl_startup(&mut image).ok()
    }

    /// FAT32 holds files up to 4 GiB - 1; a ZSO of `iso_len` bytes may come
    /// out bigger than that only if its worst case does.
    fn zso_may_not_fit(&self, iso_len: u64) -> bool {
        self.state
            .info
            .kind
            .max_file_size()
            .is_some_and(|m| zso::max_output(iso_len) > m)
    }

    /// The ISO bytes a ZSO on the device yields.
    fn zso_iso_len(&self, path: &RelPath) -> Result<u64, String> {
        let reader = self.fs.open(path).map_err(|e| e.to_string())?;
        let image = DiscImage::open(reader).map_err(|e| e.to_string())?;
        Ok(image.sector_count() * SECTOR_SIZE as u64)
    }

    fn convert(&mut self, game: &GameRef, target: Target) -> Result<Transaction, Skipped> {
        let found = find_game(self.fs, &self.state, game).map_err(|e| {
            skip(
                &describe(game),
                Phrase::notice("notFound").with("reason", e.phrase()),
            )
        })?;
        let what = describe(game);
        if target == Target::Zso && !self.options.profile.reads_zso() {
            return Err(skip(&what, Phrase::notice("zsoUnsupportedByProfile")));
        }
        let Found {
            media,
            kind,
            startup,
        } = found;
        let startup = startup.ok_or_else(|| {
            skip(
                &what,
                Phrase::notice("noStartup").with("reason", Phrase::reason("noGameId")),
            )
        })?;
        match kind {
            FoundKind::Image { path, name, len } => {
                let current = match name.extension {
                    ImageExtension::Iso => Target::Iso,
                    ImageExtension::Zso => Target::Zso,
                };
                if current == target {
                    return Err(skip(&what, Phrase::notice("alreadyThatFormat")));
                }
                let mut magic = [0u8; 4];
                let is_zso = match self
                    .fs
                    .open(&path)
                    .and_then(|mut r| r.read_exact(&mut magic))
                {
                    Ok(()) => magic == zso::MAGIC,
                    // Shorter than a ZSO header: no image at all, refused below.
                    Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => false,
                    // Guessing would copy ZSO bytes into an .iso: refuse.
                    Err(err) => {
                        return Err(skip(
                            &what,
                            Phrase::notice("unreadable").with("error", err.to_string()),
                        ));
                    }
                };
                let (source, iso_len) = if is_zso {
                    let iso_len = self.zso_iso_len(&path).map_err(|e| {
                        skip(
                            &what,
                            Phrase::notice("unreadable").with("error", e.to_string()),
                        )
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
                        Phrase::notice("partialSector").with("size", iso_len),
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
                self.drop_cache(&mut tx, media).map_err(|e| {
                    skip(
                        &what,
                        Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
                    )
                })?;
                Ok(tx)
            }
            FoundKind::Ul { record, .. } => {
                if target == Target::Ul {
                    return Err(skip(&what, Phrase::notice("alreadyThatFormat")));
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
        let new_path = join(&folder, &new_name).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        if find_entry(self.state.entries(media), &new_name).is_some() || !self.claim(&new_path) {
            return Err(skip(
                what,
                Phrase::notice("nameTaken").with("path", new_path.to_string()),
            ));
        }
        let iso_len = source.len;
        let mut tx = Transaction::titled(
            Phrase::title("convertImage")
                .with("name", file_name)
                .with("format", target.label()),
        );
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
                if self.zso_may_not_fit(iso_len) {
                    tx.notices
                        .push(Notice::warning(Phrase::notice("zsoMayNotFit")));
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
                    return Err(skip(what, Phrase::notice("tooLargeUseUl")));
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
            Phrase::notice("oldImageDeleted")
                .with("name", file_name)
                .with("format", target.label()),
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
        let chunks = split_plan(iso_len).map_err(|e| {
            skip(
                what,
                Phrase::notice("cannotSplit").with("reason", reasons::text(&e)),
            )
        })?;
        if self.state.info.remote && !ul_cfg::fits_smb(chunks.len()) {
            return Err(skip(
                what,
                Phrase::notice("ulTooManyPartsForSmb").with("parts", chunks.len()),
            ));
        }
        let record = UlRecord::new(&name, startup, chunks.len(), media).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::ul(&e)),
            )
        })?;
        let crc = usba_crc32(name.as_bytes());
        let current = ul_cfg::UlCfg::parse(&self.ul_current());
        if current
            .records
            .iter()
            .any(|r| r.name_crc() == crc && r.startup_bytes() == startup.as_bytes())
        {
            return Err(skip(
                what,
                Phrase::notice("alreadyOnDevice")
                    .with("name", &name)
                    .with("startup", startup),
            ));
        }
        let mut tx = Transaction::titled(
            Phrase::title("convertToUl")
                .with("title", title)
                .count(chunks.len()),
        );
        tx.game = Some(game_info(&name, Some(startup), media, "UL"));
        if name != title {
            tx.notices
                .push(Notice::info(Phrase::notice("ulTitle").with("name", &name)));
        }
        self.plan.sources.push(source);
        let index = self.plan.sources.len() - 1;
        for chunk in &chunks {
            let path = rel(&record.chunk_name(chunk.part)).map_err(|e| {
                skip(
                    what,
                    Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
                )
            })?;
            if find_entry(&self.state.top, path.as_str()).is_some() || !self.claim(&path) {
                return Err(skip(
                    what,
                    Phrase::notice("nameTaken").with("path", path.to_string()),
                ));
            }
            tx.push(Step::WriteFile {
                path,
                source: index,
                offset: chunk.offset,
                len: chunk.len,
            });
        }
        let step = self.ul_with(&record).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        tx.push(step);
        tx.notices
            .push(Notice::info(Phrase::notice("imageDeletedAfterUl")));
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
                .ok_or_else(|| {
                    skip(
                        what,
                        Phrase::notice("ulPartMissing")
                            .with("part", part)
                            .with("name", &chunk),
                    )
                })?;
            // OPL reads part n at n GiB (`DeviceReadSectors()`, steps of
            // 0x80000 sectors, modules/iopcore/cdvdman/device-smb.c): joined,
            // other sizes would give an image unlike the one OPL plays.
            let last = part + 1 == record.parts();
            if (!last && entry.len != ul_cfg::CHUNK_SIZE)
                || (last && (entry.len == 0 || entry.len > ul_cfg::CHUNK_SIZE))
            {
                return Err(skip(
                    what,
                    Phrase::notice("ulPartSize")
                        .with("part", part)
                        .with("name", &chunk)
                        .with("size", entry.len),
                ));
            }
            let path = rel(&entry.name).map_err(|e| {
                skip(
                    what,
                    Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
                )
            })?;
            parts.push((path, entry.len));
        }
        let iso_len: u64 = parts.iter().map(|(_, len)| len).sum();
        if !iso_len.is_multiple_of(SECTOR_SIZE as u64) {
            return Err(skip(
                what,
                Phrase::notice("partialSector").with("size", iso_len),
            ));
        }
        if target == Target::Iso
            && self
                .state
                .info
                .kind
                .max_file_size()
                .is_some_and(|m| iso_len > m)
        {
            return Err(skip(what, Phrase::notice("tooLargeForFat32")));
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
        let mut style = self.options.name_style.unwrap_or(NameStyle::WithStartup);
        // Without the ID in its name, an image is known by SYSTEM.CNF: an
        // ID fixed in ul.cfg would be lost.
        if style == NameStyle::TitleOnly && self.chunks_startup(&parts).as_deref() != Some(startup)
        {
            notices.push(Notice::info(
                Phrase::notice("idKeptInName").with("startup", startup),
            ));
            style = NameStyle::WithStartup;
        }
        let name = image_name(startup, &title, ext, style, &mut notices).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        let folder = self.state.folder(media).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        let target_path = join(&folder, &name).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        if find_entry(self.state.entries(media), &name).is_some() || !self.claim(&target_path) {
            return Err(skip(
                what,
                Phrase::notice("nameTaken").with("path", target_path.to_string()),
            ));
        }
        let mut tx = Transaction::titled(
            Phrase::title("convertFromUl")
                .with("name", record.name())
                .count(parts.len())
                .with("format", target.label()),
        );
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
        if target == Target::Zso && self.zso_may_not_fit(iso_len) {
            tx.notices
                .push(Notice::warning(Phrase::notice("zsoMayNotFit")));
        }
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
        let (step, records) = self
            .ul_without(record)
            .map_err(|e| skip(what, Phrase::notice("notFound").with("reason", e.phrase())))?;
        tx.push(step);
        if records > 1 {
            tx.notices.push(Notice::info(
                Phrase::notice("ulDuplicateRecords").with("times", records),
            ));
        }
        for (path, len) in parts {
            tx.then(Step::Remove {
                path,
                len,
                what: Removal::UserData,
            });
        }
        self.drop_cache(&mut tx, media).map_err(|e| {
            skip(
                what,
                Phrase::notice("badName").with("reason", reasons::bad_name(&e)),
            )
        })?;
        tx.notices
            .push(Notice::info(Phrase::notice("chunksDeletedAfterImage")));
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
    let title = |count: usize| {
        Phrase::title("convertGames")
            .count(count)
            .with("format", target.label())
    };
    let plan = new_plan(PlanKind::Convert, title(games.len()), &state);
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
    plan.set_title(title(count));
    if count == 0 && plan.notices.iter().all(|n| n.code != "unfinishedOperation") {
        plan.notices
            .push(Notice::error(Phrase::notice("nothingToConvert")));
    }
    // The most the device holds at once: the new form is written before the
    // old one goes. Every file takes whole clusters, as in imports.
    let cluster = state.info.cluster_size;
    let mut running = 0u64;
    let mut peak = 0u64;
    for tx in &plan.transactions {
        running += tx
            .steps
            .iter()
            .map(|s| on_disk(s.write_bytes(), cluster))
            .sum::<u64>();
        peak = peak.max(running);
        running = running.saturating_sub(tx.steps.iter().map(Step::freed_bytes).sum());
    }
    let needed = peak + RESERVE;
    if count > 0 && needed > state.info.free_bytes {
        plan.notices.push(Notice::error(
            Phrase::notice("noSpace")
                .with("needed", needed)
                .with("free", state.info.free_bytes),
        ));
    }
    if count > 0 {
        plan.notices.push(Notice::info(Phrase::notice("idKept")));
    }
    Ok(plan)
}
