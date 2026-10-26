//! A memory card emulator's SD: how YAOPLM knows one and which of its files
//! it never changes (ADR-0048, research/19, research/07 §2.7).
//!
//! The signs, in the opened folder or in a folder above it up to the root of
//! its volume (the SD's root, or a copy of the SD on the computer):
//! - `.sd2psx/`: sd2psXtd's settings (`src/settings.c`), `Game2Folder.ini`
//!   (`src/card_config.c`) and the copy of its key (`src/keystore.c`);
//! - `MemoryCards/` with a variant folder `PS2`, `PS1`, `COH`, `SC2` or
//!   `PROT` (`src/card_config.c`, `src/ps2/ps2_cardman.c`): sd2psx keeps its
//!   settings in flash and has no `.sd2psx/` (sd2psx/firmware@2408cef);
//! - `mcp2.bin`, or `os/` next to `PS2/` or `PS1/`: MemCard PRO 2, whose
//!   firmware is closed (its manual).
//!
//! sd2psXtd/firmware@8a20bd9 (tag 1.4.0) for the file names. The service
//! files are protected whatever mode the device is read in: writes to them
//! are refused by plans and, as the last line, by [`crate::HostFs`].

use std::io;
use std::path::Path;

use crate::fs::{DeviceFs, DirEntry, RelPath, find_entry};

/// Folders under `MemoryCards/`, one per card variant the firmware emulates.
pub const VARIANTS: [&str; 5] = ["PS2", "PS1", "COH", "SC2", "PROT"];
/// What sd2psx and sd2psXtd keep at the SD's root. `civ.bin` is the
/// MagicGate key: the firmware writes it to flash, deletes it and keeps a
/// copy as `.sd2psx/civ.bin` (`keystore_deploy()`).
pub const SD2PSX_FILES: [&str; 3] = [".sd2psx", "MemoryCards", "civ.bin"];
/// What MemCard PRO 2 keeps at the SD's root.
pub const MCP2_FILES: [&str; 4] = ["mcp2.bin", "os", "PS1", "PS2"];

/// The emulator whose SD holds a device root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorSd {
    /// sd2psx or sd2psXtd (PSXMemCard Gen2 and other sd2psx hardware).
    pub sd2psx: bool,
    pub mcp2: bool,
    /// The files that told, relative to the SD's root: `.sd2psx`,
    /// `MemoryCards/PS2`, `mcp2.bin`, `os`.
    pub signs: Vec<String>,
    /// Where the opened root lies on the SD, `/`-separated; empty at the
    /// SD's root.
    pub at: String,
}

impl EmulatorSd {
    /// Top-level names this emulator owns.
    pub fn service_files(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.sd2psx {
            names.extend(SD2PSX_FILES);
        }
        if self.mcp2 {
            names.extend(MCP2_FILES);
        }
        names
    }

    /// `path` (relative to the opened root) is one of the emulator's files
    /// or inside one: YAOPLM does not change it outside explicit card
    /// features (ADR-0048). A FAT 8.3 alias of one (`MEMORY~1`) counts as
    /// the name it stands for: the OS resolves it.
    pub fn protects(&self, path: &RelPath) -> bool {
        let first = self
            .at
            .split('/')
            .chain(path.components())
            .find(|c| !c.is_empty());
        first.is_some_and(|first| self.is_service_name(first))
    }

    /// `name` (a top-level name on the SD) is one of the emulator's files
    /// or an 8.3 alias of one.
    pub fn is_service_name(&self, name: &str) -> bool {
        self.service_files()
            .iter()
            .any(|service| name.eq_ignore_ascii_case(service) || is_short_alias(name, service))
    }

    /// The same SD seen from a folder at `at` on it.
    fn at(mut self, at: String) -> Self {
        self.at = at;
        self
    }
}

/// `alias` may be the FAT 8.3 short name the OS gives `long` (`MEMORY~1`,
/// `SD2PSX~1` for `.sd2psx`; after four of a kind Windows takes two letters
/// and four hex digits, `ME1A2B~1`). Long names that are valid 8.3 names
/// (`civ.bin`, `PS2`) get no alias.
fn is_short_alias(alias: &str, long: &str) -> bool {
    let base: String = long
        .trim_start_matches('.')
        .chars()
        .filter(|c| *c != '.' && *c != ' ')
        .map(|c| match c {
            '+' | ',' | ';' | '=' | '[' | ']' => '_',
            c => c.to_ascii_uppercase(),
        })
        .collect();
    let needs_alias = long.starts_with('.') || long.contains(' ') || base.len() > 8;
    let Some((prefix, number)) = alias.split_once('~') else {
        return false;
    };
    if !needs_alias
        || prefix.is_empty()
        || number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|b| b.is_ascii_digit())
        || prefix.len() + 1 + number.len() > 8
    {
        return false;
    }
    let prefix = prefix.to_ascii_uppercase();
    let hashed = prefix.len() == 6
        && base.get(..2).is_some_and(|two| prefix.starts_with(two))
        && prefix[2..].bytes().all(|b| b.is_ascii_hexdigit());
    base.starts_with(&prefix) || hashed
}

