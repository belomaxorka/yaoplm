//! Loader profiles: which OPL build the device is prepared for (SPEC §4).
//!
//! The rules that differ between builds live here so every format module
//! asks the same question. Sources: research/02 (§2 exFAT, §4.7
//! fragments, §6.3 `$Compatibility`, §7 art formats, §12 ZSO), research/05
//! §3.4 (RiptOPL) and research/06 §1.11 (fragments).

/// Size of the fragment table OPL hands its BDM `cdvdman`
/// (`modules/iopcore/common/cdvd_config.h`, ps2homebrew/Open-PS2-Loader@
/// b3a21997; 64 since `8f04d009`, June 2022). Neutrino and RiptOPL's
/// Neutrino launcher use a table of the same size.
pub const BDM_MAX_FRAGS: u32 = 64;

/// The fragment rule of a build for games on BDM (USB, MX4SIO, iLink,
/// BDM HDD). `bdmLaunchGame()` in `src/bdmsupport.c` asks the driver for
/// each file's runs of clusters (`USBMASS_IOCTL_GET_FRAGLIST`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FragmentLimit {
    /// 1.2.0 betas: all parts share one table of this many runs. Upstream
    /// checks the limit per part only, so a game over it in total is not
    /// refused cleanly; YAOPLM holds the total to it (IMP-7b).
    Total(u32),
    /// 1.1.0: every file in one run (`USBMASS_IOCTL_CHECK_CHAIN` per part,
    /// `src/bdmsupport.c` at tag v1.1.0), or "the game is fragmented".
    OnePerFile,
}

impl FragmentLimit {
    /// Whether OPL starts a game whose files lie in `runs` pieces each.
    pub fn allows(self, runs: &[u32]) -> bool {
        match self {
            Self::Total(limit) => {
                runs.iter().map(|&n| u64::from(n)).sum::<u64>() <= u64::from(limit)
            }
            Self::OnePerFile => runs.iter().all(|&n| n <= 1),
        }
    }
}

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

    /// How many runs of clusters a game may occupy on a BDM device.
    pub fn fragment_limit(self) -> FragmentLimit {
        match self {
            Self::Opl110 => FragmentLimit::OnePerFile,
            Self::Opl120Beta | Self::RiptOpl => FragmentLimit::Total(BDM_MAX_FRAGS),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_limits() {
        let beta = LoaderProfile::Opl120Beta.fragment_limit();
        assert_eq!(beta, FragmentLimit::Total(64));
        assert!(beta.allows(&[]));
        assert!(beta.allows(&[64]));
        assert!(beta.allows(&[40, 24]));
        // Each part fits the table on its own; together they do not.
        assert!(!beta.allows(&[40, 25]));
        assert_eq!(LoaderProfile::RiptOpl.fragment_limit(), beta);

        let stable = LoaderProfile::Opl110.fragment_limit();
        assert!(stable.allows(&[1, 1, 1, 1, 1]));
        assert!(!stable.allows(&[1, 2]));
    }
}
