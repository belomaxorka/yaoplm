//! Mini Logiqx DATs in Redump's layout for verification tests (META-2,
//! GAME-5). Hashes are computed from the bytes given, so a test can make a
//! DAT that a generated image matches, or one it fails.

use md5::Md5;
use sha1::{Digest, Sha1};

/// One `<game>` with its roms.
#[derive(Debug, Clone)]
struct Game {
    name: String,
    id: u32,
    serial: Option<String>,
    roms: Vec<String>,
}

/// Builds a `Sony - PlayStation 2` datafile.
#[derive(Debug, Clone, Default)]
pub struct DatBuilder {
    games: Vec<Game>,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A `<rom>` line for `bytes`.
pub fn rom_line(name: &str, bytes: &[u8]) -> String {
    format!(
        r#"<rom name="{}" size="{}" crc="{:08x}" md5="{}" sha1="{}"/>"#,
        escape(name),
        bytes.len(),
        crc32fast::hash(bytes),
        hex(&Md5::digest(bytes)),
        hex(&Sha1::digest(bytes)),
    )
}

impl DatBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// A DVD game: one `.iso` rom with the hashes of `iso`. `serial` is
    /// written as Redump spells it, e.g. `SLUS-20062` or `SLPM 65002`.
    pub fn iso(mut self, name: &str, serial: Option<&str>, iso: &[u8]) -> Self {
        let id = self.games.len() as u32 + 1000;
        self.games.push(Game {
            name: name.to_owned(),
            id,
            serial: serial.map(str::to_owned),
            roms: vec![rom_line(&format!("{name}.iso"), iso)],
        });
        self
    }

    /// A CD game: a `.cue` and one `.bin` with these bytes.
    pub fn bin_cue(mut self, name: &str, serial: Option<&str>, bin: &[u8]) -> Self {
        let id = self.games.len() as u32 + 1000;
        let cue = format!(
            "FILE \"{name}.bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n"
        );
        self.games.push(Game {
            name: name.to_owned(),
            id,
            serial: serial.map(str::to_owned),
            roms: vec![
                rom_line(&format!("{name}.cue"), cue.as_bytes()),
                rom_line(&format!("{name}.bin"), bin),
            ],
        });
        self
    }

    pub fn build(&self) -> String {
        let mut out = format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE datafile PUBLIC \"-//Logiqx//DTD ROM Management Datafile//EN\" \"http://www.logiqx.com/Dats/datafile.dtd\">\n<datafile>\n\t<header>\n\t\t<name>Sony - PlayStation 2</name>\n\t\t<description>Sony - PlayStation 2 - Datfile ({n}) (2026-10-23 01-56-22)</description>\n\t\t<version>2026-10-23 01-56-22</version>\n\t\t<date>2026-10-23 01-56-22</date>\n\t\t<author>YAOPLM test fixture</author>\n\t\t<homepage>redump.info</homepage>\n\t\t<url>https://redump.info/</url>\n\t</header>\n",
            n = self.games.len()
        );
        for game in &self.games {
            out.push_str(&format!(
                "\t<game name=\"{}\" id=\"{}\">\n\t\t<category>Games</category>\n",
                escape(&game.name),
                game.id
            ));
            if let Some(serial) = &game.serial {
                out.push_str(&format!("\t\t<serial>{}</serial>\n", escape(serial)));
            }
            out.push_str(&format!(
                "\t\t<version>1.00</version>\n\t\t<description>{}</description>\n",
                escape(&game.name)
            ));
            for rom in &game.roms {
                out.push_str(&format!("\t\t{rom}\n"));
            }
            out.push_str("\t</game>\n");
        }
        out.push_str("</datafile>\n");
        out
    }
}
