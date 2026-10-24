//! A game's settings in `CFG/<ID>.cfg`, as OPL's game menu edits them
//! (CFG-2).
//!
//! Format reference: `docs/research/02-opl-formats.md` §6.3 and
//! `docs/research/15-cfg-vmc-prior-art.md` §1. Mirrors `guiGameSaveConfig()`,
//! `guiGameSetGSMSettingsState()` and the other `...SettingsState()`
//! handlers and `guiGameAltStartupNameHandler()` (`src/guigame.c`),
//! `configSetVMC()` (`src/config.c`), and what a launch reads:
//! `InitGSMConfig()` (`src/gsm.c`), `InitCheatsConfig()` (`src/cheatman.c`)
//! and the PADEMU part of `sbPrepare()` (`src/supportbase.c`), all in
//! ps2homebrew/Open-PS2-Loader@b3a21997. Rewritten from the logic, not
//! copied (ADR-0002).
//!
//! The rules:
//! - a setting at its default is removed rather than written: compatibility
//!   0, an empty alternative startup or VMC slot, a per-game GSM, cheat or
//!   PADEMU value of 0;
//! - GSM, cheats and PADEMU are per game while their `...Source` key
//!   exists, whatever its value, and global (`conf_game.cfg`) otherwise.
//!   Per game writes the key as 1; going back to global removes only that
//!   key, so the per-game values stay for the next switch, as in OPL;
//! - a save that sets anything but the VMC slots also sets
//!   `$ConfigSource=1`, which keeps OPL's compatibility-list update away
//!   from settings the user made;
//! - `$DMA` (HDD only), `$DNAS`, `$ParentalLock`, the pad macro, the OSD
//!   language and every key YAOPLM does not know stay as they are.
//!
//! [`GameSettings::apply`] touches only the settings that differ from what
//! the file says now, so an edit never rewrites the keys around it.

use crate::LoaderProfile;
use crate::cfg::{CfgDocument, CfgError};
use crate::vmc::{self, VmcNameError};

pub const COMPATIBILITY: &str = "$Compatibility";
pub const DMA: &str = "$DMA";
pub const ALT_STARTUP: &str = "$AltStartup";
pub const VMC_SLOTS: [&str; 2] = ["$VMC_0", "$VMC_1"];
pub const CONFIG_SOURCE: &str = "$ConfigSource";
pub const GSM_SOURCE: &str = "$GSMSource";
pub const ENABLE_GSM: &str = "$EnableGSM";
pub const GSM_VMODE: &str = "$GSMVMode";
pub const GSM_X_OFFSET: &str = "$GSMXOffset";
pub const GSM_Y_OFFSET: &str = "$GSMYOffset";
pub const GSM_FIELD_FIX: &str = "$GSMFIELDFix";
pub const CHEATS_SOURCE: &str = "$CheatsSource";
pub const ENABLE_CHEAT: &str = "$EnableCheat";
pub const CHEAT_MODE: &str = "$CheatMode";
pub const PADEMU_SOURCE: &str = "$PADEMUSource";
pub const ENABLE_PADEMU: &str = "$EnablePadEmu";
pub const PADEMU_SETTINGS: &str = "$PadEmuSettings";

/// OPL boots `cdrom0:\<value>;1` from a 32-byte buffer (`ElfPath` in
/// `src/system.c`): 21 characters fit, although the menu takes 31.
pub const ALT_STARTUP_MAX: usize = 21;
/// The last GSM video mode (`predef_vmode[29]` in `src/gsm.c`).
pub const GSM_VMODE_MAX: i32 = 28;
/// `$CheatMode`: 0 applies every code, 1 asks at launch.
pub const CHEAT_MODE_MAX: i32 = 1;
/// `CONFIG_SOURCE_USER`.
const SOURCE_USER: &str = "1";

