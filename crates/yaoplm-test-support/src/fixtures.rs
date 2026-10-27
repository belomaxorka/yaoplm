//! Ready-made images for the `SYSTEM.CNF` cases OPL handles differently
//! (IMP-2; research/02 §4.3; TESTING.md "Фикстуры").

use crate::content::Content;
use crate::iso::{IsoBuilder, VirtualIso};

/// `SYSTEM.CNF` of a retail PS2 disc with this boot path.
pub fn ps2_cnf(boot2: &str) -> String {
    format!("BOOT2 = {boot2}\r\nVER = 1.00\r\nVMODE = NTSC\r\n")
}

/// Bytes standing in for a boot ELF in images that are never booted.
pub fn placeholder_elf() -> Vec<u8> {
    let mut elf = b"\x7FELF\x01\x01\x01".to_vec();
    elf.resize(512, 0);
    elf
}

/// A disc that boots `elf` as `cdrom0:\<startup>;1`. With a real homebrew
/// ELF this is the console test image; `min_bytes` pads it, e.g. past
/// 4 GiB for the `ul` test.
pub fn boot_iso(elf: Vec<u8>, startup: &str, min_bytes: u64) -> IsoBuilder {
    IsoBuilder::new()
        .volume_id("YAOPLM_TEST")
        .system_cnf(ps2_cnf(&format!("cdrom0:\\{startup};1")))
        .file(format!("{startup};1"), Content::Bytes(elf))
        .min_size(min_bytes)
}

/// The image variants every ID parser must agree with OPL on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Retail,
    LfOnly,
    NoBoot2,
    LowercaseKey,
    SubfolderElf,
    LongId,
    Ps1,
    /// `SYSTEM.CNF` stored without the `;1` version.
    NoVersionSuffix,
    LowercaseName,
    Dvd9,
}

impl Variant {
    pub const ALL: [Self; 10] = [
        Self::Retail,
        Self::LfOnly,
        Self::NoBoot2,
        Self::LowercaseKey,
        Self::SubfolderElf,
        Self::LongId,
        Self::Ps1,
        Self::NoVersionSuffix,
        Self::LowercaseName,
        Self::Dvd9,
    ];

    /// A file-name friendly label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::LfOnly => "lf-only",
            Self::NoBoot2 => "no-boot2",
            Self::LowercaseKey => "lowercase-key",
            Self::SubfolderElf => "subfolder-elf",
            Self::LongId => "long-id",
            Self::Ps1 => "ps1",
            Self::NoVersionSuffix => "no-version-suffix",
            Self::LowercaseName => "lowercase-name",
            Self::Dvd9 => "dvd9",
        }
    }

    /// The game ID OPL shows, or `None` when OPL hides the image.
    pub fn expected_startup(self) -> Option<&'static str> {
        match self {
            Self::Retail => Some("SLUS_200.62"),
            Self::LfOnly => Some("SCES_500.03"),
            Self::SubfolderElf => Some("DATA\\MAIN.E"),
            Self::LongId => Some("SLPM_123.45"),
            Self::Dvd9 => Some("SLUS_212.18"),
            Self::NoBoot2
            | Self::LowercaseKey
            | Self::Ps1
            | Self::NoVersionSuffix
            | Self::LowercaseName => None,
        }
    }

    /// Sector OPL reports as `layer1_start` for the DVD-9 variant.
    pub const DVD9_LAYER0_SECTORS: u64 = 64;

    pub fn builder(self) -> IsoBuilder {
        let elf = || Content::Bytes(placeholder_elf());
        // A CD: the `CD-XA001` mark decides its disc type, as it does for a
        // real dump, whatever the database says of its ID.
        let cd = || IsoBuilder::new().cd_xa();
        let retail = |id: &str| {
            cd().system_cnf(ps2_cnf(&format!("cdrom0:\\{id};1")))
                .file(format!("{id};1"), elf())
        };
        match self {
            Self::Retail => retail("SLUS_200.62"),
            Self::LfOnly => cd()
                .system_cnf("BOOT2 = cdrom0:\\SCES_500.03;1\nVER = 1.00\nVMODE = PAL\n")
                .file("SCES_500.03;1", elf()),
            Self::NoBoot2 => cd().system_cnf("VER = 1.00\r\nVMODE = PAL\r\n"),
            Self::LowercaseKey => cd()
                .system_cnf("boot2 = cdrom0:\\SLUS_200.62;1\r\n")
                .file("SLUS_200.62;1", elf()),
            Self::SubfolderElf => cd()
                .system_cnf(ps2_cnf("cdrom0:\\DATA\\MAIN.ELF;1"))
                .file("DATA/MAIN.ELF;1", elf()),
            Self::LongId => cd()
                .system_cnf(ps2_cnf("cdrom0:\\SLPM_123.4567;1"))
                .file("SLPM_123.4567;1", elf()),
            Self::Ps1 => cd()
                .system_cnf(
                    "BOOT = cdrom:\\SCUS_944.26;1\r\nTCB = 4\r\nEVENT = 10\r\nSTACK = 801FFFF0\r\n",
                )
                .file("SCUS_944.26;1", elf()),
            Self::NoVersionSuffix => cd()
                .file(
                    "SYSTEM.CNF",
                    Content::Bytes(ps2_cnf("cdrom0:\\SLUS_200.62;1").into_bytes()),
                )
                .file("SLUS_200.62;1", elf()),
            Self::LowercaseName => cd()
                .file(
                    "system.cnf;1",
                    Content::Bytes(ps2_cnf("cdrom0:\\SLUS_200.62;1").into_bytes()),
                )
                .file("SLUS_200.62;1", elf()),
            Self::Dvd9 => IsoBuilder::new()
                .system_cnf(ps2_cnf("cdrom0:\\SLUS_212.18;1"))
                .file("SLUS_212.18;1", elf())
                .dual_layer(Self::DVD9_LAYER0_SECTORS),
        }
    }

    pub fn build(self) -> VirtualIso {
        self.builder().build()
    }
}
