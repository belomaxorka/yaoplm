//! The import planner: sources in, a [`Plan`] out (IMP-1, IMP-3 to IMP-6,
//! IMP-8, IMP-10, IMP-11, LIB-3).
//!
//! For each source it predicts the game ID exactly as OPL will, picks CD or
//! DVD, names the file by ADR-0006 (`SLUS_200.62.Title.iso`) and decides
//! the layout:
//! - a file that fits the file system is copied whole into `CD/` or `DVD/`
//!   (IMP-6); a ZSO stays a ZSO unless the profile cannot read it;
//! - on FAT32 an image over 4 GiB - 1 is split into 1 GiB `ul` chunks with
//!   a 64-byte record appended to `ul.cfg` (IMP-5, research/02 §5). The
//!   records already there keep every byte, including a ragged tail.
//!
//! Nothing is written here. Conflicts with what is already on the device
//! skip that source with a reason; a lack of space blocks the whole plan.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use yaoplm_device::fs::{DeviceFs, find_entry};
use yaoplm_device::source::SourceFs;
use yaoplm_formats::iso_name::{
    self, ImageExtension, NameError, NameFormat, NameStyle, TITLE_MAX, build_file_name,
    is_name_safe_startup, sanitize_title,
};
use yaoplm_formats::serial;
use yaoplm_formats::ul_cfg::{self, UlRecord, split_plan};
use yaoplm_formats::usba_crc32::usba_crc32;
use yaoplm_formats::{LoaderProfile, Media, zso};

use crate::device::{DeviceState, join, on_disk};
use crate::plan::{Bytes, Expect, GameInfo, Notice, Plan, PlanKind, Skipped, Step, Transaction};
use crate::scan::CD_MAX_BYTES;
use crate::source::{ProbeError, Probed, probe, sheet_files};
use crate::titles;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transfer {
    #[default]
    Copy,
    /// Delete the sources once each game is on the device (IMP-11).
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// `ul` only where the file system needs it.
    #[default]
    Auto,
    /// Always `ul` chunks (USBAdvance style).
    Ul,
}

/// Per-source choices the user made in the plan screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOverride {
    pub path: PathBuf,
    pub title: Option<String>,
    pub media: Option<Media>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportOptions {
    pub profile: LoaderProfile,
    pub name_style: NameStyle,
    pub transfer: Transfer,
    pub layout: Layout,
    pub overrides: Vec<ImportOverride>,
    /// Store images as ZSO (GAME-4): OPL 1.2.0 beta only, never for `ul`.
    pub compress: bool,
}

/// Extensions OPL users have that YAOPLM does not import yet (IMP-1b/c).
const LATER: [&str; 9] = ["chd", "cso", "zip", "7z", "rar", "nrg", "mdf", "img", "ecm"];
const MAX_DEPTH: usize = 8;
/// Kept free for the journal, CFG backups and directory growth.
const RESERVE: u64 = 1 << 20;

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("._"))
}

fn skip(path: &Path, code: &str, message: impl Into<String>) -> Skipped {
    Skipped {
        path: path.to_path_buf(),
        reason: Notice::warning(code, message),
    }
}

fn walk(
    sources: &dyn SourceFs,
    dir: &Path,
    depth: usize,
    found: &mut Vec<PathBuf>,
    skipped: &mut Vec<Skipped>,
) {
    let Ok(entries) = sources.list(dir) else {
        skipped.push(skip(dir, "unreadable", "cannot list this folder"));
        return;
    };
    for path in entries {
        if is_hidden(&path) {
            continue;
        }
        match sources.metadata(&path) {
            Ok(meta) if meta.is_dir => {
                if depth < MAX_DEPTH {
                    walk(sources, &path, depth + 1, found, skipped);
                }
            }
            Ok(_) => {
                let ext = extension(&path);
                if matches!(ext.as_str(), "iso" | "zso" | "cue" | "bin") {
                    found.push(path);
                } else if LATER.contains(&ext.as_str()) {
                    skipped.push(skip(
                        &path,
                        "formatNotYetSupported",
                        format!(".{ext} files are not supported yet"),
                    ));
                }
            }
            Err(_) => skipped.push(skip(&path, "unreadable", "cannot read this file")),
        }
    }
}

