//! Loader profiles: which OPL build the device is prepared for (SPEC §4).
//!
//! The rules that differ between builds live here so every format module
//! asks the same question. Sources: research/02 (§2 exFAT, §6.3
//! `$Compatibility`, §7 art formats, §12 ZSO) and research/05 §3.4 (RiptOPL).

/// The loader a library targets. Default: OPL 1.2.0 beta (ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LoaderProfile {
    #[default]
    Opl120Beta,
    /// The last stable release (2021).
    Opl110,
    /// NathanNeurotic's fork.
    RiptOpl,
}

impl LoaderProfile {
    pub const ALL: [Self; 3] = [Self::Opl120Beta, Self::Opl110, Self::RiptOpl];

    /// ZSO support arrived in the 1.2.0 betas (OPL commit `c34a072`).
    pub fn reads_zso(self) -> bool {
        !matches!(self, Self::Opl110)
    }

    /// exFAT on BDM devices: 1.2.0 beta rev1880 and later.
    pub fn reads_exfat(self) -> bool {
        !matches!(self, Self::Opl110)
    }

    /// Art file extensions in lookup order. 1.2.0 beta dropped JPG and BMP
    /// (OPL commit `c5a12a0`); YAOPLM always writes PNG (ADR-0005).
    pub fn art_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Opl110 => &["png", "jpg", "bmp"],
            Self::Opl120Beta | Self::RiptOpl => &["png"],
        }
    }

    /// Valid `$Compatibility` bits: modes 1-6 upstream, 7-8 in RiptOPL.
    pub fn compat_mask(self) -> u32 {
        match self {
            Self::RiptOpl => 0xFF,
            Self::Opl120Beta | Self::Opl110 => 0x3F,
        }
    }
}
