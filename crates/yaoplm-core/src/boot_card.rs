//! What a memory card emulator's boot card holds, read from the card's own
//! file system (DEV-2, LDR-5, ADR-0048, research/19 §4).
//!
//! A boot card is judged by its content, never by the name `BootCard.ini`
//! gives its channel: on the owner's factory SD the names of channels 3–6
//! are wrong, and an SD Builder image ships OPL builds under swapped file
//! names. So:
//! - **the image's SHA-256** against sd2psXtd's SD Builder images
//!   (`data/emulator/sd-builder.tsv`, hashes only, ADR-0007) names it
//!   exactly when it is one of them;
//! - **the launcher** comes from the titles in the save folders'
//!   `icon.sys`: `B?EXEC-SYSTEM` titled `FreeMcBoot` or `PS2BBL`, a `BOOT`
//!   folder titled `FORTUNA` (FunTuna); `SYS-CONF/FREEMCB.CNF` and
//!   `SYS-CONF/PS2BBL.INI` when the titles say nothing;
//! - **OPL builds** inside are known by the version and the family inside
//!   their ELF (ADR-0041), whatever their file is called. A build with MMCE
//!   carries its setting `mmce_mode` (MMCE fork, YAOPL, RiptOPL) or
//!   `enable_mmce` (wOPL) in its program;
//! - **a PS1 boot card** is known by its save names (FreePSXBoot) and its
//!   hash.

use std::io::{self, Read};
use std::sync::OnceLock;

use yaoplm_device::{Advice, DeviceFs, RelPath};
use yaoplm_formats::icon_sys::IconSys;
use yaoplm_formats::ps1mc::Ps1Card;
use yaoplm_formats::ps2_elf;
use yaoplm_formats::ps2mc::Card;

use crate::detect::{Family, program_family};

static SD_BUILDER: &str = include_str!("../../../data/emulator/sd-builder.tsv");
/// A loader ELF bigger than this on a card is not read: cards are 8 MiB.
const ELF_MAX: u64 = 16 << 20;
/// How deep the card's folders are searched for ELFs.
const DEPTH: usize = 4;

/// A launcher a boot card starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Launcher {
    FreeMcBoot,
    Ps2bbl,
    FunTuna,
    FreePsxBoot,
}

/// One of SD Builder's images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderImage {
    /// `PS2`, `PS1`, `COH`.
    pub variant: &'static str,
    /// The names SD Builder offers it under: `FMCB 1.966`.
    pub names: Vec<&'static str>,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// SD Builder's images, from `data/emulator/sd-builder.tsv`.
pub fn builder_images() -> &'static [BuilderImage] {
    static IMAGES: OnceLock<Vec<BuilderImage>> = OnceLock::new();
    IMAGES.get_or_init(|| {
        SD_BUILDER
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .filter_map(|line| {
                let mut cols = line.split('\t');
                let variant = cols.next()?;
                let names = cols.next()?.split(" / ").collect();
                let file = cols.next()?;
                let bytes = cols.next()?.parse().ok()?;
                let sha256 = cols.next()?;
                Some(BuilderImage {
                    variant,
                    names,
                    file,
                    bytes,
                    sha256,
                })
            })
            .collect()
    })
}

/// An OPL-family build inside a boot card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLoader {
    /// On the card: `APPS/OPNPS2LD.ELF`.
    pub path: String,
    pub family: Family,
    pub version: Option<String>,
    /// It reads games from a memory card emulator's SD.
    pub mmce: bool,
}

/// What a boot card holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootContents {
    pub launchers: Vec<Launcher>,
    pub loaders: Vec<CardLoader>,
    /// The SD Builder image it is, byte for byte.
    pub builder: Option<BuilderImage>,
    /// Save folders and the titles their `icon.sys` gives.
    pub folders: Vec<(String, Option<String>)>,
    /// The card's file system could not be read: why.
    pub unreadable: Option<String>,
}

fn add(launchers: &mut Vec<Launcher>, launcher: Launcher) {
    if !launchers.contains(&launcher) {
        launchers.push(launcher);
    }
}

fn is_exec_folder(name: &str) -> bool {
    // `BAEXEC-SYSTEM`, `BEEXEC-SYSTEM`, `BIEXEC-SYSTEM`, `BCEXEC-SYSTEM`.
    name.len() == 13 && name.starts_with('B') && name[2..].eq_ignore_ascii_case("EXEC-SYSTEM")
}

