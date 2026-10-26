//! A memory card emulator's SD: what is on it, what its files tell the
//! storage doctor, and the plans that may not touch it (DEV-2, ADR-0048).
//!
//! Layout from research/19 §2–§3 and research/07 §2.7, file names from
//! sd2psXtd/firmware@8a20bd9 (tag 1.4.0): `src/ps2/ps2_cardman.c`
//! (`MemoryCards/<variant>/BOOT/BootCard-<channel>.mcd`, the legacy
//! `BootCard.mcd`, `Card<N>/Card<N>-<channel>.mcd`, per-game
//! `<ID>/<ID>-<channel>.mcd`, damaged images moved aside as
//! `<folder>-<channel>.crp.<n>`), `src/card_config.c` (`<folder>/<folder>.ini`,
//! `.sd2psx/Game2Folder.ini`) and `src/settings.c` (`.sd2psx/settings.ini`).
//! Everything here only reads.

use std::io;

use yaoplm_device::emulator::{EmulatorSd, VARIANTS};
use yaoplm_device::fs::{DeviceFs, DirEntry, find_entry, list_or_empty};
use yaoplm_device::{Advice, RelPath};
use yaoplm_formats::sd2psx_ini::{self, CardIni, Ini};
use yaoplm_i18n::Phrase;

use crate::device::{join, rel};
use crate::plan::{Notice, Plan, Step};

pub const SETTINGS_INI: &str = ".sd2psx/settings.ini";
pub const GAME2FOLDER_INI: &str = ".sd2psx/Game2Folder.ini";
/// The card size the firmware's README recommends, in MiB.
pub const RECOMMENDED_CARD_SIZE: u32 = 8;
/// Damaged images named in the doctor's warning; the count says the rest.
const DAMAGED_SHOWN: usize = 3;
/// A card folder's ini bigger than this is not read: the firmware's are a
/// few hundred bytes.
const INI_MAX: u64 = 64 << 10;

/// One boot card image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootChannel {
    pub channel: u32,
    /// From `BootCard.ini`; a name says nothing of what the image holds
    /// (research/19 §4: wrong on the factory card for channels 3–6).
    pub name: Option<String>,
    /// Relative to the SD's root.
    pub file: String,
    pub bytes: u64,
}

/// The cards of one variant folder (`PS2`, `PS1`, `COH`...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The folder's name as it is on the SD.
    pub name: String,
    pub boot: Vec<BootChannel>,
    /// `BootCard.ini` `[Settings] MaxChannels`.
    pub max_channels: Option<u32>,
    /// `BootCard.ini` `[Settings] CardSize`.
    pub card_size: Option<u32>,
    /// `Card<N>` folders.
    pub cards: u32,
    /// Other folders: cards the firmware made for a game ID.
    pub game_cards: u32,
}

/// What is on the SD, for the device card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survey {
    pub variants: Vec<Variant>,
    /// `.sd2psx/settings.ini` as the firmware reads it.
    pub settings: Option<Ini>,
    /// Images the firmware moved aside, relative to the SD's root.
    pub damaged: Vec<String>,
    /// What the files tell the doctor.
    pub advice: Vec<Advice>,
}

/// `BootCard-3.mcd` → 3; the legacy `BootCard.mcd` is channel 1.
fn boot_channel(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".mcd").or_else(|| {
        name.len()
            .checked_sub(4)
            .filter(|&cut| name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(".mcd"))
            .map(|cut| &name[..cut])
    })?;
    if stem.eq_ignore_ascii_case("BootCard") {
        return Some(1);
    }
    let number = stem
        .get(..9)
        .filter(|p| p.eq_ignore_ascii_case("BootCard-"))?;
    let digits = &stem[number.len()..];
    // `BootCard-%d.mcd` from channel 1: no sign, no leading zero.
    let channel: u32 = digits.parse().ok()?;
    (channel >= 1 && digits.bytes().all(|b| b.is_ascii_digit()) && channel.to_string() == digits)
        .then_some(channel)
}

/// `Card12` is a numbered card folder.
fn is_card_folder(name: &str) -> bool {
    name.get(..4)
        .is_some_and(|p| p.eq_ignore_ascii_case("Card"))
        && name.len() > 4
        && name[4..].bytes().all(|b| b.is_ascii_digit())
}

