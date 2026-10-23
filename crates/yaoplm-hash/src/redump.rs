//! Matching an image against Redump (GAME-5, META-2).
//!
//! A SHA-1 match of the whole image is the only proof that a dump is good;
//! the game ID only says which Redump entries are worth comparing with.
//! Three outcomes, as SPEC GAME-5 names them:
//! - **verified**: size and SHA-1 equal a Redump rom;
//! - **bad dump**: Redump lists the game ID, but none of its dumps has these
//!   hashes (a damaged, trimmed or patched image, or a revision Redump
//!   does not have; the candidates and their sizes say which is likelier);
//! - **unknown**: nothing to compare with. The ID is not in the DAT
//!   (homebrew, prototypes, an unlisted disc), or Redump lists the game as
//!   BIN/CUE and the image is an ISO: a CD ISO keeps 2048 of the 2352 bytes
//!   of every sector, so it can never hash like the BIN.
//!
//! Redump spells serials `SLPM 65002`, `SLUS-20062GH`, `SCES-53880/ANZ`,
//! `SLUS21568` or several at once (`SLES-50539, SLES-50539-P`); OPL's ID is
//! `SLUS_200.62`. [`serial_key`] maps all of them to `SLUS-20062`.

use std::collections::HashMap;

use crate::dat::{Datafile, Game, Header, Rom};
use crate::digest::Hashes;

/// A comparable form of a serial or an OPL ID: letters, a hyphen, the first
/// five digits (fewer if that is all there is). `None` when the text does
/// not start with letters followed by digits.
pub fn serial_key(text: &str) -> Option<String> {
    let text = text.trim().to_ascii_uppercase();
    let mut chars = text.chars().peekable();
    let mut letters = String::new();
    while let Some(c) = chars.peek().copied().filter(char::is_ascii_alphabetic) {
        letters.push(c);
        chars.next();
    }
    while chars.peek().is_some_and(|c| matches!(c, ' ' | '-' | '_')) {
        chars.next();
    }
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if c != '.' {
            break;
        }
        chars.next();
        if digits.len() == 5 {
            break;
        }
    }
    (!letters.is_empty() && letters.len() <= 5 && !digits.is_empty())
        .then(|| format!("{letters}-{digits}"))
}

/// Every serial in a Redump `<serial>` field.
pub fn serial_keys(field: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for part in field.split([',', ';', '&']) {
        if let Some(key) = serial_key(part)
            && !keys.contains(&key)
        {
            keys.push(key);
        }
    }
    keys
}

/// What the hashed bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Data {
    /// 2048-byte sectors: an ISO, a decompressed ZSO, joined `ul` chunks.
    Iso,
    /// A raw 2352-byte track: a BIN.
    Raw,
}

