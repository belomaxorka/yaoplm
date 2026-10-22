//! `CD/games.bin` and `DVD/games.bin`: OPL's scan cache (IMP-8, LIB-4).
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.6. Mirrors
//! `base_game_info_t` (`include/supportbase.h`) and
//! `loadISOGameListCache()`/`queryISOGameListCache()`/`scanForISO()`
//! (`src/supportbase.c`) of ps2homebrew/Open-PS2-Loader@b3a21997.
//!
//! The file is a raw array of in-memory structs with no header. With the EE
//! GCC layout (u32 aligned to 4) a record is 188 bytes. For a new-style
//! image name OPL looks up `name + extension` here first and, on a hit, uses
//! the cached ID without mounting the image; it keeps that ID even if the
//! image was replaced under the same name. YAOPLM reads the cache only to
//! predict what OPL shows, and deletes it in folders it changes.

pub const FILE_NAME: &str = "games.bin";
/// `sizeof(base_game_info_t)` on the EE.
pub const RECORD_SIZE: usize = 188;

/// One cached game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedGame {
    pub name: String,
    pub startup: String,
    pub extension: String,
    pub parts: u8,
    pub media: u8,
    pub format: u8,
}

impl CachedGame {
    /// The image file name OPL matches against (`strcmp`, case-sensitive).
    pub fn file_name(&self) -> String {
        format!("{}{}", self.name, self.extension)
    }
}

fn c_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Records OPL loads: `size / 188`, a partial tail is ignored.
pub fn parse(bytes: &[u8]) -> Vec<CachedGame> {
    bytes
        .as_chunks::<RECORD_SIZE>()
        .0
        .iter()
        .map(|r| CachedGame {
            name: c_str(&r[..161]),
            startup: c_str(&r[161..174]),
            extension: c_str(&r[174..179]),
            parts: r[179],
            media: r[180],
            format: r[181],
        })
        .collect()
}

/// The cached ID OPL would use for `file_name`, if any.
pub fn lookup<'a>(cache: &'a [CachedGame], file_name: &str) -> Option<&'a CachedGame> {
    cache.iter().find(|game| game.file_name() == file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, startup: &str, ext: &str) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_SIZE];
        r[..name.len()].copy_from_slice(name.as_bytes());
        r[161..161 + startup.len()].copy_from_slice(startup.as_bytes());
        r[174..174 + ext.len()].copy_from_slice(ext.as_bytes());
        r[179] = 1;
        r[180] = 0x14;
        r[181] = 2;
        r
    }

    #[test]
    fn parse_and_lookup() {
        let mut bytes = record("Ico", "SCUS_971.13", ".iso");
        bytes.extend(record("Okami", "SLUS_215.15", ".zso"));
        bytes.extend([0u8; 10]);
        let cache = parse(&bytes);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache[1].startup, "SLUS_215.15");
        assert_eq!(
            lookup(&cache, "Ico.iso").map(|g| g.startup.as_str()),
            Some("SCUS_971.13")
        );
        assert_eq!(lookup(&cache, "ico.iso"), None);
        assert_eq!(lookup(&cache, "Ico.zso"), None);
    }
}
