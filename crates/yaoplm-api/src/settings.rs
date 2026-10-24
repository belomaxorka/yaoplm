//! A game's settings and memory cards on the wire (CFG-2, VMC-1).
//!
//! `ReadCfg` shows what a game's CFG says, `WriteCfg` plans a change to it
//! (with the cards it gets), `ListVmc` lists `VMC/`, `CreateVmc` plans
//! blank cards. The rules live in `yaoplm_core::settings` and
//! `yaoplm_formats::game_cfg`; these types only carry them.

use std::path::Path;

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_core::settings as core;
use yaoplm_formats::game_cfg as cfg;

use crate::paths::display_path;
use crate::phrase::Phrase;

/// GSM: a video mode OPL forces on the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GsmSettings {
    /// These values rather than `conf_game.cfg`'s (`$GSMSource`).
    pub per_game: bool,
    pub enabled: bool,
    /// 0 to 28, as OPL's list.
    pub vmode: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub field_fix: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CheatSettings {
    pub per_game: bool,
    pub enabled: bool,
    /// 0: every code; 1: pick at launch (OPL 1.2).
    pub mode: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PadEmuSettings {
    pub per_game: bool,
    pub enabled: bool,
    /// Bit 0 Bluetooth; bit 8 + port emulation; bit 16 + port vibration;
    /// bits 24 to 26 multitap, its port and the fake-DS3 workaround.
    pub settings: u32,
}

/// What OPL's game menu edits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    /// `$Compatibility`: mode N is bit N - 1.
    pub compatibility: u32,
    /// `$DMA` (HDD only): shown, never written.
    pub dma: Option<i32>,
    /// `$AltStartup`: an ELF on the disc to boot instead.
    pub alt_startup: String,
    /// `$VMC_0` and `$VMC_1`; empty for none.
    pub vmc: Vec<String>,
    pub gsm: GsmSettings,
    pub cheats: CheatSettings,
    pub pad_emu: PadEmuSettings,
}

impl From<&cfg::GameSettings> for GameSettings {
    fn from(s: &cfg::GameSettings) -> Self {
        Self {
            compatibility: s.compatibility,
            dma: s.dma,
            alt_startup: s.alt_startup.clone(),
            vmc: s.vmc.to_vec(),
            gsm: GsmSettings {
                per_game: s.gsm.per_game,
                enabled: s.gsm.enabled,
                vmode: s.gsm.vmode,
                x_offset: s.gsm.x_offset,
                y_offset: s.gsm.y_offset,
                field_fix: s.gsm.field_fix,
            },
            cheats: CheatSettings {
                per_game: s.cheats.per_game,
                enabled: s.cheats.enabled,
                mode: s.cheats.mode,
            },
            pad_emu: PadEmuSettings {
                per_game: s.pad_emu.per_game,
                enabled: s.pad_emu.enabled,
                settings: s.pad_emu.settings,
            },
        }
    }
}

impl From<&GameSettings> for cfg::GameSettings {
    fn from(s: &GameSettings) -> Self {
        let slot = |n: usize| s.vmc.get(n).cloned().unwrap_or_default();
        Self {
            compatibility: s.compatibility,
            dma: s.dma,
            alt_startup: s.alt_startup.clone(),
            vmc: [slot(0), slot(1)],
            gsm: cfg::Gsm {
                per_game: s.gsm.per_game,
                enabled: s.gsm.enabled,
                vmode: s.gsm.vmode,
                x_offset: s.gsm.x_offset,
                y_offset: s.gsm.y_offset,
                field_fix: s.gsm.field_fix,
            },
            cheats: cfg::Cheats {
                per_game: s.cheats.per_game,
                enabled: s.cheats.enabled,
                mode: s.cheats.mode,
            },
            pad_emu: cfg::PadEmu {
                per_game: s.pad_emu.per_game,
                enabled: s.pad_emu.enabled,
                settings: s.pad_emu.settings,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CfgPair {
    pub key: String,
    pub value: String,
}

/// A game's CFG (`ReadCfg`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CfgView {
    pub startup: String,
    /// Where the file is, or would be created.
    pub path: String,
    pub exists: bool,
    /// False for wOPL's libconfig files, which YAOPLM never rewrites.
    pub editable: bool,
    pub settings: GameSettings,
    /// Every key, as OPL loads it.
    pub entries: Vec<CfgPair>,
    /// The names OPL's VMC menu suggests for slots 1 and 2.
    pub card_names: Vec<String>,
}

impl CfgView {
    pub fn new(root: &Path, cfg: &core::GameCfg) -> Self {
        Self {
            startup: cfg.startup.clone(),
            path: display_path(&root.join(cfg.path.components().collect::<std::path::PathBuf>())),
            exists: cfg.exists,
            editable: cfg.editable,
            settings: (&cfg.settings).into(),
            entries: cfg
                .entries
                .iter()
                .map(|e| CfgPair {
                    key: e.key.clone(),
                    value: e.value.clone(),
                })
                .collect(),
            card_names: (0..2)
                .map(|slot| core::default_card_name(&cfg.startup, slot))
                .collect(),
        }
    }
}

/// A blank card for `WriteCfg` and `CreateVmc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct NewVmc {
    /// `VMC/<name>.bin`, at most 31 characters.
    pub name: String,
    /// 8, 16, 32 or 64.
    pub size_mb: u32,
    /// A card with this name may be replaced: it moves to the backup
    /// folder first, saves and all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub replace: Option<bool>,
}

impl From<&NewVmc> for core::NewCard {
    fn from(card: &NewVmc) -> Self {
        Self {
            name: card.name.clone(),
            size_mb: card.size_mb,
            replace: card.replace.unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VmcUser {
    pub startup: String,
    /// 0 for slot 1, 1 for slot 2.
    pub slot: u8,
}

/// A file in `VMC/` (`ListVmc`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VmcCard {
    /// What `$VMC_0`/`$VMC_1` say: the file name without `.bin`.
    pub name: String,
    pub path: String,
    pub size: u64,
    /// The size in MiB when OPL would take the card.
    pub size_mb: Option<u32>,
    /// Why OPL would not, in English.
    pub problem: Option<String>,
    /// `problem` for front ends that translate it.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_phrase: Option<Phrase>,
    /// Saves on the card, when its root directory could be read.
    pub saves: Option<u32>,
    pub used_by: Vec<VmcUser>,
}

impl VmcCard {
    pub fn new(root: &Path, card: &core::Card) -> Self {
        Self {
            name: card.name.clone(),
            path: display_path(&root.join(card.path.components().collect::<std::path::PathBuf>())),
            size: card.len,
            size_mb: card.size_mb,
            problem: card
                .problem
                .as_ref()
                .map(|p| core::card_problem(p).english()),
            problem_phrase: card
                .problem
                .as_ref()
                .map(|p| Phrase::from(&core::card_problem(p))),
            saves: card.saves,
            used_by: card
                .used_by
                .iter()
                .map(|(startup, slot)| VmcUser {
                    startup: startup.clone(),
                    slot: *slot,
                })
                .collect(),
        }
    }
}