/// A PS2 (or arcade) card.
fn ps2_contents<R: Read + io::Seek>(card: &mut Card<R>, found: &mut BootContents) {
    let walked = match card.walk(DEPTH) {
        Ok(walked) => walked,
        Err(err) => {
            found.unreadable = Some(err.to_string());
            return;
        }
    };
    let titles: Vec<(String, Option<String>)> = walked
        .iter()
        .filter(|(path, entry)| entry.is_dir() && !path.contains('/'))
        .map(|(path, entry)| {
            let title = card
                .list(entry)
                .ok()
                .and_then(|inner| {
                    inner
                        .into_iter()
                        .find(|e| e.is_file() && e.name == b"icon.sys")
                })
                .and_then(|icon| card.read(&icon, 4096).ok())
                .and_then(|bytes| IconSys::parse(&bytes))
                .map(|icon| icon.title());
            (path.clone(), title)
        })
        .collect();
    for (folder, title) in &titles {
        let title = title.as_deref().unwrap_or("").to_ascii_lowercase();
        if is_exec_folder(folder) && (title.contains("freemcboot") || title.contains("fmcb")) {
            add(&mut found.launchers, Launcher::FreeMcBoot);
        }
        if is_exec_folder(folder) && title.contains("ps2bbl") {
            add(&mut found.launchers, Launcher::Ps2bbl);
        }
        if title.contains("fortuna") {
            add(&mut found.launchers, Launcher::FunTuna);
        }
    }
    let has = |path: &str| {
        walked
            .iter()
            .any(|(p, e)| e.is_file() && p.eq_ignore_ascii_case(path))
    };
    if has("SYS-CONF/FREEMCB.CNF") {
        add(&mut found.launchers, Launcher::FreeMcBoot);
    }
    if has("SYS-CONF/PS2BBL.INI") {
        add(&mut found.launchers, Launcher::Ps2bbl);
    }
    found.folders = titles;
    for (path, entry) in &walked {
        let is_elf = entry.is_file()
            && path.len() > 4
            && path.is_char_boundary(path.len() - 4)
            && path[path.len() - 4..].eq_ignore_ascii_case(".elf");
        if !is_elf || u64::from(entry.length) > ELF_MAX {
            continue;
        }
        let Ok(bytes) = card.read(entry, ELF_MAX) else {
            continue;
        };
        let Ok(program) = ps2_elf::program(&bytes) else {
            continue;
        };
        let Some(family) = program_family(&program) else {
            continue;
        };
        found.loaders.push(CardLoader {
            path: path.clone(),
            family,
            version: ps2_elf::find_version(&program.bytes),
            mmce: program.contains(b"mmce_mode") || program.contains(b"enable_mmce"),
        });
    }
}

/// What the boot card image `file` on `fs` holds. A card whose file system
/// cannot be read says why in `unreadable`; only reading the image itself
/// failing is an error.
pub fn contents(fs: &dyn DeviceFs, file: &RelPath, len: u64) -> io::Result<BootContents> {
    let mut found = BootContents::default();
    let sha = yaoplm_hash::sha256_reader(fs.open(file)?)?;
    found.builder = builder_images()
        .iter()
        .find(|image| image.bytes == len && image.sha256.eq_ignore_ascii_case(&sha))
        .cloned();
    if len == yaoplm_formats::ps1mc::CARD_SIZE as u64 {
        match Ps1Card::parse(&fs.read(file)?) {
            Ok(card) => {
                if card
                    .saves
                    .iter()
                    .any(|s| s.name.to_ascii_lowercase().contains("freepsxboot"))
                {
                    add(&mut found.launchers, Launcher::FreePsxBoot);
                }
                found.folders = card
                    .saves
                    .iter()
                    .map(|s| {
                        (
                            s.name.clone(),
                            Some(s.title.clone()).filter(|t| !t.is_empty()),
                        )
                    })
                    .collect();
            }
            Err(err) => found.unreadable = Some(err.to_string()),
        }
        return Ok(found);
    }
    match Card::open(fs.open(file)?, len) {
        Ok(mut card) => ps2_contents(&mut card, &mut found),
        Err(err) => found.unreadable = Some(err.to_string()),
    }
    Ok(found)
}

fn launcher_name(launcher: Launcher) -> &'static str {
    match launcher {
        Launcher::FreeMcBoot => "FreeMcBoot",
        Launcher::Ps2bbl => "PS2BBL",
        Launcher::FunTuna => "FunTuna",
        Launcher::FreePsxBoot => "FreePSXBoot",
    }
}

/// What the card holds, in a few words for a warning: the SD Builder name
/// when it is one, else its launchers.
pub fn describe(contents: &BootContents) -> Option<String> {
    if let Some(image) = &contents.builder {
        return image.names.first().map(|n| (*n).to_owned());
    }
    (!contents.launchers.is_empty()).then(|| {
        contents
            .launchers
            .iter()
            .map(|l| launcher_name(*l))
            .collect::<Vec<_>>()
            .join(" + ")
    })
}

