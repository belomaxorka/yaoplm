//! The bundled game info table (META-3, ADR-0050): serial → developer,
//! publisher, release date, player count and genre, plus the Wikidata item
//! of the game where one could be matched.
//!
//! `data/game-info.json.gz` is built by `scripts/build-game-info.py` from
//! libretro-database's developer DAT (CC BY-SA 4.0; PSXDataCenter facts,
//! descriptions never read) and a Wikidata extract (CC0) matched by
//! PlayStation ID or by title; credits are in `data/ATTRIBUTION.md`. Like
//! the title database (ADR-0024) the file is embedded in the binary and
//! decoded on first use.
//!
//! What the table gives is facts to show and, by a plan, to write into
//! `CFG/<ID>.cfg` (research/23 §1): it never carries a description.

use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::sync::OnceLock;

use serde::Deserialize;
use yaoplm_formats::serial::lookup_key;

static DATA: &[u8] = include_bytes!("../../../data/game-info.json.gz");

/// The genre vocabulary (ADR-0050): one short list, the same on the card,
/// in the plan, in the CLI and in the CFG. The codes are the i18n keys
/// `genre.<code>`; `scripts/build-game-info.py` maps the sources onto them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Genre {
    Action,
    ActionAdventure,
    Adventure,
    Platform,
    Shooter,
    Fighting,
    BeatEmUp,
    Racing,
    Sports,
    Rpg,
    Strategy,
    Simulation,
    Puzzle,
    Music,
    Party,
    Board,
    Card,
    Casino,
    Mahjong,
    Quiz,
    Horror,
    VisualNovel,
    Educational,
    Pinball,
    Other,
}

impl Genre {
    pub const ALL: [Genre; 25] = [
        Genre::Action,
        Genre::ActionAdventure,
        Genre::Adventure,
        Genre::Platform,
        Genre::Shooter,
        Genre::Fighting,
        Genre::BeatEmUp,
        Genre::Racing,
        Genre::Sports,
        Genre::Rpg,
        Genre::Strategy,
        Genre::Simulation,
        Genre::Puzzle,
        Genre::Music,
        Genre::Party,
        Genre::Board,
        Genre::Card,
        Genre::Casino,
        Genre::Mahjong,
        Genre::Quiz,
        Genre::Horror,
        Genre::VisualNovel,
        Genre::Educational,
        Genre::Pinball,
        Genre::Other,
    ];

    /// The code in the data file and the i18n key suffix.
    pub fn code(self) -> &'static str {
        match self {
            Genre::Action => "action",
            Genre::ActionAdventure => "action-adventure",
            Genre::Adventure => "adventure",
            Genre::Platform => "platform",
            Genre::Shooter => "shooter",
            Genre::Fighting => "fighting",
            Genre::BeatEmUp => "beat-em-up",
            Genre::Racing => "racing",
            Genre::Sports => "sports",
            Genre::Rpg => "rpg",
            Genre::Strategy => "strategy",
            Genre::Simulation => "simulation",
            Genre::Puzzle => "puzzle",
            Genre::Music => "music",
            Genre::Party => "party",
            Genre::Board => "board",
            Genre::Card => "card",
            Genre::Casino => "casino",
            Genre::Mahjong => "mahjong",
            Genre::Quiz => "quiz",
            Genre::Horror => "horror",
            Genre::VisualNovel => "visual-novel",
            Genre::Educational => "educational",
            Genre::Pinball => "pinball",
            Genre::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Option<Genre> {
        Genre::ALL.into_iter().find(|g| g.code() == code)
    }

    /// The English label, as the `en` catalogue has it (`genre.<code>` in
    /// `web/src/locales/en/translation.json`): what the CFG `Genre` key
    /// and the CLI print.
    pub fn label_en(self) -> &'static str {
        yaoplm_i18n::text(&format!("genre.{}", self.code())).unwrap_or_else(|| self.code())
    }
}

/// A release date as the sources give it: to the day, the month or only
/// the year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReleaseDate {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl ReleaseDate {
    /// `2001-10-23`, `2001-10` or `2001`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year: u16 = parts
            .next()?
            .parse()
            .ok()
            .filter(|y| (1990..=2100).contains(y))?;
        let month = match parts.next() {
            None => None,
            Some(m) => Some(m.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?),
        };
        let day = match parts.next() {
            None => None,
            Some(d) => Some(d.parse::<u8>().ok().filter(|d| (1..=31).contains(d))?),
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The ISO form the sources use.
    pub fn iso(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ReleaseDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(month) = self.month {
            write!(f, "-{month:02}")?;
            if let Some(day) = self.day {
                write!(f, "-{day:02}")?;
            }
        }
        Ok(())
    }
}