/// One Redump entry, for the verdict and the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    /// `https://redump.info/disc/<id>`.
    pub id: Option<String>,
    pub serial: Option<String>,
    pub version: Option<String>,
    /// The rom the image would have to be: the `.iso`, or the first `.bin`.
    pub rom: String,
    pub size: u64,
    /// The rom has exactly the size of the image: same layout, other bytes.
    pub same_size: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unknown {
    /// The DAT has no entry with this ID (or the image has no ID).
    NotListed,
    /// Redump lists the game as BIN/CUE; an ISO cannot match its hashes.
    CdImage(Vec<Candidate>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Size and SHA-1 equal this Redump rom.
    Verified {
        game: Candidate,
        /// The game ID is one of the entry's serials. False when the same
        /// image is listed under another ID (OPL reads one from `SYSTEM.CNF`,
        /// Redump prints the one on the case).
        serial_matches: bool,
    },
    BadDump(Vec<Candidate>),
    Unknown(Unknown),
}

/// A datafile prepared for lookups.
#[derive(Debug, Clone, Default)]
pub struct RedumpIndex {
    header: Header,
    games: Vec<Game>,
    /// Serial keys per game, parallel to `games`.
    keys: Vec<Vec<String>>,
    by_sha1: HashMap<[u8; 20], Vec<(usize, usize)>>,
    by_key: HashMap<String, Vec<usize>>,
}

fn is_iso(rom: &Rom) -> bool {
    rom.name.to_ascii_lowercase().ends_with(".iso")
}

fn is_bin(rom: &Rom) -> bool {
    rom.name.to_ascii_lowercase().ends_with(".bin")
}

impl RedumpIndex {
    pub fn new(datafile: Datafile) -> Self {
        let mut index = Self {
            header: datafile.header,
            ..Self::default()
        };
        for (g, game) in datafile.games.into_iter().enumerate() {
            for (r, rom) in game.roms.iter().enumerate() {
                if let Some(sha1) = rom.sha1 {
                    index.by_sha1.entry(sha1).or_default().push((g, r));
                }
            }
            let keys = game.serial.as_deref().map(serial_keys).unwrap_or_default();
            for key in &keys {
                index.by_key.entry(key.clone()).or_default().push(g);
            }
            index.keys.push(keys);
            index.games.push(game);
        }
        index
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// Whether the DAT carries serials at all (the `serial,version` variant).
    pub fn has_serials(&self) -> bool {
        !self.by_key.is_empty()
    }

    /// Entries listing this ID, in DAT order.
    pub fn games_for(&self, id: &str) -> Vec<&Game> {
        serial_key(id)
            .and_then(|key| self.by_key.get(&key))
            .map(|games| games.iter().map(|&g| &self.games[g]).collect())
            .unwrap_or_default()
    }

    fn candidate(&self, game: usize, rom: &Rom, size: u64) -> Candidate {
        let g = &self.games[game];
        Candidate {
            name: g.name.clone(),
            id: g.id.clone(),
            serial: g.serial.clone(),
            version: g.version.clone(),
            rom: rom.name.clone(),
            size: rom.size,
            same_size: rom.size == size,
        }
    }

    /// The verdict for an image with `hashes`, whose game ID is `id` when
    /// it has one.
    pub fn verify(&self, hashes: &Hashes, id: Option<&str>, data: Data) -> Verdict {
        let key = id.and_then(serial_key);
        let hits = self.by_sha1.get(&hashes.sha1).into_iter().flatten();
        let mut best: Option<(usize, usize, bool)> = None;
        for &(g, r) in hits {
            if self.games[g].roms[r].size != hashes.size {
                continue;
            }
            let serial_matches = key.as_ref().is_some_and(|k| self.keys[g].contains(k));
            if best.is_none_or(|(_, _, matched)| serial_matches && !matched) {
                best = Some((g, r, serial_matches));
            }
        }
        if let Some((g, r, serial_matches)) = best {
            return Verdict::Verified {
                game: self.candidate(g, &self.games[g].roms[r], hashes.size),
                serial_matches,
            };
        }

        let games = key
            .as_ref()
            .and_then(|k| self.by_key.get(k))
            .cloned()
            .unwrap_or_default();
        if games.is_empty() {
            return Verdict::Unknown(Unknown::NotListed);
        }
        let mut cds = Vec::new();
        let mut others = Vec::new();
        for g in games {
            let roms = &self.games[g].roms;
            match roms.iter().find(|r| is_iso(r)) {
                Some(iso) => others.push(self.candidate(g, iso, hashes.size)),
                None => {
                    if let Some(bin) = roms.iter().find(|r| is_bin(r)) {
                        let mut candidate = self.candidate(g, bin, hashes.size);
                        if data == Data::Iso {
                            // Same number of sectors as the BIN's track.
                            candidate.same_size =
                                bin.size % 2352 == 0 && bin.size / 2352 * 2048 == hashes.size;
                            cds.push(candidate);
                        } else {
                            others.push(candidate);
                        }
                    }
                }
            }
        }
        if others.is_empty() && !cds.is_empty() {
            return Verdict::Unknown(Unknown::CdImage(cds));
        }
        others.extend(cds);
        Verdict::BadDump(others)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::parse;

    fn hashes(size: u64, sha1: u8) -> Hashes {
        Hashes {
            size,
            crc32: 0,
            sha1: [sha1; 20],
            md5: None,
        }
    }

    fn sha(byte: u8) -> String {
        format!("{byte:02x}").repeat(20)
    }

    fn index() -> RedumpIndex {
        let xml = format!(
            r#"<datafile>
<game name="Grand Theft Auto III (USA)" id="8814"><serial>SLUS-20062, SLUS-20062GH</serial>
  <rom name="Grand Theft Auto III (USA).iso" size="4096" crc="00000000" sha1="{a}"/></game>
<game name="Grand Theft Auto III (USA) (v2.00)" id="9000"><serial>SLUS 20062</serial>
  <rom name="Grand Theft Auto III (USA) (v2.00).iso" size="6144" crc="00000000" sha1="{b}"/></game>
<game name="Ico (Japan)" id="1"><serial>SCPS 11003</serial>
  <rom name="Ico (Japan).iso" size="2048" crc="00000000" sha1="{c}"/></game>
<game name="Disc (Japan) (Taikenban)" id="4504"><serial>SLPM 60101</serial>
  <rom name="Disc (Japan) (Taikenban).cue" size="93" crc="00000000" sha1="{d}"/>
  <rom name="Disc (Japan) (Taikenban).bin" size="4704" crc="00000000" sha1="{e}"/></game>
<game name="Shared image (Europe)" id="77"><serial>SLES 50000</serial>
  <rom name="Shared image (Europe).iso" size="2048" crc="00000000" sha1="{c}"/></game>
</datafile>"#,
            a = sha(0xaa),
            b = sha(0xbb),
            c = sha(0xcc),
            d = sha(0xdd),
            e = sha(0xee),
        );
        RedumpIndex::new(parse(xml.as_bytes()).unwrap())
    }

    #[test]
    fn serial_spellings() {
        for (text, key) in [
            ("SLUS_200.62", "SLUS-20062"),
            ("SLUS-20062", "SLUS-20062"),
            ("SLPM 65002", "SLPM-65002"),
            ("slus 21240gh", "SLUS-21240"),
            ("SCES-53880/ANZ", "SCES-53880"),
            ("SLUS21568", "SLUS-21568"),
            ("SLUS-21910-F", "SLUS-21910"),
            ("KRT 005", "KRT-005"),
        ] {
            assert_eq!(serial_key(text).as_deref(), Some(key), "{text}");
        }
        assert_eq!(serial_key("DATA\\MAIN.E"), None);
        assert_eq!(serial_key("12345"), None);
        assert_eq!(
            serial_keys("SLUS 21596, SLUS 21596P3F, SLUS 27013F"),
            ["SLUS-21596", "SLUS-27013"]
        );
    }

    #[test]
    fn verified_by_sha1_and_size() {
        let index = index();
        assert_eq!(index.len(), 5);
        assert!(index.has_serials());
        let Verdict::Verified {
            game,
            serial_matches,
        } = index.verify(&hashes(4096, 0xaa), Some("SLUS_200.62"), Data::Iso)
        else {
            panic!("expected a match");
        };
        assert_eq!(game.name, "Grand Theft Auto III (USA)");
        assert_eq!(game.id.as_deref(), Some("8814"));
        assert!(serial_matches);

        // The same bytes with another size are not a match.
        assert!(matches!(
            index.verify(&hashes(4097, 0xaa), Some("SLUS_200.62"), Data::Iso),
            Verdict::BadDump(_)
        ));

        // Two entries share an image: the one with the game's ID wins.
        let Verdict::Verified {
            game,
            serial_matches,
        } = index.verify(&hashes(2048, 0xcc), Some("SLES_500.00"), Data::Iso)
        else {
            panic!("expected a match");
        };
        assert_eq!(game.name, "Shared image (Europe)");
        assert!(serial_matches);
        let Verdict::Verified { serial_matches, .. } =
            index.verify(&hashes(2048, 0xcc), Some("SLUS_999.99"), Data::Iso)
        else {
            panic!("expected a match");
        };
        assert!(!serial_matches);
    }

    #[test]
    fn bad_dumps_list_candidates() {
        let index = index();
        let Verdict::BadDump(candidates) =
            index.verify(&hashes(4096, 0x11), Some("SLUS_200.62"), Data::Iso)
        else {
            panic!("expected a bad dump");
        };
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].same_size);
        assert!(!candidates[1].same_size);
    }

    #[test]
    fn unknown_ids_and_cd_isos() {
        let index = index();
        assert_eq!(
            index.verify(&hashes(4096, 0x11), Some("SLUS_999.99"), Data::Iso),
            Verdict::Unknown(Unknown::NotListed)
        );
        assert_eq!(
            index.verify(&hashes(4096, 0x11), None, Data::Iso),
            Verdict::Unknown(Unknown::NotListed)
        );
        // Redump has this CD as BIN/CUE: an ISO of it cannot be checked.
        let Verdict::Unknown(Unknown::CdImage(cds)) =
            index.verify(&hashes(4096, 0x11), Some("SLPM_601.01"), Data::Iso)
        else {
            panic!("expected a CD");
        };
        assert_eq!(cds[0].rom, "Disc (Japan) (Taikenban).bin");
        assert!(cds[0].same_size, "4704 / 2352 * 2048 = 4096");
        // A BIN is compared directly.
        assert!(matches!(
            index.verify(&hashes(4704, 0xee), Some("SLPM_601.01"), Data::Raw),
            Verdict::Verified { .. }
        ));
        assert!(matches!(
            index.verify(&hashes(4704, 0x12), Some("SLPM_601.01"), Data::Raw),
            Verdict::BadDump(_)
        ));
        assert_eq!(index.games_for("SLUS-20062").len(), 2);
    }
}
