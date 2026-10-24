//! Which market a game and a picture belong to (ADR-0034, research/14 §4).
//!
//! Covers, back covers, spines and disc labels differ between releases:
//! rating badges, the PAL or NTSC bar, the language. A picture of another
//! market's release is the wrong picture, so for these types a source may
//! only answer with a picture keyed by the game's own ID or serial, or
//! tagged with its market. Screenshots, title screens, backgrounds and
//! logos are shared across markets.

use yaoplm_formats::art::ArtKind;

/// A group of markets whose packaging can stand in for each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// NTSC-U: the USA and Canada.
    America,
    /// PAL: Europe and Australia.
    Europe,
    /// NTSC-J: Japan, Korea, the rest of Asia.
    Asia,
}

/// Whether a picture type shows the release rather than the game.
pub fn region_specific(kind: ArtKind) -> bool {
    matches!(
        kind,
        ArtKind::Cov | ArtKind::Cov2 | ArtKind::Lab | ArtKind::Ico
    )
}

/// The family of a serial or OPL ID from its four-letter prefix (`SLUS`,
/// `SLES_533.32`); `None` for prefixes that say nothing about the market
/// (homebrew, prototypes).
pub fn of_id(id: &str) -> Option<Family> {
    let prefix = id.get(..4)?.to_ascii_uppercase();
    match prefix.as_str() {
        "SLUS" | "SCUS" => Some(Family::America),
        "SLES" | "SCES" | "SCED" => Some(Family::Europe),
        "SLPS" | "SLPM" | "SCPS" | "SCPM" | "SLAJ" | "SCAJ" | "SLKA" | "SCKA" | "SLCS" | "SCCS"
        | "PAPX" | "PCPX" | "SLPN" | "SCPN" => Some(Family::Asia),
        _ => None,
    }
}

/// The family of one region word of a Redump name (`USA`, `Europe`,
/// `Germany`, `Japan`...).
pub fn of_redump_region(word: &str) -> Option<Family> {
    match word.trim() {
        "USA" | "Canada" | "Brazil" | "Latin America" | "Mexico" => Some(Family::America),
        "Europe" | "Germany" | "France" | "Italy" | "Spain" | "Australia" | "Russia"
        | "Portugal" | "Netherlands" | "Poland" | "Sweden" | "Finland" | "Greece" | "Belgium"
        | "Norway" | "Denmark" | "Austria" | "Switzerland" | "Ireland" | "Croatia"
        | "South Africa" | "India" | "United Kingdom" | "UK" | "Scandinavia" | "New Zealand" => {
            Some(Family::Europe)
        }
        "Japan" | "Asia" | "Korea" | "China" | "Taiwan" | "Hong Kong" => Some(Family::Asia),
        _ => None,
    }
}

/// The families of a Redump name's region tag: the first parenthesised
/// group that names a region, `Grand Theft Auto III (USA, Canada)`, `Ico
/// (Europe) (En,Fr)`, `Bonus Demo 7 (6-7) (Europe)`.
pub fn of_redump_name(name: &str) -> Vec<Family> {
    let mut rest = name;
    while let Some(start) = rest.find('(') {
        let Some(len) = rest[start + 1..].find(')') else {
            break;
        };
        let mut families: Vec<Family> = rest[start + 1..start + 1 + len]
            .split(',')
            .filter_map(of_redump_region)
            .collect();
        families.dedup();
        if !families.is_empty() {
            return families;
        }
        rest = &rest[start + 1 + len..];
    }
    Vec::new()
}

/// Whether a picture found by Redump name may stand for a game with
/// serial `id`: always for types shared across markets, and for the
/// others only when the name is tagged with the game's market. A serial
/// that names no market lets every name through.
pub fn name_fits(kind: ArtKind, id: &str, redump_name: &str) -> bool {
    match of_id(id) {
        Some(family) if region_specific(kind) => of_redump_name(redump_name).contains(&family),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_of_ids_and_names() {
        assert_eq!(of_id("SLUS-20071"), Some(Family::America));
        assert_eq!(of_id("scus_971.13"), Some(Family::America));
        assert_eq!(of_id("SCES-50003"), Some(Family::Europe));
        assert_eq!(of_id("SLPS_250.02"), Some(Family::Asia));
        assert_eq!(of_id("SLKA-25003"), Some(Family::Asia));
        // Promotional discs were pressed for several markets.
        assert_eq!(of_id("PBPX-95503"), None);
        assert_eq!(of_id("DOOM_123.45"), None);
        assert_eq!(of_id("SL"), None);
        assert_eq!(
            of_redump_name("Grand Theft Auto III (USA, Canada) (v1.60)"),
            [Family::America]
        );
        assert_eq!(
            of_redump_name("Dead or Alive 2 (Europe) (En,Fr,De,Es,It)"),
            [Family::Europe]
        );
        assert_eq!(
            of_redump_name("Pac-Man World 2 (USA, Europe)"),
            [Family::America, Family::Europe]
        );
        assert_eq!(
            of_redump_name("Bonus Demo 7 (6-7) (Europe) (En,Fr,De,Es,It)"),
            [Family::Europe]
        );
        assert_eq!(
            of_redump_name("Linux (for PlayStation 2) Release 1.0 (Japan) (Disc 1)"),
            [Family::Asia]
        );
        assert_eq!(of_redump_name("Odd Name"), []);
        assert_eq!(of_redump_name("Odd (Name"), []);
        assert!(region_specific(ArtKind::Ico) && !region_specific(ArtKind::Bg));
    }

    #[test]
    fn names_of_other_markets_do_not_fit_packaging() {
        // libretro's "Dead or Alive 2 (Japan)" is the PAL scan, which only
        // the order of sources can catch; a name of another market is
        // caught here.
        assert!(name_fits(
            ArtKind::Cov,
            "SLPS-25002",
            "Dead or Alive 2 (Japan)"
        ));
        assert!(!name_fits(
            ArtKind::Cov,
            "SLPS-25002",
            "Dead or Alive 2 (Europe) (En,Fr,De,Es,It)"
        ));
        assert!(name_fits(
            ArtKind::Scr,
            "SLPS-25002",
            "Dead or Alive 2 (Europe) (En,Fr,De,Es,It)"
        ));
        assert!(!name_fits(ArtKind::Lab, "SLUS-20071", "Untagged"));
        assert!(name_fits(ArtKind::Cov, "HOME_000.01", "Anything (Japan)"));
    }
}
