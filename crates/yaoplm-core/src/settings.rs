//! Planners for a game's settings (CFG-2) and for memory cards (VMC-1).
//!
//! - A game's settings live in `CFG/<ID>.cfg`, named after the ID OPL uses.
//!   They are edited the way OPL's game menu saves them
//!   ([`yaoplm_formats::game_cfg`]): only what changed, keys at their
//!   default removed, every other key and line kept byte for byte (CFG-1).
//!   wOPL's libconfig files are never rewritten.
//! - Each change is one transaction: the CFG is copied to
//!   `.yaoplm/backup/<id>/` first (CFG-5) and replaced atomically; that
//!   write is the commit point. The plan lists every key that changes.
//! - A blank card is formatted like OPL's own (`vmc::write_blank`), as one
//!   file whose space is reserved before the first byte (ADR-0019), and is
//!   assigned to a slot in the same transaction. An existing card is
//!   replaced only on request: it moves to `.yaoplm/backup/<id>/VMC/`
//!   first, so its saves can still be copied back by hand (ADR-0036).
//! - Cards are never deleted or renamed here (ADR-0020).

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom};

use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, DirEntry, find_entry, list_or_empty};
use yaoplm_formats::LoaderProfile;
use yaoplm_formats::cfg::{self, CfgDocument, CfgEntry, CfgError, Dialect};
use yaoplm_formats::game_cfg::{GameSettings, SettingsError, VMC_SLOTS};
use yaoplm_formats::iso_name::ImageExtension;
use yaoplm_formats::vmc::{self, Layout, VmcNameError, VmcProblem};
use yaoplm_i18n::Phrase;

use crate::art::backup_dirs;
use crate::device::{DeviceState, join, rel};
use crate::ops::{
    Found, FoundKind, GameRef, OpError, backup_step, find_game, game_info, new_backup_dir, new_plan,
};
use crate::plan::{Bytes, CfgChange, Expect, GameInfo, Notice, Plan, PlanKind, Step, Transaction};

pub const CFG_FOLDER: &str = "CFG";

/// A blank card to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCard {
    /// `VMC/<name>.bin`.
    pub name: String,
    pub size_mb: u32,
    /// A card with this name is there and may be replaced: it moves to the
    /// backup folder first.
    pub replace: bool,
}

/// What to change in a game's CFG.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsEdit {
    /// The settings OPL's menu edits, as they should be.
    pub settings: Option<GameSettings>,
    /// Keys written as they are, after `settings` (`yaoplm cfg --set`).
    pub set: Vec<(String, String)>,
    /// Keys removed.
    pub unset: Vec<String>,
    /// Cards to create in the same transaction, typically for a slot.
    pub cards: Vec<NewCard>,
}

/// A game's CFG as YAOPLM shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCfg {
    pub startup: String,
    /// Where the file is, or would be created.
    pub path: RelPath,
    pub exists: bool,
    /// False for wOPL's libconfig: shown, never rewritten.
    pub editable: bool,
    pub settings: GameSettings,
    /// Every key, as OPL would load it.
    pub entries: Vec<CfgEntry>,
}

/// A file in `VMC/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// The name `$VMC_0`/`$VMC_1` use: the file name without `.bin`.
    pub name: String,
    pub path: RelPath,
    pub len: u64,
    /// The size in MiB when OPL would take the card.
    pub size_mb: Option<u32>,
    /// Why OPL would not.
    pub problem: Option<VmcProblem>,
    /// Saves in the root directory, when it could be read.
    pub saves: Option<u32>,
    /// Games whose CFG names the card, with the slot (0 or 1).
    pub used_by: Vec<(String, u8)>,
}

fn invalid(phrase: Phrase) -> OpError {
    OpError::Invalid(phrase)
}

