//! Virtual memory cards `VMC/<name>.bin`: names and the validity check OPL
//! runs before using one. Creating cards is VMC-1 (v0.2).
//!
//! Format reference: `docs/research/02-opl-formats.md` §9. Mirrors
//! `vmc_superblock_t` (`include/mcemu.h`), `sysCheckVMC()` (`src/system.c`)
//! and the 32-byte name buffers in `src/guigame.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997.
//!
//! OPL accepts a card when the superblock magic starts with
//! `Sony PS2 Memory Card Format`, the card type is 2, the file size equals
//! `pages_per_cluster * clusters_per_card * page_size`, and that size is a
//! whole number of MiB. A zero-filled file (what some managers write) fails
//! the check. On BDM the file must also be one contiguous cluster chain.

pub const VMC_FOLDER: &str = "VMC";
/// Name buffers are 32 bytes with a terminator.
pub const NAME_MAX: usize = 31;
/// Sizes OPL's VMC menu creates.
pub const SIZES_MB: [u32; 4] = [8, 16, 32, 64];
pub const MAGIC: &[u8] = b"Sony PS2 Memory Card Format";
/// Bytes of the superblock OPL reads.
pub const SUPERBLOCK_SIZE: usize = 0x200;
const MIB: u64 = 1 << 20;

/// Superblock fields OPL looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superblock {
    /// `magic[40]`: the magic text followed by the version, e.g. `1.2.0.0`.
    pub magic: [u8; 40],
    pub page_size: u16,
    pub pages_per_cluster: u16,
    pub pages_per_block: u16,
    pub clusters_per_card: u32,
    pub card_type: u8,
    pub card_flags: u8,
}

