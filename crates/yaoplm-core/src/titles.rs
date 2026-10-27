//! The bundled title database (META-1, ADR-0024): serial → title, region,
//! related serials and Redump names.
//!
//! `data/titles.json.gz` is built by `scripts/build-data.py` from PCSX2's
//! `GameIndex.yaml` (GPL-3.0) and the libretro-database PS2 DAT (CC BY-SA
//! 4.0, Redump data); credits are in `data/ATTRIBUTION.md`. The file is
//! embedded in the binary and decoded on first use: about 13 000 entries,
//! a few milliseconds and a few megabytes, with no file to locate at run
//! time in the CLI, the server or the desktop app.
//!
//! IDs are accepted in both spellings (`SLUS_200.62`, `SLUS-20062`) and
//! with region or revision suffixes (`SLES-54793/ANZ`); see
//! [`yaoplm_formats::serial`].

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use serde::Deserialize;
use yaoplm_formats::serial::lookup_key;

use crate::Media;

static DATA: &[u8] = include_bytes!("../../../data/titles.json.gz");

#[derive(Deserialize)]
struct RawGame {
    /// `name` in GameIndex.
    #[serde(default)]
    t: Option<String>,
    /// `name-en`, only when it differs from `name`.
    #[serde(default)]
    e: Option<String>,
    /// `region` in GameIndex: `NTSC-U`, `PAL-M5`...
    #[serde(default)]
    r: Option<String>,
    /// `memcardFilters`: serials of the same game family.
    #[serde(default)]
    k: Vec<String>,
    /// Redump `(name, region)` pairs from the DAT.
    #[serde(default)]
    d: Vec<(String, String)>,
    /// `CD` or `DVD`: Redump dumps a CD as BIN/CUE and a DVD as one ISO,
    /// so the roms' extensions in the DAT say which disc the serial is on.
    #[serde(default)]
    m: Option<String>,
}

#[derive(Deserialize)]
struct RawSource {
    #[serde(default)]
    version: String,
}

#[derive(Deserialize)]
struct RawSources {
    #[serde(default)]
    libretro: Option<RawSource>,
}

#[derive(Deserialize)]
struct RawDb {
    format: u32,
    built: String,
    #[serde(default)]
    sources: Option<RawSources>,
    games: HashMap<String, RawGame>,
}

/// A Redump disc name, which names the file in libretro-thumbnails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedumpName {
    /// `Grand Theft Auto III (USA, Canada)`.
    pub name: String,
    /// `USA`, `Europe`, `Japan`, `Germany`...
    pub region: String,
}

/// What the database knows about one serial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `SLUS-20062`.
    pub serial: String,
    /// The title PCSX2 shows; in Japanese for most NTSC-J games.
    pub title: Option<String>,
    /// A romanised or English title, when it differs from `title`.
    pub title_en: Option<String>,
    /// PCSX2's region code: `NTSC-U`, `NTSC-J`, `PAL-E`, `PAL-M5`...
    pub region: Option<String>,
    /// Serials of the same game family (other regions, discs, bundles).
    pub related: Vec<String>,
    /// Redump names of the discs with this serial.
    pub redump: Vec<RedumpName>,
    /// The disc the serial was pressed on, when Redump dumped only one kind.
    pub media: Option<Media>,
}

impl Entry {
    /// The title to show and to put in file names: `title_en` when `title`
    /// is written in a script a FAT device cannot store, otherwise `title`;
    /// a Redump name without its tags when PCSX2 has no title at all.
    pub fn preferred_title(&self) -> Option<String> {
        match (&self.title, &self.title_en) {
            (Some(title), Some(en)) if !is_latin(title) => Some(en.clone()),
            (Some(title), _) => Some(title.clone()),
            (None, Some(en)) => Some(en.clone()),
            (None, None) => self.redump.first().map(|r| strip_tags(&r.name)),
        }
    }