fn settings_reason(err: &SettingsError) -> Phrase {
    match err {
        SettingsError::CompatibilityBits { value, allowed } => Phrase::reason("compatibilityBits")
            .with("value", format!("{value:#04x}"))
            .with("allowed", format!("{allowed:#04x}")),
        SettingsError::AltStartupTooLong(value) => Phrase::reason("altStartupTooLong")
            .with("value", value)
            .with("max", yaoplm_formats::game_cfg::ALT_STARTUP_MAX),
        SettingsError::AltStartupChars(value) => {
            Phrase::reason("altStartupChars").with("value", value)
        }
        SettingsError::VmcName { slot, error } => Phrase::reason("vmcSlotName")
            .with("slot", u64::from(*slot) + 1)
            .with("reason", vmc_name_reason(error)),
        SettingsError::GsmVmode(value) => Phrase::reason("gsmVmode")
            .with("value", value.to_string())
            .with("max", yaoplm_formats::game_cfg::GSM_VMODE_MAX.to_string()),
        SettingsError::CheatMode(value) => {
            Phrase::reason("cheatMode").with("value", value.to_string())
        }
        SettingsError::Cfg(err) => cfg_reason(err),
    }
}

fn cfg_reason(err: &CfgError) -> Phrase {
    match err {
        CfgError::EmptyKey => Phrase::reason("cfgKeyEmpty"),
        CfgError::KeyTooLong(key) => Phrase::reason("cfgKeyTooLong")
            .with("key", key)
            .with("max", cfg::KEY_MAX),
        CfgError::BadKey(key) => Phrase::reason("cfgKeyChars").with("key", key),
        CfgError::RuntimeKey(key) => Phrase::reason("cfgRuntimeKey").with("key", key),
        CfgError::ValueTooLong { key, len } => Phrase::reason("cfgValueTooLong")
            .with("key", key)
            .with("length", *len)
            .with("max", cfg::VALUE_MAX),
        CfgError::BadValue(key) => Phrase::reason("cfgValueChars").with("key", key),
    }
}

fn vmc_name_reason(err: &VmcNameError) -> Phrase {
    match err {
        VmcNameError::Empty => Phrase::reason("vmcNameEmpty"),
        VmcNameError::TooLong(name) => Phrase::reason("vmcNameTooLong")
            .with("name", name)
            .with("max", vmc::NAME_MAX),
        VmcNameError::BadChars(name) => Phrase::reason("vmcNameChars").with("name", name),
    }
}

fn settings_error(err: &SettingsError) -> OpError {
    invalid(Phrase::error("settingsInvalid").with("reason", settings_reason(err)))
}

/// A top-level folder as it is named on the device, or `name` itself.
fn folder(state: &DeviceState, name: &str) -> io::Result<RelPath> {
    rel(find_entry(&state.top, name)
        .filter(|e| e.is_dir)
        .map_or(name, |e| e.name.as_str()))
}

fn entries_of(fs: &dyn DeviceFs, state: &DeviceState, name: &str) -> io::Result<Vec<DirEntry>> {
    match find_entry(&state.top, name).filter(|e| e.is_dir) {
        Some(dir) => list_or_empty(fs, &rel(&dir.name)?),
        None => Ok(Vec::new()),
    }
}

/// The CFG of `startup`: its path, and its bytes when it exists.
fn locate_cfg(
    fs: &dyn DeviceFs,
    state: &DeviceState,
    startup: &str,
) -> io::Result<(RelPath, Option<Vec<u8>>)> {
    let dir = folder(state, CFG_FOLDER)?;
    let wanted = format!("{startup}.cfg");
    match find_entry(&entries_of(fs, state, CFG_FOLDER)?, &wanted).filter(|e| !e.is_dir) {
        Some(entry) => {
            let path = join(&dir, &entry.name)?;
            let bytes = fs.read(&path)?;
            Ok((path, Some(bytes)))
        }
        None => Ok((join(&dir, &wanted)?, None)),
    }
}

fn found_info(found: &Found, startup: &str) -> GameInfo {
    let (title, format) = match &found.kind {
        FoundKind::Image { name, .. } => (
            name.title.clone(),
            if name.extension == ImageExtension::Zso {
                "ZSO"
            } else {
                "ISO"
            },
        ),
        FoundKind::Ul { record, .. } => (record.name(), "UL"),
    };
    game_info(&title, Some(startup), found.media, format)
}