impl Superblock {
    /// Reads the fields from the start of the file. Missing bytes count as 0,
    /// as OPL zeroes the struct before reading.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut raw = [0u8; SUPERBLOCK_SIZE];
        let n = bytes.len().min(SUPERBLOCK_SIZE);
        raw[..n].copy_from_slice(&bytes[..n]);
        let u16_at = |i: usize| u16::from_le_bytes([raw[i], raw[i + 1]]);
        let mut magic = [0u8; 40];
        magic.copy_from_slice(&raw[..40]);
        Self {
            magic,
            page_size: u16_at(0x28),
            pages_per_cluster: u16_at(0x2A),
            pages_per_block: u16_at(0x2C),
            clusters_per_card: u32::from_le_bytes([raw[0x30], raw[0x31], raw[0x32], raw[0x33]]),
            card_type: raw[0x150],
            card_flags: raw[0x151],
        }
    }

    /// Card size implied by the superblock.
    pub fn card_bytes(&self) -> u64 {
        u64::from(self.pages_per_cluster)
            * u64::from(self.clusters_per_card)
            * u64::from(self.page_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VmcProblem {
    #[error("not a PS2 memory card image (no superblock magic)")]
    BadMagic,
    #[error("card type {0} is not a PS2 card (2)")]
    CardType(u8),
    #[error("file is {file} bytes but the superblock describes {card}")]
    SizeMismatch { file: u64, card: u64 },
    #[error("size {0} is not a whole number of MiB")]
    NotWholeMib(u64),
}

/// `sysCheckVMC()`: the card size in MiB, or why OPL refuses the file.
pub fn opl_check(superblock: &[u8], file_size: u64) -> Result<u32, VmcProblem> {
    let sb = Superblock::parse(superblock);
    if !sb.magic.starts_with(MAGIC) {
        return Err(VmcProblem::BadMagic);
    }
    if sb.card_type != 2 {
        return Err(VmcProblem::CardType(sb.card_type));
    }
    if sb.card_bytes() != file_size {
        return Err(VmcProblem::SizeMismatch {
            file: file_size,
            card: sb.card_bytes(),
        });
    }
    if !file_size.is_multiple_of(MIB) {
        return Err(VmcProblem::NotWholeMib(file_size));
    }
    u32::try_from(file_size / MIB).map_err(|_| VmcProblem::NotWholeMib(file_size))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VmcNameError {
    #[error("VMC name is empty")]
    Empty,
    #[error("VMC name {0:?} is longer than {NAME_MAX} bytes")]
    TooLong(String),
    #[error("VMC name {0:?} contains characters a file name cannot hold")]
    BadChars(String),
}

/// Checks a name for `$VMC_0`/`$VMC_1`; the file is `VMC/<name>.bin`.
pub fn validate_name(name: &str) -> Result<(), VmcNameError> {
    if name.is_empty() {
        return Err(VmcNameError::Empty);
    }
    if name.len() > NAME_MAX {
        return Err(VmcNameError::TooLong(name.to_owned()));
    }
    let bad = name.chars().any(|c| {
        !c.is_ascii_graphic() && c != ' '
            || matches!(c, '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|')
    });
    if bad {
        return Err(VmcNameError::BadChars(name.to_owned()));
    }
    Ok(())
}

pub fn file_name(name: &str) -> String {
    format!("{name}.bin")
}

/// The name OPL's menu suggests: `<ID or group>_<slot>`, slot 0 or 1.
pub fn suggested_name(id: &str, slot: u8) -> String {
    format!("{id}_{slot}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn superblock(size_mb: u32) -> Vec<u8> {
        let mut sb = vec![0xFFu8; SUPERBLOCK_SIZE];
        sb[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
        sb[28..40].copy_from_slice(b"1.2.0.0\0\0\0\0\0");
        sb[0x28..0x2A].copy_from_slice(&512u16.to_le_bytes());
        sb[0x2A..0x2C].copy_from_slice(&2u16.to_le_bytes());
        sb[0x2C..0x2E].copy_from_slice(&16u16.to_le_bytes());
        sb[0x30..0x34].copy_from_slice(&(size_mb * 1024).to_le_bytes());
        sb[0x150] = 2;
        sb[0x151] = 0x2B;
        sb
    }

    #[test]
    fn accepts_genvmc_cards() {
        for mb in SIZES_MB {
            assert_eq!(opl_check(&superblock(mb), u64::from(mb) * MIB), Ok(mb));
        }
    }

    #[test]
    fn rejects_what_opl_rejects() {
        // A zero-filled "card", as some managers create.
        assert_eq!(
            opl_check(&[0u8; SUPERBLOCK_SIZE], 8 * MIB),
            Err(VmcProblem::BadMagic)
        );
        let mut sb = superblock(8);
        sb[0x150] = 1;
        assert_eq!(opl_check(&sb, 8 * MIB), Err(VmcProblem::CardType(1)));
        assert_eq!(
            opl_check(&superblock(8), 16 * MIB),
            Err(VmcProblem::SizeMismatch {
                file: 16 * MIB,
                card: 8 * MIB
            })
        );
        // A truncated file reads as zeros past its end.
        assert_eq!(
            opl_check(&superblock(8)[..0x100], 8 * MIB),
            Err(VmcProblem::CardType(0))
        );
    }

    #[test]
    fn names() {
        assert_eq!(suggested_name("SLUS_200.62", 0), "SLUS_200.62_0");
        assert_eq!(file_name("generic_1"), "generic_1.bin");
        assert!(validate_name("SLUS_200.62_0").is_ok());
        assert!(validate_name(&"a".repeat(31)).is_ok());
        assert!(matches!(
            validate_name(&"a".repeat(32)),
            Err(VmcNameError::TooLong(_))
        ));
        assert!(matches!(
            validate_name("a/b"),
            Err(VmcNameError::BadChars(_))
        ));
        assert!(matches!(
            validate_name("карта"),
            Err(VmcNameError::BadChars(_))
        ));
        assert_eq!(validate_name(""), Err(VmcNameError::Empty));
    }
}