/// `Card1-1.crp.0`: an image the firmware could not use.
fn is_damaged(name: &str) -> bool {
    name.rsplit_once(".crp.")
        .is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn read_ini(fs: &dyn DeviceFs, path: &RelPath, len: u64) -> io::Result<Option<Ini>> {
    if len > INI_MAX {
        return Ok(None);
    }
    Ok(Some(Ini::parse(&fs.read(path)?)))
}

/// Remembers an ini whose last line the firmware never reads.
fn check_tail(advice: &mut Vec<Advice>, file: &RelPath, ini: &Ini) {
    if let Some(line) = &ini.unread_tail {
        advice.push(Advice::EmulatorIniTail {
            file: file.as_str().to_owned(),
            line: line.trim().to_owned(),
        });
    }
}

fn find_file(entries: &[DirEntry], name: &str) -> Option<DirEntry> {
    find_entry(entries, name).filter(|e| !e.is_dir).cloned()
}

fn variant(
    fs: &dyn DeviceFs,
    dir: &RelPath,
    name: &str,
    damaged: &mut Vec<String>,
    advice: &mut Vec<Advice>,
) -> io::Result<Variant> {
    let mut found = Variant {
        name: name.to_owned(),
        boot: Vec::new(),
        max_channels: None,
        card_size: None,
        cards: 0,
        game_cards: 0,
    };
    for folder in list_or_empty(fs, dir)?.into_iter().filter(|e| e.is_dir) {
        let path = join(dir, &folder.name)?;
        let files = list_or_empty(fs, &path)?;
        for file in files.iter().filter(|f| !f.is_dir && is_damaged(&f.name)) {
            damaged.push(join(&path, &file.name)?.as_str().to_owned());
        }
        let boot = folder.name.eq_ignore_ascii_case("BOOT");
        // The boot folder's ini is `BootCard.ini`, a card folder's its own
        // name (`card_config_get_ini_name()`).
        let ini_name = if boot {
            "BootCard.ini".to_owned()
        } else {
            format!("{}.ini", folder.name)
        };
        let ini = match find_file(&files, &ini_name) {
            Some(entry) => {
                let ini_path = join(&path, &entry.name)?;
                let ini = read_ini(fs, &ini_path, entry.len)?;
                if let Some(ini) = &ini {
                    check_tail(advice, &ini_path, ini);
                }
                ini
            }
            None => None,
        };
        if boot {
            let card = ini.as_ref().map(CardIni::read).unwrap_or_default();
            found.max_channels = card.max_channels;
            found.card_size = card.card_size;
            let mut boot: Vec<BootChannel> = Vec::new();
            for file in files.iter().filter(|f| !f.is_dir) {
                let Some(channel) = boot_channel(&file.name) else {
                    continue;
                };
                // `BootCard-1.mcd` wins over the legacy name (`ps2_cardman.c`).
                let legacy = file.name.len() == "BootCard.mcd".len();
                let file_path = join(&path, &file.name)?.as_str().to_owned();
                match boot.iter_mut().find(|b| b.channel == channel) {
                    Some(_) if legacy => {}
                    Some(present) => {
                        present.file = file_path;
                        present.bytes = file.len;
                    }
                    None => boot.push(BootChannel {
                        channel,
                        name: card.name(channel).map(str::to_owned),
                        file: file_path,
                        bytes: file.len,
                    }),
                }
            }
            boot.sort_by_key(|b| b.channel);
            found.boot = boot;
        } else if is_card_folder(&folder.name) {
            found.cards += 1;
        } else {
            found.game_cards += 1;
        }
    }
    Ok(found)
}

/// What is on the SD whose root is `fs`'s root.
pub fn survey(fs: &dyn DeviceFs) -> io::Result<Survey> {
    let mut advice = Vec::new();
    let mut damaged = Vec::new();
    let top = list_or_empty(fs, &RelPath::root())?;
    let settings = match find_entry(&top, ".sd2psx").filter(|e| e.is_dir) {
        Some(dir) => {
            let dir = rel(&dir.name)?;
            let files = list_or_empty(fs, &dir)?;
            let mut settings = None;
            for name in ["settings.ini", "Game2Folder.ini"] {
                if let Some(entry) = find_file(&files, name) {
                    let path = join(&dir, &entry.name)?;
                    if let Some(ini) = read_ini(fs, &path, entry.len)? {
                        check_tail(&mut advice, &path, &ini);
                        if name == "settings.ini" {
                            settings = Some(ini);
                        }
                    }
                }
            }
            settings
        }
        None => None,
    };
    let mut variants = Vec::new();
    if let Some(cards) = find_entry(&top, "MemoryCards").filter(|e| e.is_dir) {
        let cards_dir = rel(&cards.name)?;
        let inside = list_or_empty(fs, &cards_dir)?;
        for name in VARIANTS {
            if let Some(entry) = find_entry(&inside, name).filter(|e| e.is_dir) {
                let dir = join(&cards_dir, &entry.name)?;
                variants.push(variant(fs, &dir, &entry.name, &mut damaged, &mut advice)?);
            }
        }
    }
    if let Some(size) = settings.as_ref().and_then(sd2psx_ini::settings_card_size)
        && size > RECOMMENDED_CARD_SIZE
    {
        advice.push(Advice::EmulatorCardSize(size));
    }
    if !damaged.is_empty() {
        let mut files = damaged
            .iter()
            .take(DAMAGED_SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if damaged.len() > DAMAGED_SHOWN {
            files.push_str(", …");
        }
        advice.push(Advice::EmulatorDamaged {
            count: damaged.len(),
            files,
        });
    }
    Ok(Survey {
        variants,
        settings,
        damaged,
        advice,
    })
}

/// The paths a step writes, renames or removes.
fn changed_paths(step: &Step) -> Vec<&RelPath> {
    match step {
        Step::CreateDir { path }
        | Step::RemoveDir { path }
        | Step::WriteFile { path, .. }
        | Step::WriteZso { path, .. }
        | Step::WriteVmc { path, .. }
        | Step::PutFile { path, .. }
        | Step::Remove { path, .. } => vec![path],
        Step::CopyFile { to, .. } => vec![to],
        Step::Rename { from, to } => vec![from, to],
        Step::RemoveSource { .. } | Step::DropJournal { .. } => Vec::new(),
    }
}

/// The first path of `plan` that is one of the emulator's service files.
pub fn first_protected<'a>(plan: &'a Plan, sd: &EmulatorSd) -> Option<&'a RelPath> {
    plan.transactions
        .iter()
        .flat_map(|t| &t.steps)
        .flat_map(changed_paths)
        .find(|path| sd.protects(path))
}