/// The ID of a game, as OPL names its CFG.
fn startup_of(found: &Found) -> Result<String, OpError> {
    found
        .startup
        .clone()
        .ok_or_else(|| invalid(Phrase::error("noGameId")))
}

/// Reads a game's settings (`yaoplm cfg <root> <game>`).
pub fn read_game_cfg(fs: &dyn DeviceFs, game: &GameRef) -> Result<GameCfg, OpError> {
    let state = DeviceState::read(fs)?;
    let found = find_game(fs, &state, game)?;
    let startup = startup_of(&found)?;
    let (path, bytes) = locate_cfg(fs, &state, &startup)?;
    let raw = bytes.as_deref().unwrap_or_default();
    let editable = cfg::detect_dialect(raw) == Dialect::Opl;
    let doc = CfgDocument::parse(if editable { raw } else { &[] });
    Ok(GameCfg {
        startup,
        path,
        exists: bytes.is_some(),
        editable,
        settings: GameSettings::read(&doc),
        entries: doc.entries(),
    })
}

/// The keys whose values differ between two versions of a CFG.
fn changed_keys(before: &CfgDocument, after: &CfgDocument) -> Vec<CfgChange> {
    let old: BTreeMap<String, String> = before
        .entries()
        .into_iter()
        .map(|e| (e.key, e.value))
        .collect();
    let mut seen = HashSet::new();
    let mut changes = Vec::new();
    for entry in after.entries() {
        seen.insert(entry.key.clone());
        let was = old.get(&entry.key);
        if was != Some(&entry.value) {
            changes.push(CfgChange {
                key: entry.key,
                before: was.cloned(),
                after: Some(entry.value),
            });
        }
    }
    for entry in before.entries() {
        if !seen.contains(&entry.key) {
            changes.push(CfgChange {
                key: entry.key,
                before: Some(entry.value),
                after: None,
            });
        }
    }
    changes
}

/// The steps that create `cards` in `VMC/`, replaced ones moved to
/// `backup` first.
fn card_steps(
    state: &DeviceState,
    existing: &[DirEntry],
    cards: &[NewCard],
    backup: &RelPath,
    now: i64,
    tx: &mut Transaction,
) -> Result<(), OpError> {
    let dir = folder(state, vmc::VMC_FOLDER)?;
    if !cards.is_empty() && !state.has_folder(vmc::VMC_FOLDER) {
        tx.push(Step::CreateDir { path: dir.clone() });
    }
    let mut names = HashSet::new();
    let mut backed_up = false;
    for card in cards {
        vmc::validate_name(&card.name).map_err(|e| {
            invalid(Phrase::error("vmcNameInvalid").with("reason", vmc_name_reason(&e)))
        })?;
        if !names.insert(card.name.to_lowercase()) {
            return Err(invalid(Phrase::error("vmcTwice").with("name", &card.name)));
        }
        Layout::new(card.size_mb)
            .map_err(|_| invalid(Phrase::error("vmcSize").with("size", u64::from(card.size_mb))))?;
        let file = vmc::file_name(&card.name);
        let path = match find_entry(existing, &file) {
            Some(old) if old.is_dir => {
                return Err(OpError::Conflict(
                    Phrase::error("alreadyExists").with("path", join(&dir, &old.name)?.to_string()),
                ));
            }
            Some(_) if !card.replace => {
                return Err(OpError::Conflict(
                    Phrase::error("vmcExists").with("name", &card.name),
                ));
            }
            Some(old) => {
                let vmc_backup = join(backup, vmc::VMC_FOLDER)?;
                if !backed_up {
                    for step in backup_dirs(&vmc_backup)? {
                        tx.push(step);
                    }
                    backed_up = true;
                }
                let from = join(&dir, &old.name)?;
                tx.push(Step::Rename {
                    from: from.clone(),
                    to: join(&vmc_backup, &old.name)?,
                });
                tx.notices.push(Notice::warning(
                    Phrase::notice("vmcReplaced")
                        .with("path", from.to_string())
                        .with("folder", vmc_backup.to_string()),
                ));
                from
            }
            None => join(&dir, &file)?,
        };
        tx.push(Step::WriteVmc {
            path,
            size_mb: card.size_mb,
            created: now,
        });
    }
    Ok(())
}