/// Where a value comes from, shown next to it (ADR-0050).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfoSource {
    /// libretro-database's developer DAT: PSXDataCenter's facts for the
    /// serial itself.
    Libretro,
    /// The Wikidata item of the game, which covers every regional release.
    Wikidata,
}

/// Facts for one serial from the developer DAT.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub released: Option<ReleaseDate>,
    pub players: Option<u32>,
    pub genres: Vec<Genre>,
}

/// How a Wikidata item was tied to the serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchedBy {
    /// The item's PlayStation ID (`P2606`) is this serial.
    Id,
    /// The item's English label or article title equals the serial's title
    /// in the bundled title database, and no other PS2 item has it.
    Title,
}

/// The Wikidata item of the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikidataItem {
    /// `Q149106`.
    pub qid: String,
    pub matched_by: MatchedBy,
    pub label_en: Option<String>,
    pub label_ru: Option<String>,
    /// The English Wikipedia article title, when there is one.
    pub article_en: Option<String>,
    /// The Russian Wikipedia article title, when there is one.
    pub article_ru: Option<String>,
    pub genres: Vec<Genre>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    /// The earliest publication date of any release.
    pub released: Option<ReleaseDate>,
}

impl WikidataItem {
    pub fn url(&self) -> String {
        format!("https://www.wikidata.org/wiki/{}", self.qid)
    }
}

/// One value with its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    pub source: InfoSource,
}

/// What the table knows about one serial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// `SLUS-20062`.
    pub serial: String,
    pub facts: Option<Facts>,
    pub wikidata: Option<WikidataItem>,
}

impl GameInfo {
    /// The developer to show: the DAT's (the serial's own release) before
    /// Wikidata's (the title, any region).
    pub fn developer(&self) -> Option<Sourced<String>> {
        self.text(
            |f| f.developer.as_deref(),
            |w| w.developers.first().map(String::as_str),
        )
    }

    pub fn publisher(&self) -> Option<Sourced<String>> {
        self.text(
            |f| f.publisher.as_deref(),
            |w| w.publishers.first().map(String::as_str),
        )
    }

    /// The release date: the serial's own from the DAT, else the game's
    /// earliest from Wikidata.
    pub fn released(&self) -> Option<Sourced<ReleaseDate>> {
        if let Some(date) = self.facts.as_ref().and_then(|f| f.released) {
            return Some(Sourced {
                value: date,
                source: InfoSource::Libretro,
            });
        }
        self.wikidata
            .as_ref()
            .and_then(|w| w.released)
            .map(|value| Sourced {
                value,
                source: InfoSource::Wikidata,
            })
    }

    /// The player count; Wikidata has none worth the name.
    pub fn players(&self) -> Option<Sourced<u32>> {
        self.facts
            .as_ref()
            .and_then(|f| f.players)
            .map(|value| Sourced {
                value,
                source: InfoSource::Libretro,
            })
    }

    /// The genres: the DAT's one word, else Wikidata's.
    pub fn genres(&self) -> Option<Sourced<Vec<Genre>>> {
        if let Some(genres) = self.facts.as_ref().filter(|f| !f.genres.is_empty()) {
            return Some(Sourced {
                value: genres.genres.clone(),
                source: InfoSource::Libretro,
            });
        }
        self.wikidata
            .as_ref()
            .filter(|w| !w.genres.is_empty())
            .map(|w| Sourced {
                value: w.genres.clone(),
                source: InfoSource::Wikidata,
            })
    }

    fn text(
        &self,
        from_facts: impl Fn(&Facts) -> Option<&str>,
        from_wikidata: impl Fn(&WikidataItem) -> Option<&str>,
    ) -> Option<Sourced<String>> {
        if let Some(value) = self.facts.as_ref().and_then(from_facts) {
            return Some(Sourced {
                value: value.to_owned(),
                source: InfoSource::Libretro,
            });
        }
        self.wikidata
            .as_ref()
            .and_then(from_wikidata)
            .map(|value| Sourced {
                value: value.to_owned(),
                source: InfoSource::Wikidata,
            })
    }
}

#[derive(Deserialize)]
struct RawFacts {
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    released: Option<String>,
    #[serde(default)]
    players: Option<u32>,
    #[serde(default)]
    genres: Vec<String>,
}

#[derive(Deserialize)]
struct RawWikidata {
    qid: String,
    #[serde(rename = "match")]
    method: String,
    #[serde(default)]
    en: Option<String>,
    #[serde(default)]
    ru: Option<String>,
    #[serde(default)]
    enwiki: Option<String>,
    #[serde(default)]
    ruwiki: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    developer: Vec<String>,
    #[serde(default)]
    publisher: Vec<String>,
    #[serde(default)]
    released: Option<String>,
}