/// PADEMU bits (`guiGamePadEmuUpdater()`): bit 0 Bluetooth instead of USB;
/// bits 8+ emulation per port; bits 16+ vibration per port; bit 24
/// multitap, 25 its port, 26 the Bluetooth workaround.
pub mod pademu {
    pub const BLUETOOTH: u32 = 1;
    pub const fn port(port: u32) -> u32 {
        1 << (8 + port)
    }
    pub const fn vibration(port: u32) -> u32 {
        1 << (16 + port)
    }
    pub const MULTITAP: u32 = 1 << 24;
    pub const MULTITAP_PORT: u32 = 1 << 25;
    pub const WORKAROUND: u32 = 1 << 26;
}

/// GSM: the video mode OPL forces on the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gsm {
    /// `$GSMSource` exists: these values, not `conf_game.cfg`'s.
    pub per_game: bool,
    pub enabled: bool,
    /// 0..=28; see research/02 §6.3.
    pub vmode: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub field_fix: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cheats {
    /// `$CheatsSource` exists.
    pub per_game: bool,
    pub enabled: bool,
    /// 0: every code; 1: pick at launch.
    pub mode: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PadEmu {
    /// `$PADEMUSource` exists.
    pub per_game: bool,
    pub enabled: bool,
    /// [`pademu`] bits.
    pub settings: u32,
}

/// What OPL's game menu edits, as a launch would read it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameSettings {
    /// `$Compatibility` bits; mode N is bit N-1.
    pub compatibility: u32,
    /// `$DMA`, shown for HDD games only; read, never written here.
    pub dma: Option<i32>,
    /// An ELF on the disc to boot instead of the one in `SYSTEM.CNF`.
    pub alt_startup: String,
    /// `$VMC_0` and `$VMC_1`: card names for slots 1 and 2, or empty.
    pub vmc: [String; 2],
    pub gsm: Gsm,
    pub cheats: Cheats,
    pub pad_emu: PadEmu,
}

/// Why a setting cannot be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error(
        "compatibility bits {value:#x} are not all known to this loader (allowed {allowed:#x})"
    )]
    CompatibilityBits { value: u32, allowed: u32 },
    #[error("alternative startup {0:?} is longer than {ALT_STARTUP_MAX} characters")]
    AltStartupTooLong(String),
    #[error("alternative startup {0:?} has characters OPL's keyboard cannot type")]
    AltStartupChars(String),
    #[error("VMC slot {slot}: {error}")]
    VmcName { slot: u8, error: VmcNameError },
    #[error("GSM video mode {0} is not one of OPL's 0 to {GSM_VMODE_MAX}")]
    GsmVmode(i32),
    #[error("cheat mode {0} is not 0 or {CHEAT_MODE_MAX}")]
    CheatMode(i32),
    #[error(transparent)]
    Cfg(#[from] CfgError),
}

/// One key an edit changes: `None` is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChange {
    pub key: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

fn int(doc: &CfgDocument, key: &str) -> i32 {
    doc.get_int(key).unwrap_or(0)
}

/// `$AltStartup` as OPL's keyboard leaves it: a-z made upper case.
pub fn normalize_alt_startup(text: &str) -> String {
    text.chars().map(|c| c.to_ascii_uppercase()).collect()
}

impl GameSettings {
    pub fn read(doc: &CfgDocument) -> Self {
        let present = |key: &str| doc.get(key).is_some();
        Self {
            compatibility: int(doc, COMPATIBILITY) as u32,
            dma: doc.get_int(DMA),
            alt_startup: doc.get(ALT_STARTUP).unwrap_or_default(),
            vmc: VMC_SLOTS.map(|key| doc.get(key).unwrap_or_default()),
            gsm: Gsm {
                per_game: present(GSM_SOURCE),
                enabled: int(doc, ENABLE_GSM) != 0,
                vmode: int(doc, GSM_VMODE),
                x_offset: int(doc, GSM_X_OFFSET),
                y_offset: int(doc, GSM_Y_OFFSET),
                field_fix: int(doc, GSM_FIELD_FIX) != 0,
            },
            cheats: Cheats {
                per_game: present(CHEATS_SOURCE),
                enabled: int(doc, ENABLE_CHEAT) != 0,
                mode: int(doc, CHEAT_MODE),
            },
            pad_emu: PadEmu {
                per_game: present(PADEMU_SOURCE),
                enabled: int(doc, ENABLE_PADEMU) != 0,
                settings: int(doc, PADEMU_SETTINGS) as u32,
            },
        }
    }