    /// Redump names in the order worth trying for this serial: the ones
    /// whose region matches PCSX2's region first, then the rest.
    pub fn redump_names_by_region(&self) -> Vec<&RedumpName> {
        let wanted = self.region.as_deref().map(redump_region);
        let mut names: Vec<&RedumpName> = self.redump.iter().collect();
        if let Some(wanted) = wanted {
            names.sort_by_key(|n| {
                if n.region == wanted {
                    0
                } else if same_family(&n.region, wanted) {
                    1
                } else {
                    2
                }
            });
        }
        names
    }
}

/// Every letter is Latin (ASCII, Latin-1, Latin Extended): punctuation and
/// symbols do not count, kana, kanji, hangul and Cyrillic do.
fn is_latin(text: &str) -> bool {
    text.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| (c as u32) < 0x0250 || (0x1E00..=0x1EFF).contains(&(c as u32)))
}

/// `Grand Theft Auto III (USA, Canada) (v1.60)` → `Grand Theft Auto III`.
pub fn strip_tags(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The Redump region word PCSX2's region code corresponds to.
pub fn redump_region(pcsx2: &str) -> &'static str {
    let code = pcsx2.to_ascii_uppercase();
    let (system, rest) = code.split_once('-').unwrap_or((code.as_str(), ""));
    let first = rest.split('-').next().unwrap_or("");
    match (system, first) {
        ("NTSC", "U") => "USA",
        ("NTSC", "J") => "Japan",
        ("NTSC", "K") => "Korea",
        ("NTSC", "C") => "China",
        ("NTSC", "HK" | "T") => "Asia",
        ("NTSC", _) => "USA",
        ("PAL", "G" | "D") => "Germany",
        ("PAL", "F" | "FR") => "France",
        ("PAL", "I" | "IT") => "Italy",
        ("PAL", "S" | "SP") => "Spain",
        ("PAL", "A" | "AU") => "Australia",
        ("PAL", "R" | "RU") => "Russia",
        ("PAL", "P" | "PT") => "Portugal",
        ("PAL", "NL" | "DU") => "Netherlands",
        ("PAL", "PL") => "Poland",
        ("PAL", "SW" | "SE") => "Sweden",
        ("PAL", "FI") => "Finland",
        ("PAL", "GR") => "Greece",
        ("PAL", "BE") => "Belgium",
        ("PAL", "NO") => "Norway",
        ("PAL", "UK") => "Europe",
        _ => "Europe",
    }
}

const EUROPE: [&str; 22] = [
    "Europe",
    "Germany",
    "France",
    "Italy",
    "Spain",
    "Australia",
    "Russia",
    "Portugal",
    "Netherlands",
    "Poland",
    "Sweden",
    "Finland",
    "Greece",
    "Belgium",
    "Norway",
    "Denmark",
    "Austria",
    "Switzerland",
    "Ireland",
    "Croatia",
    "South Africa",
    "India",
];

/// Whether two Redump regions share a video standard and language set
/// closely enough for one cover to stand in for the other.
fn same_family(a: &str, b: &str) -> bool {
    let europe = |r: &str| EUROPE.contains(&r);
    let asia = |r: &str| matches!(r, "Japan" | "Asia" | "Korea" | "China" | "Taiwan");
    let america = |r: &str| matches!(r, "USA" | "Canada" | "Brazil");
    (europe(a) && europe(b)) || (asia(a) && asia(b)) || (america(a) && america(b))
}

/// Build information for the About screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbInfo {
    pub format: u32,
    /// ISO date of the build.
    pub built: String,
    /// Version string of the libretro DAT (`2026.10.07`).
    pub libretro_version: String,
    pub entries: usize,
}

pub struct Database {
    info: DbInfo,
    games: HashMap<String, Entry>,
    /// Serials by Redump title without tags, lower case: the releases of
    /// one game in other regions and languages.
    by_title: HashMap<String, Vec<String>>,
}