/// Plans changing a game's settings and creating the cards it gets
/// (CFG-2, VMC-1). `now` (Unix seconds) stamps new cards.
pub fn plan_settings(
    fs: &dyn DeviceFs,
    game: &GameRef,
    edit: &SettingsEdit,
    profile: LoaderProfile,
    now: i64,
) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let found = find_game(fs, &state, game)?;
    let startup = startup_of(&found)?;
    let (path, bytes) = locate_cfg(fs, &state, &startup)?;
    if bytes
        .as_deref()
        .is_some_and(|b| cfg::detect_dialect(b) == Dialect::Libconfig)
    {
        return Err(invalid(
            Phrase::error("cfgNotOpl").with("path", path.to_string()),
        ));
    }
    let before = CfgDocument::parse(bytes.as_deref().unwrap_or_default());
    let mut doc = before.clone();
    if let Some(settings) = &edit.settings {
        settings
            .apply(&mut doc, profile)
            .map_err(|e| settings_error(&e))?;
    }
    for (key, value) in &edit.set {
        doc.set(key, value)
            .map_err(|e| settings_error(&SettingsError::Cfg(e)))?;
    }
    for key in &edit.unset {
        doc.remove(key);
    }
    // Keys set by name pass the same checks as the menu's.
    GameSettings::read(&doc)
        .validate(&GameSettings::read(&before), profile)
        .map_err(|e| settings_error(&e))?;
    let changes = changed_keys(&before, &doc);

    let info = found_info(&found, &startup);
    let title = Phrase::title("gameSettings").with("title", &info.title);
    let mut plan = new_plan(PlanKind::Settings, title.clone(), &state);
    if changes.is_empty() && edit.cards.is_empty() {
        plan.notices
            .push(Notice::info(Phrase::notice("settingsUnchanged")));
        return Ok(plan);
    }

    let mut tx = Transaction::titled(title);
    tx.game = Some(info);
    let backup = new_backup_dir()?;
    let existing = entries_of(fs, &state, vmc::VMC_FOLDER)?;
    card_steps(&state, &existing, &edit.cards, &backup, now, &mut tx)?;

    // A slot naming a card that is neither there nor created now.
    let after = GameSettings::read(&doc);
    let created: HashSet<String> = edit.cards.iter().map(|c| c.name.to_lowercase()).collect();
    for (slot, name) in after.vmc.iter().enumerate() {
        let missing = !name.is_empty()
            && !created.contains(&name.to_lowercase())
            && find_entry(&existing, &vmc::file_name(name)).is_none();
        if missing && doc.get(VMC_SLOTS[slot]) != before.get(VMC_SLOTS[slot]) {
            tx.notices.push(Notice::warning(
                Phrase::notice("vmcNotOnDevice")
                    .with("slot", slot + 1)
                    .with("name", name),
            ));
        }
    }

    if !changes.is_empty() {
        if !state.has_folder(CFG_FOLDER) {
            tx.push(Step::CreateDir {
                path: path.parent().unwrap_or_default(),
            });
        }
        let expect = match &bytes {
            Some(old) => {
                tx.push(backup_step(&backup, &path)?);
                Expect::Content(Bytes(old.clone()))
            }
            None => Expect::Absent,
        };
        tx.push(Step::PutFile {
            path: path.clone(),
            content: Bytes(doc.to_bytes()),
            expect,
        });
    }
    tx.cfg_changes = changes;
    plan.transactions.push(tx);
    Ok(plan)
}

/// Plans creating blank cards that no game names yet (`yaoplm vmc create`).
pub fn plan_cards(fs: &dyn DeviceFs, cards: &[NewCard], now: i64) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let title = match cards {
        [card] => Phrase::title("createVmc").with("name", &card.name),
        _ => Phrase::title("createVmcs").count(cards.len()),
    };
    let mut plan = new_plan(PlanKind::Vmc, title.clone(), &state);
    if cards.is_empty() {
        return Ok(plan);
    }
    let mut tx = Transaction::titled(title);
    let existing = entries_of(fs, &state, vmc::VMC_FOLDER)?;
    card_steps(&state, &existing, cards, &new_backup_dir()?, now, &mut tx)?;
    plan.transactions.push(tx);
    Ok(plan)
}