    /// `$AltStartup` to write: upper case like OPL's keyboard makes it, or
    /// the value already there, byte for byte, when it was not edited.
    fn alt_startup_after(&self, before: &Self) -> String {
        if self.alt_startup == before.alt_startup {
            before.alt_startup.clone()
        } else {
            normalize_alt_startup(&self.alt_startup)
        }
    }

    /// Checks the settings that differ from `before` (what the file says
    /// now): a value OPL would misread, or that its menu could not produce.
    /// Settings left as they are pass whatever they hold.
    pub fn validate(&self, before: &Self, profile: LoaderProfile) -> Result<(), SettingsError> {
        // Bits a file already has stay (0.9.3's modes 7 and 8 in community
        // CFGs, RiptOPL's from another stick); new ones must be known.
        let allowed = profile.compat_mask();
        if self.compatibility & !before.compatibility & !allowed != 0 {
            return Err(SettingsError::CompatibilityBits {
                value: self.compatibility,
                allowed,
            });
        }
        let alt = self.alt_startup_after(before);
        if alt != before.alt_startup {
            if alt.len() > ALT_STARTUP_MAX {
                return Err(SettingsError::AltStartupTooLong(alt));
            }
            // An ISO9660 file name, maybe in a folder; OPL adds the `;1`.
            if !alt
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '\\' | '-'))
            {
                return Err(SettingsError::AltStartupChars(alt));
            }
        }
        for (slot, name) in self.vmc.iter().enumerate() {
            if !name.is_empty() && *name != before.vmc[slot] {
                vmc::validate_name(name).map_err(|error| SettingsError::VmcName {
                    slot: slot as u8,
                    error,
                })?;
            }
        }
        let vmode = self.gsm.vmode;
        if vmode != before.gsm.vmode && !(0..=GSM_VMODE_MAX).contains(&vmode) {
            return Err(SettingsError::GsmVmode(vmode));
        }
        let mode = self.cheats.mode;
        if mode != before.cheats.mode && !(0..=CHEAT_MODE_MAX).contains(&mode) {
            return Err(SettingsError::CheatMode(mode));
        }
        Ok(())
    }

    /// Writes into `doc` the settings that differ from what it says now,
    /// the way OPL's menu saves them. Returns the keys that changed, in the
    /// order they were changed.
    pub fn apply(
        &self,
        doc: &mut CfgDocument,
        profile: LoaderProfile,
    ) -> Result<Vec<KeyChange>, SettingsError> {
        let before = Self::read(doc);
        self.validate(&before, profile)?;
        let mut edit = Edit {
            doc,
            changes: Vec::new(),
            set_any: false,
        };
        if self.compatibility != before.compatibility {
            edit.int_or_remove(COMPATIBILITY, self.compatibility as i32)?;
        }

        let gsm = (&self.gsm, &before.gsm);
        edit.source(GSM_SOURCE, gsm.0.per_game, gsm.1.per_game)?;
        if gsm.0.per_game {
            edit.changed(ENABLE_GSM, gsm.0.enabled.into(), gsm.1.enabled.into())?;
            edit.changed(GSM_VMODE, gsm.0.vmode, gsm.1.vmode)?;
            edit.changed(GSM_X_OFFSET, gsm.0.x_offset, gsm.1.x_offset)?;
            edit.changed(GSM_Y_OFFSET, gsm.0.y_offset, gsm.1.y_offset)?;
            edit.changed(
                GSM_FIELD_FIX,
                gsm.0.field_fix.into(),
                gsm.1.field_fix.into(),
            )?;
        }

        let cheats = (&self.cheats, &before.cheats);
        edit.source(CHEATS_SOURCE, cheats.0.per_game, cheats.1.per_game)?;
        if cheats.0.per_game {
            edit.changed(
                ENABLE_CHEAT,
                cheats.0.enabled.into(),
                cheats.1.enabled.into(),
            )?;
            edit.changed(CHEAT_MODE, cheats.0.mode, cheats.1.mode)?;
        }

        let pad = (&self.pad_emu, &before.pad_emu);
        edit.source(PADEMU_SOURCE, pad.0.per_game, pad.1.per_game)?;
        if pad.0.per_game {
            edit.changed(ENABLE_PADEMU, pad.0.enabled.into(), pad.1.enabled.into())?;
            edit.changed(
                PADEMU_SETTINGS,
                pad.0.settings as i32,
                pad.1.settings as i32,
            )?;
        }

        let alt = self.alt_startup_after(&before);
        if alt != before.alt_startup {
            edit.text_or_remove(ALT_STARTUP, &alt, true)?;
        }
        if edit.set_any && edit.doc.get(CONFIG_SOURCE).as_deref() != Some(SOURCE_USER) {
            edit.text_or_remove(CONFIG_SOURCE, SOURCE_USER, false)?;
        }
        // VMC slots last, as OPL saves them, and without `$ConfigSource`.
        for (slot, key) in VMC_SLOTS.iter().enumerate() {
            if self.vmc[slot] != before.vmc[slot] {
                edit.text_or_remove(key, &self.vmc[slot], false)?;
            }
        }
        Ok(edit.changes)
    }
}