/// Files to import: folders are searched recursively; a BIN that a CUE
/// sheet in the input names is imported through the sheet only.
pub fn expand_inputs(sources: &dyn SourceFs, inputs: &[PathBuf]) -> (Vec<PathBuf>, Vec<Skipped>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for input in inputs {
        match sources.metadata(input) {
            Ok(meta) if meta.is_dir => walk(sources, input, 0, &mut found, &mut skipped),
            Ok(_) => found.push(input.clone()),
            Err(_) => skipped.push(skip(input, "notFound", "does not exist")),
        }
    }
    let mut seen = HashSet::new();
    found.retain(|p| seen.insert(p.clone()));
    let consumed: HashSet<PathBuf> = found
        .iter()
        .filter(|p| extension(p) == "cue")
        .flat_map(|cue| sheet_files(sources, cue))
        .collect();
    found.retain(|p| extension(p) != "bin" || !consumed.contains(p));
    (found, skipped)
}

/// The title from a file name: the stem, without an old-style ID prefix.
pub fn title_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match iso_name::classify(&format!("{stem}.iso")) {
        Ok(name) if name.format == NameFormat::Old => name.title,
        _ => stem,
    }
}

/// The database title for `startup` when the file name gave none: an empty
/// title, or one that is just the ID again (`SLUS_200.62.iso`,
/// `SLUS-20062.iso`), (META-1).
fn known_title_for(from_name: &str, startup: &str) -> Option<String> {
    let bare =
        from_name.trim().is_empty() || serial::lookup_key(from_name) == serial::lookup_key(startup);
    bare.then(|| titles::known_title(startup)).flatten()
}

/// A `ul.cfg` title: printable ASCII only (OPL hashes other bytes
/// unpredictably), at most 32 bytes, no stray spaces.
pub fn ul_title(title: &str) -> String {
    let ascii: String = title
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| (' '..='~').contains(c))
        .collect();
    let mut out = ascii.split_whitespace().collect::<Vec<_>>().join(" ");
    out.truncate(ul_cfg::NAME_MAX);
    out.truncate(out.trim_end().len());
    out
}

/// Builds the image file name within OPL's limits: the title at most 160
/// characters, the whole name at most 159 bytes (the SMB buffer).
pub(crate) fn image_name(
    startup: &str,
    title: &str,
    ext: ImageExtension,
    style: NameStyle,
    notices: &mut Vec<Notice>,
) -> Result<String, NameError> {
    let mut style = style;
    if style == NameStyle::WithStartup && !is_name_safe_startup(startup) {
        notices.push(Notice::info(
            "idNotInName",
            format!(
                "the game ID {startup:?} cannot lead a file name; OPL will read it from the image"
            ),
        ));
        style = NameStyle::TitleOnly;
    }
    let mut chars: Vec<char> = title.chars().take(TITLE_MAX).collect();
    let shortened = chars.len() < title.chars().count();
    loop {
        let candidate: String = chars.iter().collect();
        let candidate = candidate.trim_end_matches(['.', ' ']).to_owned();
        let name = match build_file_name(startup, &candidate, ext, style) {
            Err(NameError::LooksLikeOldFormat(_)) if is_name_safe_startup(startup) => {
                build_file_name(startup, &candidate, ext, NameStyle::WithStartup)?
            }
            other => other?,
        };
        if iso_name::fits_smb(&name) || chars.len() <= 1 {
            if shortened || chars.len() < title.chars().count() {
                notices.push(Notice::info(
                    "titleShortened",
                    format!("the title was shortened to fit OPL's limits: {candidate}"),
                ));
            }
            return Ok(name);
        }
        chars.pop();
    }
}

struct Planner<'a> {
    state: DeviceState,
    sources: &'a dyn SourceFs,
    options: &'a ImportOptions,
    plan: Plan,
    /// Names claimed by this plan, lower-case, as `CD/x.iso` or `ul.x`.
    claimed: HashSet<String>,
    /// Game IDs on the device or in this plan.
    startups: HashSet<String>,
    /// `ul.cfg` after the transactions planned so far.
    ul_next: Option<Vec<u8>>,
    games: usize,
}

