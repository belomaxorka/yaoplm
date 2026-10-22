//! `USBA_crc32`: the title hash in `ul` chunk file names (`ul.%08X.%s.%02x`).
//!
//! Format reference: `docs/research/02-opl-formats.md` §5.3. Mirrors
//! `USBA_crc32()` in `src/system.c` of ps2homebrew/Open-PS2-Loader@b3a21997
//! (reimplemented, not copied: ADR-0002).
//!
//! It is not the standard CRC-32:
//! - the table uses polynomial 0x04C11DB7 with an inverted top-bit test and is
//!   stored in reverse order;
//! - the running value is never initialised: it starts with whatever the last
//!   table iteration left behind (`tab[0]`, which happens to be 0);
//! - the terminating NUL is hashed too, and at most 33 bytes are processed;
//! - there is no final XOR.
//!
//! Upstream reads the title through a signed `char`, so a byte >= 0x80 turns
//! into a negative table index (undefined behaviour, the result depends on
//! memory before the table). RiptOPL fixed that by reading bytes as unsigned
//! (NathanNeurotic/Open-PS2-Loader@40c52ceb `src/system.c`). [`usba_crc32`]
//! computes the RiptOPL value; [`is_portable`] tells whether upstream OPL
//! agrees with it. `ul` titles must stay 7-bit ASCII.

const POLY: u32 = 0x04C1_1DB7;

/// The hash loop stops after this many bytes even without a NUL
/// (`count <= 32` in OPL).
pub const MAX_HASHED_BYTES: usize = 33;

struct Table {
    entries: [u32; 256],
    /// Value of the accumulator after building the table; OPL hashes from it.
    seed: u32,
}

const fn build_table() -> Table {
    let mut entries = [0u32; 256];
    let mut crc = 0u32;
    let mut t = 0;
    while t < 256 {
        crc = (t as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            // OPL tests `crc < 0` on a signed int: XOR when the top bit is clear.
            crc = if crc & 0x8000_0000 != 0 {
                crc << 1
            } else {
                (crc << 1) ^ POLY
            };
            bit += 1;
        }
        entries[255 - t] = crc;
        t += 1;
    }
    Table { entries, seed: crc }
}

static TABLE: Table = build_table();

/// Hashes `name` the way OPL hashes a `ul.cfg` title.
///
/// `name` is treated as a C string: hashing stops at the first NUL in the
/// slice, or at the implicit terminator after its end, which is hashed too.
pub fn usba_crc32(name: &[u8]) -> u32 {
    let mut crc = TABLE.seed;
    for i in 0..MAX_HASHED_BYTES {
        let byte = name.get(i).copied().unwrap_or(0);
        let index = usize::from(byte ^ (crc >> 24) as u8);
        crc = TABLE.entries[index] ^ (crc << 8);
        if byte == 0 {
            break;
        }
    }
    crc
}

/// True when upstream OPL computes the same hash as [`usba_crc32`]: every
/// hashed byte is 7-bit ASCII.
pub fn is_portable(name: &[u8]) -> bool {
    name.iter()
        .take(MAX_HASHED_BYTES)
        .take_while(|&&b| b != 0)
        .all(u8::is_ascii)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_hand_computed_entries() {
        // t = 255: 0xFF000000 keeps its top bit set for all 8 rounds, so it
        // is only shifted out. The "uninitialised" seed is therefore 0.
        assert_eq!(TABLE.entries[0], TABLE.seed);
        assert_eq!(TABLE.seed, 0);
        // t = 0 lands in the last slot: 0 -> POLY -> (POLY << 1) ^ POLY ...
        let mut crc = 0u32;
        for _ in 0..8 {
            crc = if crc >> 31 == 1 {
                crc << 1
            } else {
                (crc << 1) ^ POLY
            };
        }
        assert_eq!(TABLE.entries[255], crc);
    }

    /// Checked against OPL's C function by `tests/opl_oracles.rs`.
    #[test]
    fn known_vectors() {
        let z = |n| "Z".repeat(n);
        for (name, crc) in [
            ("", 0x0000_0000),
            ("A", 0x935F_AD90),
            ("Grand Theft Auto III", 0x10D0_0014),
            ("Metal Gear Solid 2 Substance", 0xCE32_D6E8),
            (&z(32), 0xC5B9_1150),
            (&z(33), 0x92A4_6C81),
            (&z(60), 0x92A4_6C81),
            ("AG", 0x017A_9F71),
            ("Test Game", 0x1C9B_6AE0),
            // From ulmake's test suite, via JO0812/TinyPS2Manager@b1b10794
            // internal/usbextreme/split_test.go and gecm0/POUM core/crc.rs.
            ("f", 0x8433_E5CC),
            ("F", 0xA490_D3EA),
            (&format!("f{}", "o".repeat(31)), 0x84BA_9D95),
            (&format!("F{}", "O".repeat(31)), 0x8CAF_0142),
        ] {
            assert_eq!(usba_crc32(name.as_bytes()), crc, "{name:?}");
        }
    }

    #[test]
    fn stops_at_first_nul() {
        assert_eq!(usba_crc32(b"ABC"), usba_crc32(b"ABC\0junk"));
        assert_ne!(usba_crc32(b"ABC"), usba_crc32(b"ABD"));
    }

    #[test]
    fn hashes_at_most_33_bytes() {
        let long = [b'A'; 40];
        assert_eq!(usba_crc32(&long[..33]), usba_crc32(&long));
        assert_ne!(usba_crc32(&long[..32]), usba_crc32(&long[..33]));
    }

    #[test]
    fn portability() {
        assert!(is_portable(b"Grand Theft Auto III"));
        assert!(!is_portable("Пример".as_bytes()));
        // Bytes past the NUL or the 33-byte window are never hashed.
        assert!(is_portable(b"OK\0\xff"));
        let mut tail = [b'A'; 34];
        tail[33] = 0xFF;
        assert!(is_portable(&tail));
    }
}