impl Database {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut json)
            .map_err(|e| format!("gzip: {e}"))?;
        let raw: RawDb = serde_json::from_slice(&json).map_err(|e| format!("json: {e}"))?;
        let games = raw
            .games
            .into_iter()
            .map(|(serial, g)| {
                let entry = Entry {
                    serial: serial.clone(),
                    title: g.t,
                    title_en: g.e,
                    region: g.r,
                    related: g.k,
                    redump: g
                        .d
                        .into_iter()
                        .map(|(name, region)| RedumpName { name, region })
                        .collect(),
                    media: match g.m.as_deref() {
                        Some("CD") => Some(Media::Cd),
                        Some("DVD") => Some(Media::Dvd),
                        _ => None,
                    },
                };
                (serial, entry)
            })
            .collect::<HashMap<_, _>>();
        let mut by_title: HashMap<String, Vec<String>> = HashMap::new();
        for entry in games.values() {
            let mut titles: Vec<String> = entry
                .redump
                .iter()
                .map(|r| strip_tags(&r.name).to_lowercase())
                .filter(|t| !t.is_empty())
                .collect();
            titles.dedup();
            for title in titles {
                by_title
                    .entry(title)
                    .or_default()
                    .push(entry.serial.clone());
            }
        }
        for serials in by_title.values_mut() {
            serials.sort();
            serials.dedup();
        }
        Ok(Self {
            info: DbInfo {
                format: raw.format,
                built: raw.built,
                libretro_version: raw
                    .sources
                    .and_then(|s| s.libretro)
                    .map(|s| s.version)
                    .unwrap_or_default(),
                entries: games.len(),
            },
            games,
            by_title,
        })
    }

    fn empty() -> Self {
        Self {
            info: DbInfo {
                format: 0,
                built: String::new(),
                libretro_version: String::new(),
                entries: 0,
            },
            games: HashMap::new(),
            by_title: HashMap::new(),
        }
    }

    pub fn info(&self) -> &DbInfo {
        &self.info
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// Looks a game up by either spelling of its ID.
    pub fn lookup(&self, id: &str) -> Option<&Entry> {
        self.games.get(&lookup_key(id))
    }

    /// Other serials whose Redump name, without its tags, is the same as
    /// one of `entry`'s: the game's releases elsewhere, whose screenshots
    /// and logos stand in for its own (ADR-0035). Sorted by serial.
    pub fn same_title(&self, entry: &Entry) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = entry
            .redump
            .iter()
            .filter_map(|r| self.by_title.get(&strip_tags(&r.name).to_lowercase()))
            .flatten()
            .filter(|serial| **serial != entry.serial)
            .filter_map(|serial| self.games.get(serial))
            .collect();
        out.sort_by(|a, b| a.serial.cmp(&b.serial));
        out.dedup_by(|a, b| a.serial == b.serial);
        out
    }

    /// Every serial, in no particular order (coverage measurements).
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.games.values()
    }
}

/// The embedded database, decoded on first use. A build with a broken data
/// file gets an empty database rather than a panic; the tests check it is
/// not empty.
pub fn database() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| Database::decode(DATA).unwrap_or_else(|_| Database::empty()))
}

/// The title to use for `id`, if the database knows it.
pub fn known_title(id: &str) -> Option<String> {
    database().lookup(id).and_then(Entry::preferred_title)
}