impl Planner<'_> {
    fn claim(&mut self, folder: &str, name: &str) -> bool {
        self.claimed
            .insert(format!("{folder}/{name}").to_lowercase())
    }

    fn plan_source(&mut self, path: &Path) -> Result<(), Skipped> {
        let probed = probe(self.sources, path).map_err(|err| match err {
            ProbeError::NotAnImage => skip(path, "notAnImage", "not a PS2 disc image"),
            ProbeError::Unsupported(why) => skip(path, "unsupportedSource", why),
            ProbeError::Io { source, .. } => {
                skip(path, "unreadable", format!("cannot read it: {source}"))
            }
        })?;
        if probed.ps1 {
            return Err(skip(
                path,
                "ps1Game",
                "a PS1 disc: OPL does not run it (POPStarter support is planned)",
            ));
        }
        let startup = probed.startup.clone().map_err(|why| {
            skip(
                path,
                "noStartup",
                format!("OPL would hide this disc: {why}"),
            )
        })?;
        let override_ = self
            .options
            .overrides
            .iter()
            .find(|o| o.path == path || o.path == probed.input);
        let mut notices = probed.notices.clone();
        let (suggested, conflict) = probed.hints.suggest(probed.iso_len / 2048);
        let media = match override_.and_then(|o| o.media) {
            Some(media) => media,
            None => {
                notices.extend(conflict);
                suggested
            }
        };
        if media == Media::Cd && probed.iso_len > CD_MAX_BYTES {
            notices.push(Notice::warning(
                "cdTooLarge",
                "more data than a PS2 CD holds; it is probably a DVD",
            ));
        }
        let title = match override_.and_then(|o| o.title.clone()) {
            Some(title) => title,
            None => {
                let from_name = title_from_path(path);
                match known_title_for(&from_name, &startup) {
                    Some(known) => {
                        notices.push(Notice::info(
                            "titleFromDatabase",
                            format!("the file name has no title; the bundled database calls {startup} {known:?}"),
                        ));
                        known
                    }
                    None => from_name,
                }
            }
        };

        let profile = self.options.profile;
        let mut keep_zso = probed.zso && profile.reads_zso();
        if probed.zso && !profile.reads_zso() {
            notices.push(Notice::info(
                "zsoDecompressed",
                "OPL 1.1.0 cannot read ZSO: the image is stored decompressed",
            ));
        }
        let max = self.state.info.kind.max_file_size();
        let whole_len = if keep_zso {
            probed.data_len
        } else {
            probed.iso_len
        };
        let ul = self.options.layout == Layout::Ul || max.is_some_and(|m| whole_len > m);
        if ul && keep_zso {
            keep_zso = false;
            notices.push(Notice::info(
                "zsoDecompressed",
                "ul chunks hold plain ISO data: the ZSO is decompressed",
            ));
        }

        let mut tx = if ul {
            self.ul_transaction(path, &probed, &startup, &title, media, &mut notices)?
        } else {
            self.image_transaction(
                path,
                &probed,
                &startup,
                &title,
                media,
                keep_zso,
                &mut notices,
            )?
        };
        if !self.startups.insert(startup.clone()) {
            notices.push(Notice::warning(
                "duplicateStartup",
                format!("another game with ID {startup} is on the device; they share CFG, art and cheats"),
            ));
        }
        if self.options.transfer == Transfer::Move {
            tx.then(Step::RemoveSource {
                source: self.plan.sources.len() - 1,
            });
        }
        tx.notices = notices;
        self.plan.transactions.push(tx);
        self.games += 1;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn image_transaction(
        &mut self,
        path: &Path,
        probed: &Probed,
        startup: &str,
        title: &str,
        media: Media,
        keep_zso: bool,
        notices: &mut Vec<Notice>,
    ) -> Result<Transaction, Skipped> {
        let clean = sanitize_title(title);
        let clean = if clean.is_empty() {
            startup.to_owned()
        } else {
            clean
        };
        let compress = !keep_zso && self.options.compress;
        if compress && !self.options.profile.reads_zso() {
            notices.push(Notice::info(
                "zsoNotForProfile",
                "OPL 1.1.0 cannot read ZSO: the game is stored as ISO",
            ));
        }
        let compress = compress && self.options.profile.reads_zso();
        let ext = if keep_zso || compress {
            ImageExtension::Zso
        } else {
            ImageExtension::Iso
        };
        let name = image_name(startup, &clean, ext, self.options.name_style, notices)
            .map_err(|e| skip(path, "badName", e.to_string()))?;
        if !name.is_ascii() {
            notices.push(Notice::warning(
                "nonAsciiName",
                "non-ASCII characters in the name may not display or open on the console",
            ));
        }
        let folder = self
            .state
            .folder(media)
            .map_err(|e| skip(path, "badName", e.to_string()))?;
        if find_entry(self.state.entries(media), &name).is_some()
            || !self.claim(folder.as_str(), &name)
        {
            return Err(skip(
                path,
                "nameTaken",
                format!("{folder}/{name} already exists on the device"),
            ));
        }
        let target = join(&folder, &name).map_err(|e| skip(path, "badName", e.to_string()))?;
        let source = if keep_zso {
            probed.raw_source()
        } else {
            probed.iso_source()
        };
        let len = source.len;
        self.plan.sources.push(source);
        let mut tx = Transaction::new(if compress {
            format!("Compress {name} into {folder}")
        } else {
            format!("Copy {name} to {folder}")
        });
        tx.game = Some(GameInfo {
            title: iso_name::classify(&name).map_or(clean.clone(), |n| n.title),
            startup: startup.to_owned(),
            media,
            format: if keep_zso || compress { "ZSO" } else { "ISO" }.into(),
        });
        if compress {
            tx.push(Step::WriteZso {
                path: target,
                source: self.plan.sources.len() - 1,
                max_len: zso::max_output(len),
            });
            notices.push(Notice::info(
                "compressedToZso",
                "stored as ZSO: smaller on the device; OPL 1.2.0 beta reads it",
            ));
        } else {
            tx.push(Step::WriteFile {
                path: target,
                source: self.plan.sources.len() - 1,
                offset: 0,
                len,
            });
        }
        if let Some(cache) = self
            .state
            .drop_cache(media)
            .map_err(|e| skip(path, "badName", e.to_string()))?
            && !self.claimed.contains(&format!("drop:{}", media.folder()))
        {
            self.claimed.insert(format!("drop:{}", media.folder()));
            tx.then(cache);
        }
        Ok(tx)
    }

    fn ul_transaction(
        &mut self,
        path: &Path,
        probed: &Probed,
        startup: &str,
        title: &str,
        media: Media,
        notices: &mut Vec<Notice>,
    ) -> Result<Transaction, Skipped> {
        let mut name = ul_title(title);
        if name.is_empty() {
            name = ul_title(startup);
        }
        if name != title {
            notices.push(Notice::info(
                "ulTitle",
                format!("ul.cfg holds at most 32 ASCII characters: the title is {name:?}"),
            ));
        }
        let chunks =
            split_plan(probed.iso_len).map_err(|e| skip(path, "cannotSplit", e.to_string()))?;
        if self.state.info.remote && !ul_cfg::fits_smb(chunks.len()) {
            return Err(skip(
                path,
                "ulTooManyPartsForSmb",
                format!("{} ul chunks: OPL opens at most 10 over SMB", chunks.len()),
            ));
        }
        let record = UlRecord::new(&name, startup, chunks.len(), media)
            .map_err(|e| skip(path, "cannotSplit", format!("cannot store as ul: {e}")))?;
        let crc = usba_crc32(name.as_bytes());
        if self
            .state
            .ul
            .records
            .iter()
            .any(|r| r.name_crc() == crc && r.startup_bytes() == startup.as_bytes())
        {
            return Err(skip(
                path,
                "alreadyOnDevice",
                format!("ul.cfg already lists {name} ({startup})"),
            ));
        }
        for part in 0..record.parts() {
            let chunk = record.chunk_name(part);
            if find_entry(&self.state.top, &chunk).is_some() || !self.claim("", &chunk) {
                return Err(skip(
                    path,
                    "nameTaken",
                    format!("{chunk} already exists on the device"),
                ));
            }
        }
        if self.options.layout == Layout::Auto {
            notices.push(Notice::info(
                "storedAsUl",
                format!(
                    "FAT32 holds files up to 4 GiB: stored as {} ul chunks of 1 GiB",
                    chunks.len()
                ),
            ));
        }

        self.plan.sources.push(probed.iso_source());
        let source = self.plan.sources.len() - 1;
        let mut tx = Transaction::new(format!("Split {name} into {} ul chunks", chunks.len()));
        tx.game = Some(GameInfo {
            title: name.clone(),
            startup: startup.to_owned(),
            media,
            format: "UL".into(),
        });
        for chunk in &chunks {
            tx.push(Step::WriteFile {
                path: join(
                    &yaoplm_device::RelPath::root(),
                    &record.chunk_name(chunk.part),
                )
                .map_err(|e| skip(path, "badName", e.to_string()))?,
                source,
                offset: chunk.offset,
                len: chunk.len,
            });
        }
        // Insert before a ragged tail so every record stays 64-byte aligned.
        let current = self.ul_next.clone().or_else(|| self.state.ul_bytes.clone());
        let mut next = current.clone().unwrap_or_default();
        let aligned = next.len() - next.len() % ul_cfg::RECORD_SIZE;
        next.splice(aligned..aligned, record.as_bytes().iter().copied());
        tx.push(Step::PutFile {
            path: self
                .state
                .ul_path()
                .map_err(|e| skip(path, "badName", e.to_string()))?,
            content: Bytes(next.clone()),
            expect: match current {
                Some(bytes) => Expect::Content(Bytes(bytes)),
                None => Expect::Absent,
            },
        });
        self.ul_next = Some(next);
        Ok(tx)
    }
}

