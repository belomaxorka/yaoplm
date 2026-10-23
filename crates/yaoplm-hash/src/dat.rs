//! Logiqx XML datafiles, the format Redump publishes (META-2).
//!
//! `redump.info/datfile/ps2/serial,version` (checked 2026-10-23) is a zip
//! with one `.dat`:
//!
//! ```text
//! <datafile>
//!   <header><name/><description/><version/><date/><author/><homepage/><url/></header>
//!   <game name="…" id="20886">
//!     <category>Games</category>
//!     <serial>SLPM 65002</serial>          <!-- or "SLES-50539, SLES-50539-P" -->
//!     <version>1.10</version>
//!     <description>…</description>
//!     <rom name="….iso" size="…" crc="…" md5="…" sha1="…"/>
//!   </game>
//! </datafile>
//! ```
//!
//! PS2 DVDs are one `.iso` rom; CDs are a `.cue` plus one `.bin` per track.
//! The plain DAT (without `serial,version`) has no `<serial>`; it still
//! verifies by hash. Unknown elements are skipped, so other Logiqx files
//! (No-Intro, the clrmamepro-to-XML exports) read as well. A hash that is
//! not well-formed hex is dropped rather than failing the whole file.

use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

use crate::digest::parse_hex;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Header {
    pub name: String,
    /// `Sony - PlayStation 2 - Datfile (11845) (2026-10-23 01-56-22)`.
    pub description: String,
    pub version: String,
    pub date: String,
    pub author: String,
    pub homepage: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rom {
    pub name: String,
    pub size: u64,
    pub crc32: Option<u32>,
    pub md5: Option<[u8; 16]>,
    pub sha1: Option<[u8; 20]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Game {
    pub name: String,
    /// Redump's disc id: `https://redump.info/disc/<id>`.
    pub id: Option<String>,
    pub category: Option<String>,
    /// As written: one serial or several separated by commas.
    pub serial: Option<String>,
    /// The version printed on the disc, e.g. `1.10`.
    pub version: Option<String>,
    pub description: Option<String>,
    pub roms: Vec<Rom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Datafile {
    pub header: Header,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatError {
    #[error("not valid XML: {0}")]
    Xml(String),
    #[error("not a Logiqx datafile (no <datafile> element)")]
    NotADatafile,
    #[error("the DAT ends before </datafile>: the file is cut off")]
    Truncated,
}

/// Where text currently goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Header(HeaderField),
    Game(GameField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderField {
    Name,
    Description,
    Version,
    Date,
    Author,
    Homepage,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameField {
    Category,
    Serial,
    Version,
    Description,
}

fn attr(element: &BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| {
            a.normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

fn rom(element: &BytesStart<'_>) -> Option<Rom> {
    let name = attr(element, "name")?;
    let size = attr(element, "size")?.trim().parse().ok()?;
    let crc32 = attr(element, "crc")
        .and_then(|c| parse_hex::<4>(&c))
        .map(u32::from_be_bytes);
    Some(Rom {
        name,
        size,
        crc32,
        md5: attr(element, "md5").and_then(|h| parse_hex(&h)),
        sha1: attr(element, "sha1").and_then(|h| parse_hex(&h)),
    })
}

fn is_game(name: &str) -> bool {
    matches!(name, "game" | "machine")
}

fn field_for(in_header: bool, in_game: bool, name: &str) -> Field {
    if in_game {
        return match name {
            "category" => Field::Game(GameField::Category),
            "serial" => Field::Game(GameField::Serial),
            "version" => Field::Game(GameField::Version),
            "description" => Field::Game(GameField::Description),
            _ => Field::None,
        };
    }
    if in_header {
        return match name {
            "name" => Field::Header(HeaderField::Name),
            "description" => Field::Header(HeaderField::Description),
            "version" => Field::Header(HeaderField::Version),
            "date" => Field::Header(HeaderField::Date),
            "author" => Field::Header(HeaderField::Author),
            "homepage" => Field::Header(HeaderField::Homepage),
            "url" => Field::Header(HeaderField::Url),
            _ => Field::None,
        };
    }
    Field::None
}

fn store(datafile: &mut Datafile, game: Option<&mut Game>, field: Field, text: String) {
    let text = text.trim().to_owned();
    match field {
        Field::None => {}
        Field::Header(f) => {
            let h = &mut datafile.header;
            *match f {
                HeaderField::Name => &mut h.name,
                HeaderField::Description => &mut h.description,
                HeaderField::Version => &mut h.version,
                HeaderField::Date => &mut h.date,
                HeaderField::Author => &mut h.author,
                HeaderField::Homepage => &mut h.homepage,
                HeaderField::Url => &mut h.url,
            } = text;
        }
        Field::Game(f) => {
            if let Some(game) = game {
                *match f {
                    GameField::Category => &mut game.category,
                    GameField::Serial => &mut game.serial,
                    GameField::Version => &mut game.version,
                    GameField::Description => &mut game.description,
                } = Some(text).filter(|t| !t.is_empty());
            }
        }
    }
}

/// Parses a whole datafile.
pub fn parse(xml: &[u8]) -> Result<Datafile, DatError> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut datafile = Datafile::default();
    let mut seen_root = false;
    // quick-xml reports the end of input as Eof even with elements open.
    let mut closed_root = false;
    let mut in_header = false;
    let mut game: Option<Game> = None;
    let mut field = Field::None;
    let mut text = String::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| DatError::Xml(format!("{e} at byte {}", reader.buffer_position())))?;
        match event {
            Event::Start(e) => {
                let name = e.local_name();
                match name.as_ref() {
                    "datafile" => seen_root = true,
                    "header" if game.is_none() => in_header = true,
                    n if is_game(n) => {
                        game = Some(Game {
                            name: attr(&e, "name").unwrap_or_default(),
                            id: attr(&e, "id"),
                            ..Game::default()
                        });
                    }
                    // Redump closes `<rom/>` itself; other tools write
                    // `<rom ...></rom>`.
                    "rom" => {
                        if let (Some(game), Some(rom)) = (game.as_mut(), rom(&e)) {
                            game.roms.push(rom);
                        }
                    }
                    n => {
                        field = field_for(in_header, game.is_some(), n);
                        text.clear();
                    }
                }
            }
            Event::Empty(e) => {
                let name = e.local_name();
                match name.as_ref() {
                    "rom" => {
                        if let (Some(game), Some(rom)) = (game.as_mut(), rom(&e)) {
                            game.roms.push(rom);
                        }
                    }
                    n if is_game(n) => datafile.games.push(Game {
                        name: attr(&e, "name").unwrap_or_default(),
                        id: attr(&e, "id"),
                        ..Game::default()
                    }),
                    "datafile" => {
                        seen_root = true;
                        closed_root = true;
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if field != Field::None {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if field != Field::None {
                    text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if field != Field::None {
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => text.push(c),
                        _ => text.push_str(resolve_predefined_entity(&r).unwrap_or("")),
                    }
                }
            }
            Event::End(e) => {
                let name = e.local_name();
                match name.as_ref() {
                    "datafile" => closed_root = true,
                    "header" => in_header = false,
                    n if is_game(n) => {
                        if let Some(done) = game.take() {
                            datafile.games.push(done);
                        }
                    }
                    _ => {
                        if field != Field::None {
                            store(
                                &mut datafile,
                                game.as_mut(),
                                field,
                                std::mem::take(&mut text),
                            );
                            field = Field::None;
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !seen_root {
        return Err(DatError::NotADatafile);
    }
    if !closed_root {
        return Err(DatError::Truncated);
    }
    Ok(datafile)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<!DOCTYPE datafile PUBLIC "-//Logiqx//DTD ROM Management Datafile//EN" "http://www.logiqx.com/Dats/datafile.dtd">
<datafile>
	<header>
		<name>Sony - PlayStation 2</name>
		<description>Sony - PlayStation 2 - Datfile (3) (2026-10-23 01-56-22)</description>
		<version>2026-10-23 01-56-22</version>
		<date>2026-10-23 01-56-22</date>
		<author>redump.info</author>
		<homepage>redump.info</homepage>
		<url>https://redump.info/</url>
	</header>
	<game name="Grand Theft Auto III (USA)" id="8814">
		<category>Games</category>
		<serial>SLUS-20062, SLUS-20062GH</serial>
		<version>1.40</version>
		<description>Grand Theft Auto III (USA)</description>
		<rom name="Grand Theft Auto III (USA).iso" size="1588658176" crc="1A2B3C4D" md5="9e107d9d372bb6826bd81d3542a419d6" sha1="2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"/>
	</game>
	<game name="Tom &amp; Jerry &#8217;War&#x27; (Europe)" id="1">
		<serial>SLES 51234</serial>
		<description><![CDATA[Tom & Jerry]]></description>
		<rom name="Tom &amp; Jerry (Europe).cue" size="93" crc="c41a1ef7" md5="bad" sha1="af1c6c1676e4eb372603a34ded3bcdef40bc0a22"/>
		<rom name="Tom &amp; Jerry (Europe).bin" size="662814768" crc="10c34719"/>
	</game>
	<machine name="Empty"/>
</datafile>
"#;

    #[test]
    fn reads_redump_layout() {
        let dat = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(dat.header.name, "Sony - PlayStation 2");
        assert_eq!(dat.header.version, "2026-10-23 01-56-22");
        assert_eq!(dat.header.author, "redump.info");
        assert_eq!(dat.games.len(), 3);

        let gta = &dat.games[0];
        assert_eq!(gta.name, "Grand Theft Auto III (USA)");
        assert_eq!(gta.id.as_deref(), Some("8814"));
        assert_eq!(gta.category.as_deref(), Some("Games"));
        assert_eq!(gta.serial.as_deref(), Some("SLUS-20062, SLUS-20062GH"));
        assert_eq!(gta.version.as_deref(), Some("1.40"));
        let iso = &gta.roms[0];
        assert_eq!(iso.size, 1_588_658_176);
        assert_eq!(iso.crc32, Some(0x1A2B_3C4D));
        assert!(iso.md5.is_some() && iso.sha1.is_some());

        let tom = &dat.games[1];
        assert_eq!(tom.name, "Tom & Jerry \u{2019}War' (Europe)");
        assert_eq!(tom.description.as_deref(), Some("Tom & Jerry"));
        assert_eq!(tom.roms.len(), 2);
        assert_eq!(tom.roms[0].name, "Tom & Jerry (Europe).cue");
        // A malformed hash is dropped, the rom stays.
        assert_eq!(tom.roms[0].md5, None);
        assert!(tom.roms[0].sha1.is_some());
        assert_eq!(tom.roms[1].sha1, None);

        assert_eq!(dat.games[2].name, "Empty");
        assert!(dat.games[2].roms.is_empty());
    }

    #[test]
    fn rejects_what_is_not_a_datafile() {
        assert_eq!(parse(b"<html><body/></html>"), Err(DatError::NotADatafile));
        assert_eq!(parse(b""), Err(DatError::NotADatafile));
        assert!(matches!(
            parse(b"<datafile><game name='x'></datafile>"),
            Err(DatError::Xml(_))
        ));
    }

    #[test]
    fn cut_off_dats_and_other_layouts() {
        // A download cut short parses up to the cut, but is refused.
        let cut = &SAMPLE[..SAMPLE.find("<machine").unwrap()];
        assert_eq!(parse(cut.as_bytes()), Err(DatError::Truncated));
        assert!(parse(b"<datafile/>").unwrap().games.is_empty());
        // `<rom></rom>` instead of `<rom/>`, references in text.
        let dat = parse(
            br#"<datafile><game name="A"><description>Tom &amp; Jerry &#233;</description><rom name="a.iso" size="4" sha1="2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"></rom></game></datafile>"#,
        )
        .unwrap();
        assert_eq!(dat.games[0].roms.len(), 1);
        assert_eq!(dat.games[0].description.as_deref(), Some("Tom & Jerry é"));
    }
}