/// Reads the signs from a root's listing; `list` lists a folder by name.
fn read_signs(
    top: &[DirEntry],
    list: &dyn Fn(&str) -> io::Result<Vec<DirEntry>>,
) -> io::Result<Option<EmulatorSd>> {
    let dir = |name: &str| find_entry(top, name).filter(|e| e.is_dir);
    let mut found = EmulatorSd {
        sd2psx: false,
        mcp2: false,
        signs: Vec::new(),
        at: String::new(),
    };
    if let Some(entry) = dir(".sd2psx") {
        found.sd2psx = true;
        found.signs.push(entry.name.clone());
    }
    if let Some(cards) = dir("MemoryCards") {
        let inside = match list(&cards.name) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            // Failing closed: a card folder that cannot be read still makes
            // this an emulator's SD.
            Err(_) => {
                found.sd2psx = true;
                found.signs.push(cards.name.clone());
                Vec::new()
            }
            Ok(inside) => inside,
        };
        for variant in VARIANTS {
            if let Some(entry) = find_entry(&inside, variant).filter(|e| e.is_dir) {
                found.sd2psx = true;
                found.signs.push(format!("{}/{}", cards.name, entry.name));
            }
        }
    }
    if let Some(entry) = find_entry(top, "mcp2.bin").filter(|e| !e.is_dir) {
        found.mcp2 = true;
        found.signs.push(entry.name.clone());
    }
    if let Some(entry) = dir("os")
        && (dir("PS2").is_some() || dir("PS1").is_some())
    {
        found.mcp2 = true;
        found.signs.push(entry.name.clone());
    }
    Ok((found.sd2psx || found.mcp2).then_some(found))
}

/// The emulator whose files are at the root of `fs`.
pub fn signs<F: DeviceFs + ?Sized>(fs: &F) -> io::Result<Option<EmulatorSd>> {
    let top = match fs.list(&RelPath::root()) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    };
    let list = |name: &str| -> io::Result<Vec<DirEntry>> {
        let dir = RelPath::new(name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        fs.list(&dir)
    };
    read_signs(&top, &list)
}

fn host_list(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        entries.push(DirEntry {
            name,
            is_dir,
            len: 0,
        });
    }
    Ok(entries)
}

fn host_signs(dir: &Path) -> Option<EmulatorSd> {
    let top = host_list(dir).ok()?;
    read_signs(&top, &|name| host_list(&dir.join(name))).ok()?
}