/// Plans copying `inputs` (files or folders) to the device.
pub fn plan_import(
    fs: &dyn DeviceFs,
    sources: &dyn SourceFs,
    inputs: &[PathBuf],
    options: &ImportOptions,
) -> io::Result<Plan> {
    let state = DeviceState::read(fs)?;
    let mut plan = Plan::new(
        PlanKind::Import,
        "Import games",
        state.info.fingerprint.clone(),
        state.info.free_bytes,
    );
    let mut startups = HashSet::new();
    for entry in state.cd.iter().chain(&state.dvd) {
        if let Ok(name) = iso_name::classify(&entry.name)
            && let Some(id) = name.startup
        {
            startups.insert(id);
        }
    }
    startups.extend(state.ul.records.iter().map(|r| r.startup()));
    plan.notices.extend(state.unfinished_notice());
    let folders = state.missing_folders()?;
    let (candidates, skipped) = expand_inputs(sources, inputs);
    plan.skipped = skipped;

    let mut planner = Planner {
        state,
        sources,
        options,
        plan,
        claimed: HashSet::new(),
        startups,
        ul_next: None,
        games: 0,
    };
    for path in &candidates {
        if let Err(skipped) = planner.plan_source(path) {
            planner.plan.skipped.push(skipped);
        }
    }
    let Planner {
        state,
        mut plan,
        games,
        ..
    } = planner;

    if games > 0 && !folders.is_empty() {
        let mut tx = Transaction::new("Create the OPL folders");
        for step in folders {
            tx.push(step);
        }
        plan.transactions.insert(0, tx);
    }
    plan.title = match games {
        1 => "Import 1 game".into(),
        n => format!("Import {n} games"),
    };
    let cluster = state.info.cluster_size;
    let needed: u64 = plan
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .map(|s| on_disk(s.write_bytes(), cluster))
        .sum::<u64>()
        + RESERVE;
    if games > 0 && needed > state.info.free_bytes {
        plan.notices.push(Notice::error(
            "noSpace",
            format!(
                "not enough free space: {needed} bytes needed, {} available",
                state.info.free_bytes
            ),
        ));
    }
    if options.transfer == Transfer::Move && games > 0 {
        plan.notices.push(Notice::warning(
            "sourcesDeleted",
            "the source files are deleted after each game is copied",
        ));
    }
    if games == 0 && plan.notices.iter().all(|n| n.code != "unfinishedOperation") {
        plan.notices
            .push(Notice::error("nothingToImport", "no game to import"));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            title_from_path(Path::new("x/SLUS_200.62.Grand Theft Auto III.iso")),
            "Grand Theft Auto III"
        );
        assert_eq!(title_from_path(Path::new("Ico (USA).cue")), "Ico (USA)");
        assert_eq!(ul_title("  Ōkami:  Zero\tHour  "), "kami: Zero Hour");
        assert_eq!(ul_title(&"A".repeat(40)).len(), 32);
        assert_eq!(ul_title(&format!("{} B", "A".repeat(31))), "A".repeat(31));
        assert_eq!(ul_title("日本"), "");
    }

    #[test]
    fn names_fit_opl_limits() {
        let mut notices = Vec::new();
        let name = image_name(
            "SLUS_200.62",
            &"T".repeat(200),
            ImageExtension::Iso,
            NameStyle::WithStartup,
            &mut notices,
        )
        .unwrap();
        assert_eq!(name.len(), 159);
        assert_eq!(notices[0].code, "titleShortened");

        let mut notices = Vec::new();
        let name = image_name(
            "DATA\\MAIN.E",
            "Game",
            ImageExtension::Iso,
            NameStyle::WithStartup,
            &mut notices,
        )
        .unwrap();
        assert_eq!(name, "Game.iso");
        assert_eq!(notices[0].code, "idNotInName");

        // A title-only name that OPL would parse as old style gets the ID.
        let name = image_name(
            "SLUS_200.62",
            "ABCD_EFG.HI.Game",
            ImageExtension::Iso,
            NameStyle::TitleOnly,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(name, "SLUS_200.62.ABCD_EFG.HI.Game.iso");
    }
}