#[derive(Deserialize)]
struct RawGame {
    #[serde(default)]
    facts: Option<RawFacts>,
    #[serde(default)]
    wikidata: Option<RawWikidata>,
}

#[derive(Deserialize)]
struct RawSource {
    #[serde(default)]
    version: String,
    #[serde(default)]
    items: Option<u32>,
}

#[derive(Deserialize)]
struct RawDb {
    format: u32,
    built: String,
    #[serde(default)]
    sources: HashMap<String, RawSource>,
    games: HashMap<String, RawGame>,
}

/// Build information for the About screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbInfo {
    pub format: u32,
    /// ISO date of the build.
    pub built: String,
    /// Version string of the libretro developer DAT (`2020.3.19`).
    pub libretro_version: String,
    /// ISO date the Wikidata extract was fetched.
    pub wikidata_fetched: String,
    /// PS2 items in the extract.
    pub wikidata_items: u32,
    /// Serials with any record.
    pub entries: usize,
    pub with_facts: usize,
    pub with_wikidata: usize,
}

pub struct Database {
    info: DbInfo,
    games: HashMap<String, GameInfo>,
}

fn genres(codes: &[String]) -> Vec<Genre> {
    codes.iter().filter_map(|c| Genre::from_code(c)).collect()
}

impl Database {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut json)
            .map_err(|e| format!("gzip: {e}"))?;
        let raw: RawDb = serde_json::from_slice(&json).map_err(|e| format!("json: {e}"))?;
        let games: HashMap<String, GameInfo> = raw
            .games
            .into_iter()
            .map(|(serial, g)| {
                let facts = g.facts.map(|f| Facts {
                    developer: f.developer,
                    publisher: f.publisher,
                    released: f.released.as_deref().and_then(ReleaseDate::parse),
                    players: f.players,
                    genres: genres(&f.genres),
                });
                let wikidata = g.wikidata.map(|w| WikidataItem {
                    qid: w.qid,
                    matched_by: if w.method == "id" {
                        MatchedBy::Id
                    } else {
                        MatchedBy::Title
                    },
                    label_en: w.en,
                    label_ru: w.ru,
                    article_en: w.enwiki,
                    article_ru: w.ruwiki,
                    genres: genres(&w.genres),
                    developers: w.developer,
                    publishers: w.publisher,
                    released: w.released.as_deref().and_then(ReleaseDate::parse),
                });
                let info = GameInfo {
                    serial: serial.clone(),
                    facts,
                    wikidata,
                };
                (serial, info)
            })
            .collect();
        let source = |name: &str| raw.sources.get(name);
        Ok(Self {
            info: DbInfo {
                format: raw.format,
                built: raw.built,
                libretro_version: source("libretro-developer")
                    .map(|s| s.version.clone())
                    .unwrap_or_default(),
                wikidata_fetched: source("wikidata")
                    .map(|s| s.version.clone())
                    .unwrap_or_default(),
                wikidata_items: source("wikidata").and_then(|s| s.items).unwrap_or(0),
                entries: games.len(),
                with_facts: games.values().filter(|g| g.facts.is_some()).count(),
                with_wikidata: games.values().filter(|g| g.wikidata.is_some()).count(),
            },
            games,
        })
    }

    fn empty() -> Self {
        Self {
            info: DbInfo {
                format: 0,
                built: String::new(),
                libretro_version: String::new(),
                wikidata_fetched: String::new(),
                wikidata_items: 0,
                entries: 0,
                with_facts: 0,
                with_wikidata: 0,
            },
            games: HashMap::new(),
        }
    }

    pub fn info(&self) -> &DbInfo {
        &self.info
    }

    /// Looks a game up by either spelling of its ID.
    pub fn lookup(&self, id: &str) -> Option<&GameInfo> {
        self.games.get(&lookup_key(id))
    }

    /// Every serial, in no particular order (coverage measurements).
    pub fn entries(&self) -> impl Iterator<Item = &GameInfo> {
        self.games.values()
    }
}

/// The embedded table, decoded on first use. A build with a broken data
/// file gets an empty table rather than a panic; the tests check it is
/// not empty.
pub fn database() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    DB.get_or_init(|| Database::decode(DATA).unwrap_or_else(|_| Database::empty()))
}