/// Why `plan` may not run on this SD: a step that changes one of the
/// emulator's files. Explicit card features (LDR-5, VMC-2) will have their
/// own plans.
pub fn refusal(plan: &Plan, sd: &EmulatorSd) -> Option<Notice> {
    first_protected(plan, sd).map(|path| {
        Notice::error(Phrase::notice("emulatorServiceFile").with("path", path.as_str()))
    })
}

/// Blocks `plan` when it changes one of the emulator's files; nothing
/// otherwise.
pub fn guard(plan: &mut Plan, sd: &EmulatorSd) {
    if let Some(notice) = refusal(plan, sd)
        && !plan.notices.contains(&notice)
    {
        plan.notices.push(notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_firmware_uses() {
        assert_eq!(boot_channel("BootCard-1.mcd"), Some(1));
        assert_eq!(boot_channel("BootCard-12.mcd"), Some(12));
        assert_eq!(boot_channel("bootcard-3.MCD"), Some(3));
        assert_eq!(boot_channel("BootCard.mcd"), Some(1));
        // The firmware opens `BootCard-%d.mcd` from channel 1 only
        // (`ps2_cardman.c`): no leading zeros, no channel 0.
        for name in ["BootCard-01.mcd", "BootCard-0.mcd", "BootCard-+1.mcd"] {
            assert_eq!(boot_channel(name), None, "{name}");
        }
        for name in [
            "BootCard-.mcd",
            "BootCard-1a.mcd",
            "BootCard-1.bin",
            "Boot.mcd",
            "BootCard-1.crp.0",
            "ж.mcd",
        ] {
            assert_eq!(boot_channel(name), None, "{name}");
        }
        assert!(is_card_folder("Card1") && is_card_folder("card255"));
        assert!(
            !is_card_folder("Card") && !is_card_folder("SLUS-20062") && !is_card_folder("Cards")
        );
        assert!(is_damaged("Card1-1.crp.0") && is_damaged("SLUS-20062-2.crp.11"));
        assert!(!is_damaged("Card1-1.mcd") && !is_damaged("x.crp.") && !is_damaged("x.crp.a"));
    }
}