/// The emulator whose SD holds the folder `root` (canonical): its files in
/// `root` itself or in the nearest folder above it, up to the root of its
/// volume `volume_root` (without one, `root` alone counts). An unreadable
/// folder tells nothing.
pub fn find_on_host(root: &Path, volume_root: Option<&Path>) -> Option<EmulatorSd> {
    let top = volume_root.and_then(|v| std::fs::canonicalize(v).ok());
    for dir in root.ancestors() {
        let inside = top.as_deref().is_some_and(|top| dir.starts_with(top));
        if dir != root && !inside {
            break;
        }
        if let Some(found) = host_signs(dir) {
            let at: Vec<String> = root
                .strip_prefix(dir)
                .ok()?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            return Some(found.at(at.join("/")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    fn sd(signs: &[&str], at: &str) -> EmulatorSd {
        EmulatorSd {
            sd2psx: true,
            mcp2: false,
            signs: signs.iter().map(|s| (*s).to_owned()).collect(),
            at: at.to_owned(),
        }
    }

    #[test]
    fn the_service_files_are_protected_from_any_folder_on_the_sd() {
        let root = sd(&[".sd2psx"], "");
        for path in [
            ".sd2psx",
            ".sd2psx/settings.ini",
            "memorycards/PS2/BOOT/BootCard-1.mcd",
            "MemoryCards",
            "civ.bin",
            "CIV.BIN",
        ] {
            assert!(root.protects(&rel(path)), "{path}");
        }
        for path in [
            "",
            "CD",
            "CD/game.iso",
            "UPDT/OPL.ELF",
            ".yaoplm/journal",
            "PS2",
            "os",
        ] {
            assert!(!root.protects(&rel(path)), "{path}");
        }
        // A folder inside the SD reaches none of them; a folder inside one
        // of them is protected through and through.
        assert!(!sd(&[".sd2psx"], "Games").protects(&rel("MemoryCards")));
        assert!(sd(&[".sd2psx"], "MemoryCards/PS2").protects(&rel("Card1/x.mcd")));
        assert!(sd(&[".sd2psx"], "MemoryCards").protects(&RelPath::root()));
        let mcp2 = EmulatorSd {
            sd2psx: false,
            mcp2: true,
            signs: vec!["mcp2.bin".into()],
            at: String::new(),
        };
        for path in ["mcp2.bin", "os/x", "PS1/SCES_000.00", "PS2/Templates"] {
            assert!(mcp2.protects(&rel(path)), "{path}");
        }
        assert!(!mcp2.protects(&rel("MemoryCards")));
    }

    #[test]
    fn signs_on_the_host() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        assert_eq!(find_on_host(&root, Some(&root)), None);
        // A `MemoryCards` folder without a variant is not one.
        fs::create_dir_all(root.join("MemoryCards/Other")).unwrap();
        assert_eq!(find_on_host(&root, Some(&root)), None);
        fs::create_dir_all(root.join("MemoryCards/PS2/BOOT")).unwrap();
        let found = find_on_host(&root, Some(&root)).unwrap();
        assert_eq!(found.signs, ["MemoryCards/PS2"]);
        assert!(found.sd2psx && !found.mcp2 && found.at.is_empty());
        fs::create_dir(root.join(".sd2psx")).unwrap();
        fs::create_dir(root.join("MemoryCards/PS1")).unwrap();
        let found = find_on_host(&root, Some(&root)).unwrap();
        assert_eq!(
            found.signs,
            [".sd2psx", "MemoryCards/PS2", "MemoryCards/PS1"]
        );
        // A folder inside the SD sees the SD's files at the volume root.
        fs::create_dir_all(root.join("Games/PS2")).unwrap();
        let inner = root.join("Games/PS2");
        let found = find_on_host(&inner, Some(&root)).unwrap();
        assert_eq!(found.at, "Games/PS2");
        assert!(!found.protects(&rel("MemoryCards")));
        let cards = root.join("MemoryCards/PS2");
        assert!(
            find_on_host(&cards, Some(&root))
                .unwrap()
                .protects(&rel("BOOT"))
        );
        // Without the volume root only the folder itself counts.
        assert_eq!(find_on_host(&inner, None), None);
        // The nearest folder with the signs wins: a copy of the SD kept on
        // the computer, opened at a folder inside it.
        let copy = root.join("Games/backup");
        fs::create_dir_all(copy.join("MemoryCards/PS2/BOOT")).unwrap();
        let boot = fs::canonicalize(copy.join("MemoryCards/PS2/BOOT")).unwrap();
        let found = find_on_host(&boot, Some(&root)).unwrap();
        assert_eq!(
            (found.at.as_str(), found.signs.len()),
            ("MemoryCards/PS2/BOOT", 1)
        );
        // Nothing above the volume root counts.
        assert_eq!(find_on_host(&boot, Some(&copy.join("MemoryCards"))), None);
    }

    #[test]
    fn memcard_pro_2() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        // `os/` alone is too common to count.
        fs::create_dir(root.join("os")).unwrap();
        assert_eq!(find_on_host(&root, None), None);
        fs::create_dir(root.join("PS2")).unwrap();
        let found = find_on_host(&root, None).unwrap();
        assert!(found.mcp2 && !found.sd2psx);
        assert_eq!(found.signs, ["os"]);
        fs::remove_dir(root.join("os")).unwrap();
        fs::write(root.join("mcp2.bin"), b"fw").unwrap();
        assert_eq!(find_on_host(&root, None).unwrap().signs, ["mcp2.bin"]);
    }

    #[test]
    fn short_8_3_aliases_of_the_service_folders_are_protected_too() {
        // On FAT32 the OS resolves `MEMORY~1` to `MemoryCards` and
        // `SD2PSX~1` to `.sd2psx` (review of PR 1): the names alone must
        // not let a plan through.
        let root = sd(&[".sd2psx"], "");
        for path in [
            "MEMORY~1",
            "MEMORY~1/CFG",
            "memory~2/PS2/BOOT",
            "MEMOR~12/x",
            "SD2PSX~1/settings.ini",
        ] {
            assert!(root.protects(&rel(path)), "{path}");
        }
        for path in [
            "MEMORYCA",
            "MEMORY~",
            "MEMORY~X",
            "CD~1",
            "GAMES~1/MEMORY~1",
        ] {
            assert!(!root.protects(&rel(path)), "{path}");
        }
        let mcp2 = EmulatorSd {
            sd2psx: false,
            mcp2: true,
            signs: vec!["mcp2.bin".into()],
            at: String::new(),
        };
        assert!(!mcp2.protects(&rel("MEMORY~1")));
    }

    #[test]
    fn an_unreadable_card_folder_still_counts() {
        // Failing closed: a `MemoryCards/` that cannot be listed keeps the
        // SD protected instead of dropping the signs found so far.
        let top = [DirEntry {
            name: "MemoryCards".into(),
            is_dir: true,
            len: 0,
        }];
        let denied = |_: &str| -> io::Result<Vec<DirEntry>> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        };
        let found = read_signs(&top, &denied).unwrap().unwrap();
        assert!(found.sd2psx);
        assert_eq!(found.signs, ["MemoryCards"]);
    }
}