struct Edit<'a> {
    doc: &'a mut CfgDocument,
    changes: Vec<KeyChange>,
    /// A settings key was set (OPL's `result`).
    set_any: bool,
}

impl Edit<'_> {
    fn put(&mut self, key: &str, value: Option<&str>, counts: bool) -> Result<(), SettingsError> {
        let before = self.doc.get(key);
        match value {
            Some(value) => {
                self.doc.set(key, value)?;
                self.set_any |= counts;
            }
            None => {
                self.doc.remove(key);
            }
        }
        let after = self.doc.get(key);
        if before != after {
            self.changes.push(KeyChange {
                key: key.to_owned(),
                before,
                after,
            });
        }
        Ok(())
    }

    fn text_or_remove(
        &mut self,
        key: &str,
        value: &str,
        counts: bool,
    ) -> Result<(), SettingsError> {
        self.put(key, (!value.is_empty()).then_some(value), counts)
    }

    fn int_or_remove(&mut self, key: &str, value: i32) -> Result<(), SettingsError> {
        let text = value.to_string();
        self.put(key, (value != 0).then_some(text.as_str()), true)
    }

    fn changed(&mut self, key: &str, after: i32, before: i32) -> Result<(), SettingsError> {
        if after != before {
            self.int_or_remove(key, after)?;
        }
        Ok(())
    }

    fn source(&mut self, key: &str, after: bool, before: bool) -> Result<(), SettingsError> {
        match (after, before) {
            (true, false) => self.put(key, Some("1"), true),
            (false, true) => self.put(key, None, false),
            _ => Ok(()),
        }
    }
}