/// The channel's name in `BootCard.ini` disagrees with what its image holds
/// (research/19 §4): an SD Builder image under the name of another SD
/// Builder image, or a name that names a launcher the image does not hold.
/// A name of the user's own (`wLaunchELF 4.43x_isr_coh` for SD Builder's
/// `wLE COH`) is no disagreement, and a version is only checked where the
/// image is known exactly.
pub fn name_disagrees(name: &str, contents: &BootContents) -> bool {
    let name_lower = name.to_ascii_lowercase();
    if let Some(image) = &contents.builder {
        let claims_another = builder_images().iter().any(|other| {
            other.sha256 != image.sha256
                && other
                    .names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name.trim()))
        });
        if claims_another {
            return true;
        }
    }
    if contents.launchers.is_empty() {
        return false;
    }
    let named: [(Launcher, &[&str]); 4] = [
        (Launcher::Ps2bbl, &["ps2bbl"]),
        (Launcher::FreeMcBoot, &["fmcb", "freemcboot"]),
        (Launcher::FunTuna, &["funtuna", "fortuna"]),
        (Launcher::FreePsxBoot, &["freepsxboot"]),
    ];
    named.iter().any(|(launcher, words)| {
        words.iter().any(|w| name_lower.contains(w)) && !contents.launchers.contains(launcher)
    })
}

/// The doctor's warning for a channel whose name disagrees with its image.
pub fn channel_advice(
    variant: &str,
    channel: u32,
    name: Option<&str>,
    contents: &BootContents,
) -> Option<Advice> {
    let name = name?;
    if !name_disagrees(name, contents) {
        return None;
    }
    Some(Advice::EmulatorChannelName {
        variant: variant.to_owned(),
        channel,
        name: name.to_owned(),
        holds: describe(contents)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_table_reads() {
        let images = builder_images();
        assert_eq!(images.len(), 20);
        let fmcb = images.iter().find(|i| i.file == "FMCB-1966.mcd").unwrap();
        assert_eq!(
            (fmcb.variant, fmcb.names.as_slice(), fmcb.bytes),
            ("PS2", &["FMCB 1.966"][..], 8 << 20)
        );
        assert!(
            images
                .iter()
                .any(|i| i.variant == "PS1" && i.names.len() > 1)
        );
        assert!(images.iter().all(|i| i.sha256.len() == 64));
    }

    fn holding(launchers: &[Launcher], builder: Option<&str>) -> BootContents {
        BootContents {
            launchers: launchers.to_vec(),
            builder: builder
                .and_then(|file| builder_images().iter().find(|i| i.file == file).cloned()),
            ..BootContents::default()
        }
    }

    #[test]
    fn a_name_that_says_something_else() {
        // The factory card's channel 3: named FMCB 1.965, holds FMCB 1.963.
        let fmcb_1963 = holding(&[Launcher::FreeMcBoot], Some("FMCB-1963.mcd"));
        assert!(name_disagrees("FMCB 1.965", &fmcb_1963));
        assert!(!name_disagrees("FMCB 1.963", &fmcb_1963));
        assert!(!name_disagrees(" fmcb 1.963 ", &fmcb_1963));
        // Channel 5: named FMCB 1.953, holds PS2BBL.
        assert!(name_disagrees(
            "FMCB 1.953",
            &holding(&[Launcher::Ps2bbl], Some("PS2BBL.mcd"))
        ));
        // Not an SD Builder image: by launcher.
        let fmcb = holding(&[Launcher::FreeMcBoot], None);
        assert!(!name_disagrees("FMCB 1.966", &fmcb));
        assert!(!name_disagrees("My card", &fmcb));
        assert!(name_disagrees("PS2BBL", &fmcb));
        assert!(!name_disagrees(
            "FunTuna",
            &holding(&[Launcher::FunTuna], None)
        ));
        // SD Builder's arcade wLaunchELF under the factory's own name, and
        // a customised FMCB under an SD Builder name: nothing to say.
        let coh = holding(&[], Some("wLaunchELF_Arcade.mcd"));
        assert!(!name_disagrees("wLaunchELF 4.43x_isr_coh", &coh));
        assert!(!name_disagrees("FMCB 1.966", &fmcb));
        // Nothing known: no warning.
        assert!(!name_disagrees("PS2BBL", &BootContents::default()));
        let advice = channel_advice("PS2", 3, Some("FMCB 1.965"), &fmcb_1963).unwrap();
        let text = advice.phrase().english();
        assert!(
            text.contains("FMCB 1.965") && text.contains("FMCB 1.963") && text.contains('3'),
            "{text}"
        );
        assert_eq!(channel_advice("PS2", 3, None, &fmcb_1963), None);
    }
}