/// Reads what a card's superblock and root directory say.
fn inspect_card(
    fs: &dyn DeviceFs,
    path: &RelPath,
    len: u64,
) -> (Result<u32, VmcProblem>, Option<u32>) {
    let mut superblock = vec![0u8; vmc::SUPERBLOCK_SIZE];
    let Ok(mut file) = fs.open(path) else {
        return (Err(VmcProblem::BadMagic), None);
    };
    let read = read_up_to(&mut file, &mut superblock);
    superblock.truncate(read);
    let check = vmc::opl_check(&superblock, len);
    let saves = check.ok().and_then(|_| {
        let root = vmc::Superblock::parse(&superblock).root_dir_offset();
        let mut entry = [0u8; 8];
        file.seek(SeekFrom::Start(root)).ok()?;
        file.read_exact(&mut entry).ok()?;
        vmc::directory_entries(&entry).map(|n| n.saturating_sub(2))
    });
    (check, saves)
}

fn read_up_to(file: &mut dyn Read, buf: &mut [u8]) -> usize {
    let mut done = 0;
    while done < buf.len() {
        match file.read(&mut buf[done..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => done += n,
        }
    }
    done
}

/// The cards in `VMC/` and the games that name them.
pub fn list_cards(fs: &dyn DeviceFs) -> io::Result<Vec<Card>> {
    let state = DeviceState::read(fs)?;
    let dir = folder(&state, vmc::VMC_FOLDER)?;
    // Which CFG names which card: by card name, in lower case.
    let mut users: BTreeMap<String, Vec<(String, u8)>> = BTreeMap::new();
    let cfg_dir = folder(&state, CFG_FOLDER)?;
    for entry in entries_of(fs, &state, CFG_FOLDER)? {
        let Some(stem) = entry
            .name
            .strip_suffix(".cfg")
            .or_else(|| entry.name.strip_suffix(".CFG"))
        else {
            continue;
        };
        if entry.is_dir {
            continue;
        }
        let Ok(bytes) = fs.read(&join(&cfg_dir, &entry.name)?) else {
            continue;
        };
        if cfg::detect_dialect(&bytes) != Dialect::Opl {
            continue;
        }
        let doc = CfgDocument::parse(&bytes);
        for (slot, key) in VMC_SLOTS.iter().enumerate() {
            if let Some(name) = doc.get(key).filter(|n| !n.is_empty()) {
                users
                    .entry(name.to_lowercase())
                    .or_default()
                    .push((stem.to_owned(), slot as u8));
            }
        }
    }
    let mut cards = Vec::new();
    for entry in entries_of(fs, &state, vmc::VMC_FOLDER)? {
        if entry.is_dir {
            continue;
        }
        let Some(name) = entry
            .name
            .strip_suffix(".bin")
            .or_else(|| entry.name.strip_suffix(".BIN"))
        else {
            continue;
        };
        let path = join(&dir, &entry.name)?;
        let (check, saves) = inspect_card(fs, &path, entry.len);
        cards.push(Card {
            name: name.to_owned(),
            path,
            len: entry.len,
            size_mb: check.ok(),
            problem: check.err(),
            saves,
            used_by: users.remove(&name.to_lowercase()).unwrap_or_default(),
        });
    }
    cards.sort_by_key(|c| c.name.to_lowercase());
    Ok(cards)
}

/// Why OPL would not take a card, to follow a colon.
pub fn card_problem(problem: &VmcProblem) -> Phrase {
    crate::reasons::vmc(problem)
}

/// The name OPL's VMC menu suggests for a game's slot: `<ID>_<slot>`.
/// OPL 1.2 swaps the ID for a series' group ID from its own table
/// (`src/vmc_groups.c`, AFL-3.0 without a statement of where the data comes
/// from), which YAOPLM does not carry (ADR-0036).
pub fn default_card_name(startup: &str, slot: u8) -> String {
    vmc::suggested_name(startup, slot)
}