/// Keys as text, for `--set KEY=VALUE` and the plan view: the setting
/// keys OPL's menu writes, with what they hold.
pub fn known_key(key: &str) -> bool {
    [
        COMPATIBILITY,
        DMA,
        ALT_STARTUP,
        CONFIG_SOURCE,
        GSM_SOURCE,
        ENABLE_GSM,
        GSM_VMODE,
        GSM_X_OFFSET,
        GSM_Y_OFFSET,
        GSM_FIELD_FIX,
        CHEATS_SOURCE,
        ENABLE_CHEAT,
        CHEAT_MODE,
        PADEMU_SOURCE,
        ENABLE_PADEMU,
        PADEMU_SETTINGS,
    ]
    .contains(&key)
        || VMC_SLOTS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> CfgDocument {
        CfgDocument::parse(text.as_bytes())
    }

    fn text(doc: &CfgDocument) -> String {
        String::from_utf8(doc.to_bytes()).unwrap()
    }

    const OPL: LoaderProfile = LoaderProfile::Opl120Beta;

    #[test]
    fn reads_like_a_launch() {
        let settings = GameSettings::read(&doc(
            "$Compatibility=5\r\n$VMC_0=SLUS_200.62_0\r\n$GSMSource=0\r\n$EnableGSM=1\r\n$GSMVMode=8\r\n$CheatMode=1\r\n",
        ));
        assert_eq!(settings.compatibility, 5);
        assert_eq!(settings.vmc, ["SLUS_200.62_0".to_owned(), String::new()]);
        // The key's presence counts, not its value (InitGSMConfig).
        assert!(settings.gsm.per_game && settings.gsm.enabled);
        assert_eq!(settings.gsm.vmode, 8);
        assert!(!settings.cheats.per_game);
        assert_eq!(settings.cheats.mode, 1);
        assert_eq!(GameSettings::read(&doc("")), GameSettings::default());
    }

    #[test]
    fn unchanged_settings_touch_nothing() {
        let original = "Title=Ico\r\n$Compatibility=0\r\n$EnableGSM=0\r\n# kept\nodd line\n";
        let mut cfg = doc(original);
        let settings = GameSettings::read(&cfg);
        assert!(settings.apply(&mut cfg, OPL).unwrap().is_empty());
        assert_eq!(text(&cfg), original);
    }

    #[test]
    fn modes_are_set_in_place_and_removed_at_zero() {
        let mut cfg = doc("Title=Ico\r\n$Compatibility=1\r\nZzz=2\r\n");
        let mut settings = GameSettings::read(&cfg);
        settings.compatibility = 0b10_0010;
        let changes = settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "Title=Ico\r\n$Compatibility=34\r\nZzz=2\r\n$ConfigSource=1\r\n"
        );
        assert_eq!(changes[0].before.as_deref(), Some("1"));
        assert_eq!(changes[0].after.as_deref(), Some("34"));
        settings.compatibility = 0;
        let changes = settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(text(&cfg), "Title=Ico\r\nZzz=2\r\n$ConfigSource=1\r\n");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].after, None);
    }

    #[test]
    fn riptopl_modes_only_in_its_profile() {
        let mut cfg = doc("");
        let settings = GameSettings {
            compatibility: 0x41,
            ..GameSettings::default()
        };
        assert_eq!(
            settings.apply(&mut cfg, OPL),
            Err(SettingsError::CompatibilityBits {
                value: 0x41,
                allowed: 0x3F
            })
        );
        assert_eq!(text(&cfg), "");
        settings.apply(&mut cfg, LoaderProfile::RiptOpl).unwrap();
        assert_eq!(text(&cfg), "$Compatibility=65\r\n$ConfigSource=1\r\n");
    }

    #[test]
    fn gsm_per_game_and_back_to_global() {
        let mut cfg = doc("$Compatibility=2\r\n");
        let mut settings = GameSettings::read(&cfg);
        settings.gsm = Gsm {
            per_game: true,
            enabled: true,
            vmode: 10,
            x_offset: -4,
            y_offset: 0,
            field_fix: true,
        };
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "$Compatibility=2\r\n$GSMSource=1\r\n$EnableGSM=1\r\n$GSMVMode=10\r\n$GSMXOffset=-4\r\n$GSMFIELDFix=1\r\n$ConfigSource=1\r\n"
        );
        // Global again: only the source goes, the values wait for later.
        settings.gsm.per_game = false;
        let changes = settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "$Compatibility=2\r\n$EnableGSM=1\r\n$GSMVMode=10\r\n$GSMXOffset=-4\r\n$GSMFIELDFix=1\r\n$ConfigSource=1\r\n"
        );
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, GSM_SOURCE);
        assert!(!GameSettings::read(&cfg).gsm.per_game);
    }

    #[test]
    fn cheats_and_pademu() {
        let mut cfg = doc("");
        let settings = GameSettings {
            cheats: Cheats {
                per_game: true,
                enabled: true,
                mode: 0,
            },
            pad_emu: PadEmu {
                per_game: true,
                enabled: true,
                settings: pademu::port(0) | pademu::vibration(0),
            },
            ..GameSettings::default()
        };
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "$CheatsSource=1\r\n$EnableCheat=1\r\n$PADEMUSource=1\r\n$EnablePadEmu=1\r\n$PadEmuSettings=65792\r\n$ConfigSource=1\r\n"
        );
        assert_eq!(GameSettings::read(&cfg), settings);
    }

    #[test]
    fn alt_startup_is_upper_case_and_limited() {
        let mut cfg = doc("");
        let mut settings = GameSettings {
            alt_startup: "slus_200.62".into(),
            ..GameSettings::default()
        };
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(text(&cfg), "$AltStartup=SLUS_200.62\r\n$ConfigSource=1\r\n");
        settings.alt_startup = "A".repeat(22);
        assert!(matches!(
            settings.apply(&mut cfg, OPL),
            Err(SettingsError::AltStartupTooLong(_))
        ));
        settings.alt_startup = "MAIN.ELF;1".into();
        assert!(matches!(
            settings.apply(&mut cfg, OPL),
            Err(SettingsError::AltStartupChars(_))
        ));
        settings.alt_startup = "ПУСК".into();
        assert!(matches!(
            settings.apply(&mut cfg, OPL),
            Err(SettingsError::AltStartupChars(_))
        ));
        settings.alt_startup.clear();
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(text(&cfg), "$ConfigSource=1\r\n");
    }

    #[test]
    fn vmc_slots_do_not_mark_the_config_as_the_users() {
        let mut cfg = doc("Title=Ico\r\n");
        let settings = GameSettings {
            vmc: ["SLUS_200.62_0".into(), "generic_1".into()],
            ..GameSettings::default()
        };
        let changes = settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "Title=Ico\r\n$VMC_0=SLUS_200.62_0\r\n$VMC_1=generic_1\r\n"
        );
        assert_eq!(changes.len(), 2);
        let bad = GameSettings {
            vmc: ["x".repeat(32), String::new()],
            ..GameSettings::default()
        };
        assert!(matches!(
            bad.apply(&mut cfg, OPL),
            Err(SettingsError::VmcName { slot: 0, .. })
        ));
    }

    #[test]
    fn values_out_of_opls_range_are_refused() {
        let mut cfg = doc("");
        let mut settings = GameSettings::default();
        settings.gsm.vmode = 29;
        assert_eq!(
            settings.validate(&GameSettings::default(), OPL),
            Err(SettingsError::GsmVmode(29))
        );
        settings.gsm.vmode = 0;
        settings.cheats.mode = 2;
        assert_eq!(
            settings.apply(&mut cfg, OPL),
            Err(SettingsError::CheatMode(2))
        );
    }

    #[test]
    fn line_endings_and_unknown_keys_stay() {
        // An LF file with a comment, a BOM and a key OPL never wrote.
        let original = "\u{FEFF}Title=Ico\n#note\n$Compatibility=1\nVmode=vmode/ntsc\n";
        let mut cfg = doc(original);
        let mut settings = GameSettings::read(&cfg);
        settings.compatibility = 3;
        settings.vmc[0] = "ico_0".into();
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(
            text(&cfg),
            "\u{FEFF}Title=Ico\n#note\n$Compatibility=3\nVmode=vmode/ntsc\n$ConfigSource=1\r\n$VMC_0=ico_0\r\n"
        );
    }

    #[test]
    fn odd_values_already_there_do_not_block_other_edits() {
        // Written by another tool: a lower-case startup with `;1`, 0.9.3's
        // mode 7 on stock OPL, a VMC name too long for OPL's buffer.
        let long = "v".repeat(40);
        let original = format!("$AltStartup=main.elf;1\r\n$Compatibility=100\r\n$VMC_1={long}\r\n");
        let mut cfg = doc(&original);
        let mut settings = GameSettings::read(&cfg);
        settings.vmc[0] = "ico_0".into();
        settings.apply(&mut cfg, OPL).unwrap();
        assert_eq!(text(&cfg), format!("{original}$VMC_0=ico_0\r\n"));
        // A mode changes; the old bit stays, as unknown keys do.
        settings.compatibility = 100 | 1;
        settings.apply(&mut cfg, OPL).unwrap();
        assert!(text(&cfg).contains("$Compatibility=101\r\n"));
    }

    #[test]
    fn known_keys() {
        assert!(known_key("$Compatibility") && known_key("$VMC_1"));
        assert!(!known_key("Title") && !known_key("$CheatSel000"));
    }
}