/// The disc `id` was pressed on, if the database knows it (IMP-1).
pub fn known_media(id: &str) -> Option<Media> {
    database().lookup(id).and_then(|entry| entry.media)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_is_embedded_and_large() {
        let db = database();
        assert!(db.info().entries > 12_000, "{:?}", db.info());
        assert_eq!(db.info().format, 1);
        assert!(db.info().built.starts_with("20"));
        assert!(!db.info().libretro_version.is_empty());
    }

    // Redump dumps a CD as BIN/CUE and a DVD as one ISO: the database keeps
    // which it was for every serial (IMP-1).
    #[test]
    fn database_knows_the_disc_type() {
        assert_eq!(known_media("SLUS_200.62"), Some(Media::Dvd), "GTA III");
        assert_eq!(
            known_media("SCUS_906.82"),
            Some(Media::Dvd),
            "Gran Turismo 4"
        );
        assert_eq!(known_media("SLUS_200.02"), Some(Media::Cd), "Ridge Racer V");
        assert_eq!(known_media("ZZZZ_000.00"), None);
    }

    #[test]
    fn lookup_accepts_both_spellings() {
        let db = database();
        let gta = db.lookup("SLUS_200.62").unwrap();
        assert_eq!(gta.serial, "SLUS-20062");
        assert_eq!(gta.title.as_deref(), Some("Grand Theft Auto III"));
        assert_eq!(gta.region.as_deref(), Some("NTSC-U"));
        assert_eq!(gta.redump[0].name, "Grand Theft Auto III (USA, Canada)");
        assert_eq!(db.lookup("SLUS-20062").unwrap().serial, gta.serial);
        assert_eq!(db.lookup("slus-20062/anz").unwrap().serial, gta.serial);
        assert!(db.lookup("ZZZZ_000.00").is_none());
        assert!(db.lookup("DATA\\MAIN.E").is_none());
        assert_eq!(
            known_title("SLUS_200.62").as_deref(),
            Some("Grand Theft Auto III")
        );
    }

    #[test]
    fn releases_of_the_same_game_elsewhere() {
        let db = database();
        // Dead or Alive 2 in Japan and Europe share a Redump title; the US
        // "DOA2 - Hardcore" does not.
        let jp = db.lookup("SLPS-25002").unwrap();
        let others: Vec<&str> = db
            .same_title(jp)
            .iter()
            .map(|e| e.serial.as_str())
            .collect();
        assert!(others.contains(&"SCES-50003"), "{others:?}");
        assert!(!others.contains(&"SLUS-20071") && !others.contains(&"SLPS-25002"));
        assert!(db.entries().count() > 12_000);
    }

    #[test]
    fn japanese_titles_fall_back_to_english() {
        let db = database();
        let entry = db.lookup("SLPM-65002").unwrap();
        assert!(entry.title.as_deref().is_some_and(|t| !is_latin(t)));
        assert_eq!(
            entry.preferred_title().as_deref(),
            Some("0 Story [Disc 1 of 2]")
        );
        // A DAT-only entry gets its Redump name without the tags.
        let dat_only = Entry {
            serial: "KRT-005".into(),
            title: None,
            title_en: None,
            region: None,
            related: Vec::new(),
            redump: vec![RedumpName {
                name: "Karat PS2-you Pro Action Replay (Japan) (v1.7)".into(),
                region: "Japan".into(),
            }],
            media: None,
        };
        assert_eq!(
            dat_only.preferred_title().as_deref(),
            Some("Karat PS2-you Pro Action Replay")
        );
        assert!(is_latin("Pokémon Colosseum – Édition «Spéciale»"));
        assert!(!is_latin("Тетрис"));
    }

    #[test]
    fn redump_names_prefer_the_region() {
        let entry = Entry {
            serial: "SLES-50330".into(),
            title: Some("GTA III".into()),
            title_en: None,
            region: Some("PAL-G".into()),
            related: Vec::new(),
            redump: vec![
                RedumpName {
                    name: "GTA III (Europe)".into(),
                    region: "Europe".into(),
                },
                RedumpName {
                    name: "GTA III (USA)".into(),
                    region: "USA".into(),
                },
                RedumpName {
                    name: "GTA III (Germany)".into(),
                    region: "Germany".into(),
                },
            ],
            media: None,
        };
        let names: Vec<&str> = entry
            .redump_names_by_region()
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["GTA III (Germany)", "GTA III (Europe)", "GTA III (USA)"]
        );
        assert_eq!(redump_region("NTSC-U"), "USA");
        assert_eq!(redump_region("PAL-M5"), "Europe");
        assert_eq!(redump_region("ntsc-j"), "Japan");
        assert_eq!(redump_region("PAL-F-G"), "France");
        assert_eq!(strip_tags("A (B) [C] D (E (F))"), "A D");
    }
}