/// What the table knows about `id`, if anything.
pub fn known_info(id: &str) -> Option<&'static GameInfo> {
    database().lookup(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_embedded_and_large() {
        let db = database();
        let info = db.info();
        assert!(info.entries > 8_000, "{info:?}");
        assert!(
            info.with_facts > 4_500 && info.with_wikidata > 5_000,
            "{info:?}"
        );
        assert_eq!(info.format, 1);
        assert!(info.built.starts_with("20") && info.wikidata_fetched.starts_with("20"));
        assert!(!info.libretro_version.is_empty() && info.wikidata_items > 3_000);
        assert!(db.entries().count() == info.entries);
    }

    // The fixture's games: GTA III has both records, Dead or Alive 2 the
    // DAT's only, Tak both; the ID is accepted in both spellings.
    #[test]
    fn lookup_accepts_both_spellings() {
        let gta = known_info("SLUS_200.62").expect("GTA III");
        assert_eq!(gta.serial, "SLUS-20062");
        assert_eq!(known_info("SLUS-20062"), Some(gta));
        assert_eq!(known_info("slus-20062/anz"), Some(gta));
        let facts = gta.facts.as_ref().unwrap();
        assert_eq!(facts.developer.as_deref(), Some("DMA Design"));
        assert_eq!(facts.publisher.as_deref(), Some("Rockstar Games"));
        assert_eq!(
            facts.released.map(|d| d.iso()).as_deref(),
            Some("2001-10-23")
        );
        assert_eq!(facts.players, Some(1));
        assert_eq!(facts.genres, [Genre::Action]);
        let wd = gta.wikidata.as_ref().unwrap();
        assert_eq!(wd.qid, "Q149106");
        assert_eq!(wd.matched_by, MatchedBy::Title);
        assert_eq!(wd.article_en.as_deref(), Some("Grand Theft Auto III"));
        assert_eq!(wd.article_ru.as_deref(), Some("Grand Theft Auto III"));
        assert_eq!(wd.genres, [Genre::ActionAdventure]);
        assert_eq!(wd.url(), "https://www.wikidata.org/wiki/Q149106");
        let doa = known_info("SCES_500.03").expect("Dead or Alive 2");
        assert_eq!(doa.facts.as_ref().unwrap().players, Some(4));
        assert!(
            doa.wikidata.is_none(),
            "no PS2 item carries its European title"
        );
        assert!(known_info("ZZZZ_000.00").is_none());
        assert!(known_info("DATA\\MAIN.E").is_none());
    }

    // The DAT speaks for the serial itself, Wikidata for the title in any
    // region: the DAT's value comes first, Wikidata's fills the gaps.
    #[test]
    fn summary_prefers_the_serial_own_facts() {
        let gta = known_info("SLUS_200.62").unwrap();
        let developer = gta.developer().unwrap();
        assert_eq!(
            (developer.value.as_str(), developer.source),
            ("DMA Design", InfoSource::Libretro)
        );
        assert_eq!(gta.genres().unwrap().value, [Genre::Action]);
        assert_eq!(gta.players().unwrap().value, 1);
        // ChoroQ (SLUS-20930) has no DAT record: everything is Wikidata's.
        let choro = known_info("SLUS_209.30").expect("ChoroQ");
        assert!(choro.facts.is_none());
        let developer = choro.developer().unwrap();
        assert_eq!(developer.source, InfoSource::Wikidata);
        assert_eq!(choro.genres().unwrap().source, InfoSource::Wikidata);
        assert!(choro.players().is_none());
        assert_eq!(choro.released().unwrap().source, InfoSource::Wikidata);
    }

    #[test]
    fn genre_codes_round_trip() {
        for genre in Genre::ALL {
            assert_eq!(Genre::from_code(genre.code()), Some(genre));
            assert_ne!(genre.label_en(), genre.code(), "no en label for {genre:?}");
        }
        assert_eq!(Genre::from_code("knitting"), None);
        assert_eq!(Genre::BeatEmUp.code(), "beat-em-up");
        assert_eq!(Genre::Rpg.label_en(), "RPG");
    }

    #[test]
    fn release_dates() {
        let full = ReleaseDate::parse("2001-10-23").unwrap();
        assert_eq!(
            (full.year, full.month, full.day),
            (2001, Some(10), Some(23))
        );
        assert_eq!(full.to_string(), "2001-10-23");
        assert_eq!(
            ReleaseDate::parse("2001-10").unwrap().to_string(),
            "2001-10"
        );
        assert_eq!(ReleaseDate::parse("2001").unwrap().to_string(), "2001");
        assert_eq!(ReleaseDate::parse("2001-13"), None);
        assert_eq!(ReleaseDate::parse("1899"), None);
        assert_eq!(ReleaseDate::parse("2001-10-23-1"), None);
        assert_eq!(ReleaseDate::parse(""), None);
    }
}
