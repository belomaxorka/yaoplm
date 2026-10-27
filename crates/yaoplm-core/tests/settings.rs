//! A game's settings and its memory cards (CFG-2, CFG-5, VMC-1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{files, run};
use yaoplm_core::exec::Outcome;
use yaoplm_core::ops::{GameRef, OpError};
use yaoplm_core::plan::{Expect, PlanKind, Step};
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_core::settings::{
    NewCard, SettingsEdit, list_cards, plan_cards, plan_settings, read_game_cfg,
};
use yaoplm_device::{FsKind, RelPath};
use yaoplm_formats::LoaderProfile;
use yaoplm_formats::game_cfg::GameSettings;
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_formats::vmc::{self, McTime};
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

const NOW: i64 = 1_790_000_000;
const OPL: LoaderProfile = LoaderProfile::Opl120Beta;
const MIB: u64 = 1 << 20;

fn game() -> GameRef {
    GameRef::Image {
        path: RelPath::new("CD/SLUS_200.62.Retail.iso").unwrap(),
    }
}

/// A stick with one game and a CFG that has a key OPL never wrote.
fn library() -> MemFs {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file(
        "CD/SLUS_200.62.Retail.iso",
        &Variant::Retail.build().to_vec(),
    );
    fs.insert_file(
        "CFG/SLUS_200.62.cfg",
        b"Title=Retail\r\n$Compatibility=1\r\nVmode=vmode/ntsc\r\n",
    );
    fs
}

fn settings(fs: &MemFs) -> GameSettings {
    read_game_cfg(fs, &game()).unwrap().settings
}

fn edit(settings: GameSettings) -> SettingsEdit {
    SettingsEdit {
        settings: Some(settings),
        ..SettingsEdit::default()
    }
}

fn backups(fs: &MemFs) -> Vec<String> {
    fs.files()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.starts_with(".yaoplm/backup/"))
        .collect()
}

#[test]
fn reads_the_games_settings() {
    let fs = library();
    let cfg = read_game_cfg(&fs, &game()).unwrap();
    assert_eq!(cfg.startup, "SLUS_200.62");
    assert_eq!(cfg.path.as_str(), "CFG/SLUS_200.62.cfg");
    assert!(cfg.exists && cfg.editable);
    assert_eq!(cfg.settings.compatibility, 1);
    assert_eq!(cfg.entries.len(), 3);

    let empty = MemFs::new(FsKind::Fat32);
    empty.insert_file(
        "CD/SLUS_200.62.Retail.iso",
        &Variant::Retail.build().to_vec(),
    );
    let cfg = read_game_cfg(&empty, &game()).unwrap();
    assert!(!cfg.exists && cfg.editable);
    assert_eq!(cfg.settings, GameSettings::default());
}

#[test]
fn modes_change_with_a_backup_and_nothing_else_moves() {
    let fs = library();
    let mut wanted = settings(&fs);
    wanted.compatibility = 0b1010;
    let plan = plan_settings(&fs, &game(), &edit(wanted), OPL, NOW).unwrap();
    assert_eq!(plan.kind, PlanKind::Settings);
    let tx = &plan.transactions[0];
    assert_eq!(tx.game.as_ref().unwrap().startup, "SLUS_200.62");
    assert!(matches!(&tx.steps[0], Step::CopyFile { from, to }
        if from.as_str() == "CFG/SLUS_200.62.cfg" && to.as_str().ends_with("/CFG/SLUS_200.62.cfg")));
    assert!(matches!(
        &tx.steps[1],
        Step::PutFile {
            expect: Expect::Content(_),
            ..
        }
    ));
    // Every step can be undone: the CFG write is the commit point.
    assert_eq!(tx.commit, tx.steps.len());
    let keys: Vec<_> = tx
        .cfg_changes
        .iter()
        .map(|c| (c.key.as_str(), c.before.as_deref(), c.after.as_deref()))
        .collect();
    assert_eq!(
        keys,
        [
            ("$Compatibility", Some("1"), Some("10")),
            ("$ConfigSource", None, Some("1"))
        ]
    );

    let report = run(&plan, &fs, &VirtualSources::new());
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"Title=Retail\r\n$Compatibility=10\r\nVmode=vmode/ntsc\r\n$ConfigSource=1\r\n"
    );
    let saved = backups(&fs);
    assert_eq!(saved.len(), 1);
    assert_eq!(
        fs.bytes(&saved[0]).unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nVmode=vmode/ntsc\r\n"
    );
}

#[test]
fn a_new_cfg_is_created_with_its_folder() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file("DVD/SLES_500.51.Ico.iso", &Variant::Retail.build().to_vec());
    let game = GameRef::Image {
        path: RelPath::new("DVD/SLES_500.51.Ico.iso").unwrap(),
    };
    let wanted = GameSettings {
        compatibility: 1,
        ..GameSettings::default()
    };
    let plan = plan_settings(&fs, &game, &edit(wanted), OPL, NOW).unwrap();
    let steps = &plan.transactions[0].steps;
    assert!(matches!(&steps[0], Step::CreateDir { path } if path.as_str() == "CFG"));
    assert!(matches!(
        &steps[1],
        Step::PutFile {
            expect: Expect::Absent,
            ..
        }
    ));
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(
        fs.bytes("CFG/SLES_500.51.cfg").unwrap(),
        b"$Compatibility=1\r\n$ConfigSource=1\r\n"
    );
    assert!(backups(&fs).is_empty());
}

#[test]
fn nothing_to_change_is_not_a_plan() {
    let fs = library();
    let plan = plan_settings(&fs, &game(), &edit(settings(&fs)), OPL, NOW).unwrap();
    assert!(!plan.is_executable());
    assert!(plan.transactions.is_empty());
    assert_eq!(plan.notices[0].code, "settingsUnchanged");
}

#[test]
fn opl_would_not_understand_it() {
    let fs = library();
    let mut wanted = settings(&fs);
    wanted.compatibility = 0x40;
    let err = plan_settings(&fs, &game(), &edit(wanted.clone()), OPL, NOW).unwrap_err();
    assert!(matches!(&err, OpError::Invalid(p) if p.key == "errors.settingsInvalid"));
    assert!(err.to_string().contains("0x40"), "{err}");
    // RiptOPL knows modes 7 and 8.
    assert!(plan_settings(&fs, &game(), &edit(wanted), LoaderProfile::RiptOpl, NOW).is_ok());

    let raw = SettingsEdit {
        set: vec![("#Name".into(), "x".into())],
        ..SettingsEdit::default()
    };
    let err = plan_settings(&fs, &game(), &raw, OPL, NOW).unwrap_err();
    assert!(err.to_string().contains("#Name"), "{err}");
    let raw = SettingsEdit {
        set: vec![("$GSMVMode".into(), "40".into())],
        ..SettingsEdit::default()
    };
    assert!(plan_settings(&fs, &game(), &raw, OPL, NOW).is_err());
}

#[test]
fn raw_keys_are_set_and_removed_as_given() {
    let fs = library();
    let raw = SettingsEdit {
        set: vec![("Genre".into(), "Action".into())],
        unset: vec!["Vmode".into()],
        ..SettingsEdit::default()
    };
    let plan = plan_settings(&fs, &game(), &raw, OPL, NOW).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nGenre=Action\r\n"
    );
}

#[test]
fn wopl_libconfig_is_never_rewritten() {
    let fs = library();
    fs.insert_file("CFG/SLUS_200.62.cfg", b"compat :\n{\n  mode = 3;\n};\n");
    let cfg = read_game_cfg(&fs, &game()).unwrap();
    assert!(cfg.exists && !cfg.editable);
    let err = plan_settings(&fs, &game(), &edit(GameSettings::default()), OPL, NOW).unwrap_err();
    assert!(matches!(&err, OpError::Invalid(p) if p.key == "errors.cfgNotOpl"));
}

fn card(name: &str, size_mb: u32) -> NewCard {
    NewCard {
        name: name.into(),
        size_mb,
        replace: false,
    }
}

#[test]
fn a_card_is_created_and_assigned_in_one_transaction() {
    let fs = library();
    let mut wanted = settings(&fs);
    wanted.vmc[0] = "SLUS_200.62_0".into();
    let plan = plan_settings(
        &fs,
        &game(),
        &SettingsEdit {
            settings: Some(wanted),
            cards: vec![card("SLUS_200.62_0", 8)],
            ..SettingsEdit::default()
        },
        OPL,
        NOW,
    )
    .unwrap();
    assert_eq!(plan.transactions.len(), 1);
    let steps = &plan.transactions[0].steps;
    assert!(matches!(&steps[0], Step::CreateDir { path } if path.as_str() == "VMC"));
    assert!(
        matches!(&steps[1], Step::WriteVmc { path, size_mb: 8, created: NOW }
        if path.as_str() == "VMC/SLUS_200.62_0.bin")
    );
    assert_eq!(plan.write_bytes(), 8 * MIB + steps_cfg_len(&plan));
    // The slot names a card that will be there: no warning.
    assert!(plan.transactions[0].notices.is_empty());

    let report = run(&plan, &fs, &VirtualSources::new());
    assert!(report.succeeded(), "{:?}", report.error);
    let card = fs.bytes("VMC/SLUS_200.62_0.bin").unwrap();
    assert_eq!(card, vmc::blank_card(8, McTime::from_unix(NOW)).unwrap());
    assert_eq!(vmc::opl_check(&card, card.len() as u64), Ok(8));
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nVmode=vmode/ntsc\r\n$VMC_0=SLUS_200.62_0\r\n"
    );
    assert!(!fs.exists("VMC/SLUS_200.62_0.bin.yaoplm-part"));

    let cards = list_cards(&fs).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].name, "SLUS_200.62_0");
    assert_eq!((cards[0].size_mb, cards[0].saves), (Some(8), Some(0)));
    assert_eq!(cards[0].used_by, [("SLUS_200.62".to_owned(), 0)]);
}

fn steps_cfg_len(plan: &yaoplm_core::plan::Plan) -> u64 {
    plan.transactions[0]
        .steps
        .iter()
        .filter(|s| matches!(s, Step::PutFile { .. }))
        .map(Step::write_bytes)
        .sum()
}

#[test]
fn an_existing_card_needs_a_request_to_replace_it() {
    let fs = library();
    fs.insert_file("VMC/SLUS_200.62_0.bin", &[0u8; 4096]);
    let plan_card = |replace: bool| {
        plan_cards(
            &fs,
            &[NewCard {
                replace,
                ..card("slus_200.62_0", 16)
            }],
            NOW,
        )
    };
    assert!(matches!(plan_card(false), Err(OpError::Conflict(p)) if p.key == "errors.vmcExists"));

    let plan = plan_card(true).unwrap();
    assert_eq!(plan.kind, PlanKind::Vmc);
    let tx = &plan.transactions[0];
    assert_eq!(tx.notices[0].code, "vmcReplaced");
    assert!(
        !plan.deletes_user_data(),
        "the old card is kept, not deleted"
    );
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    // Same name as before, in the case the device had.
    assert_eq!(fs.data("VMC/SLUS_200.62_0.bin").unwrap().len(), 16 * MIB);
    let saved = backups(&fs);
    assert!(saved[0].ends_with("/VMC/SLUS_200.62_0.bin"), "{saved:?}");
    assert_eq!(fs.bytes(&saved[0]).unwrap(), vec![0u8; 4096]);
}

#[test]
fn a_failed_cfg_write_takes_the_new_card_back() {
    let fs = library();
    fs.insert_file("VMC/SLUS_200.62_0.bin", b"old saves");
    let mut wanted = settings(&fs);
    wanted.vmc[0] = "SLUS_200.62_0".into();
    let plan = plan_settings(
        &fs,
        &game(),
        &SettingsEdit {
            settings: Some(wanted),
            cards: vec![NewCard {
                replace: true,
                ..card("SLUS_200.62_0", 8)
            }],
            ..SettingsEdit::default()
        },
        OPL,
        NOW,
    )
    .unwrap();
    fs.fail_once("create", "SLUS_200.62.cfg.yaoplm-tmp");
    let report = run(&plan, &fs, &VirtualSources::new());
    assert!(!report.succeeded());
    assert_eq!(report.outcomes[0], Outcome::RolledBack);
    assert_eq!(fs.bytes("VMC/SLUS_200.62_0.bin").unwrap(), b"old saves");
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nVmode=vmode/ntsc\r\n"
    );
    assert!(report.journal.is_none());
}

#[test]
fn a_pulled_stick_mid_card_is_recovered() {
    for action in [RecoveryAction::Rollback, RecoveryAction::Resume] {
        let fs = library();
        let plan = plan_cards(&fs, &[card("generic_0", 8)], NOW).unwrap();
        fs.unplug_after(3 * MIB);
        let report = run(&plan, &fs, &VirtualSources::new());
        assert_eq!(report.outcomes[0], Outcome::Interrupted);
        fs.replug();
        assert!(fs.exists("VMC/generic_0.bin.yaoplm-part"));

        let recovery = plan_recovery(&fs, action).unwrap();
        let report = run(&recovery, &fs, &VirtualSources::new());
        assert!(report.succeeded(), "{action:?}: {:?}", report.error);
        assert!(!fs.exists("VMC/generic_0.bin.yaoplm-part"));
        match action {
            RecoveryAction::Rollback => assert!(!fs.exists("VMC/generic_0.bin")),
            RecoveryAction::Resume => assert_eq!(
                fs.bytes("VMC/generic_0.bin").unwrap(),
                vmc::blank_card(8, McTime::from_unix(NOW)).unwrap()
            ),
        }
    }
}

#[test]
fn a_fragmented_card_is_reported() {
    let fs = library();
    fs.set_fragments("VMC/generic_1.bin", 3);
    let plan = plan_cards(&fs, &[card("generic_1", 8)], NOW).unwrap();
    let report = run(&plan, &fs, &VirtualSources::new());
    assert!(report.succeeded());
    assert_eq!(report.notices.len(), 1);
    assert_eq!(report.notices[0].1.code, "vmcFragmented");
    assert!(
        report.notices[0].1.message.contains("(3)"),
        "{}",
        report.notices[0].1.message
    );

    fs.set_fragments("VMC/generic_0.bin", 1);
    let plan = plan_cards(&fs, &[card("generic_0", 8)], NOW).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).notices.is_empty());
}

#[test]
fn bad_card_requests_are_refused() {
    let fs = library();
    for (cards, error) in [
        (vec![card("x", 12)], "errors.vmcSize"),
        (vec![card(&"x".repeat(32), 8)], "errors.vmcNameInvalid"),
        (vec![card("a/b", 8)], "errors.vmcNameInvalid"),
        (vec![card("dup", 8), card("DUP", 8)], "errors.vmcTwice"),
    ] {
        let err = plan_cards(&fs, &cards, NOW).unwrap_err();
        assert!(
            matches!(&err, OpError::Invalid(p) if p.key == error),
            "{err}"
        );
    }
}

#[test]
fn a_slot_naming_a_missing_card_is_flagged() {
    let fs = library();
    let mut wanted = settings(&fs);
    wanted.vmc[1] = "generic_1".into();
    let plan = plan_settings(&fs, &game(), &edit(wanted), OPL, NOW).unwrap();
    let notice = &plan.transactions[0].notices[0];
    assert_eq!(notice.code, "vmcNotOnDevice");
    assert!(notice.message.contains("slot 2"), "{}", notice.message);
}

#[test]
fn cards_are_listed_with_what_opl_thinks_of_them() {
    let fs = library();
    fs.insert_file("VMC/zeros.bin", &vec![0u8; 8 << 20]);
    fs.insert_file("VMC/notes.txt", b"not a card");
    let mut used = vmc::blank_card(8, McTime::FALLBACK).unwrap();
    // A save: the root directory lists a third entry.
    let root = vmc::Superblock::parse(&used).root_dir_offset() as usize;
    used[root + 4] = 3;
    fs.insert_file("VMC/Shared.bin", &used);
    fs.insert_file("CFG/SLES_500.51.cfg", b"$VMC_1=shared\r\n");
    fs.insert_file("CFG/SLUS_200.62.cfg", b"$VMC_0=Shared\r\n");

    let cards = list_cards(&fs).unwrap();
    let names: Vec<_> = cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Shared", "zeros"]);
    assert_eq!(cards[0].size_mb, Some(8));
    assert_eq!(cards[0].saves, Some(1));
    assert_eq!(
        cards[0].used_by,
        [("SLES_500.51".to_owned(), 1), ("SLUS_200.62".to_owned(), 0)]
    );
    assert_eq!(cards[1].size_mb, None);
    assert_eq!(cards[1].problem, Some(vmc::VmcProblem::BadMagic));
    assert_eq!(cards[1].saves, None);
}

#[test]
fn ul_games_have_settings_too() {
    let fs = MemFs::new(FsKind::Fat32);
    let record = UlRecord::new("Big Game", "SLUS_212.18", 5, yaoplm_formats::Media::Dvd).unwrap();
    let mut table = UlCfg::default();
    table.records.push(record);
    fs.insert_file("ul.cfg", &table.to_bytes());
    let game = GameRef::Ul {
        index: 0,
        startup: "SLUS_212.18".into(),
        name: "Big Game".into(),
    };
    let wanted = GameSettings {
        compatibility: 0x10,
        ..GameSettings::default()
    };
    let plan = plan_settings(&fs, &game, &edit(wanted), OPL, NOW).unwrap();
    assert_eq!(plan.transactions[0].game.as_ref().unwrap().format, "UL");
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(
        fs.bytes("CFG/SLUS_212.18.cfg").unwrap(),
        b"$Compatibility=16\r\n$ConfigSource=1\r\n"
    );
    assert!(files(&fs).iter().any(|(p, _)| p == "ul.cfg"));
}

// --- Game info written by a plan (META-3, ADR-0050) ------------------------

/// The CFG of a game the bundled table knows (GTA III), with a key OPL
/// never wrote and the title of the file.
fn gta() -> MemFs {
    library()
}

#[test]
fn game_info_is_written_by_a_plan() {
    use yaoplm_core::settings::{InfoWrite, plan_game_info};
    let fs = gta();
    let plan = plan_game_info(&fs, &game(), &InfoWrite::all(), OPL, NOW).unwrap();
    assert_eq!(plan.kind, PlanKind::Settings);
    let tx = &plan.transactions[0];
    assert_eq!(tx.title_phrase.as_ref().unwrap().key, "titles.gameInfo");
    // The file's title is already `Retail`: no change to `Title`.
    let keys: Vec<&str> = tx.cfg_changes.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "Genre",
            "Release",
            "Developer",
            "Publisher",
            "Players",
            "PlayersText"
        ]
    );
    let release = tx.cfg_changes.iter().find(|c| c.key == "Release").unwrap();
    assert_eq!(
        (release.before.as_deref(), release.after.as_deref()),
        (None, Some("10-23-2001"))
    );
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nVmode=vmode/ntsc\r\nGenre=Action\r\nRelease=10-23-2001\r\nDeveloper=DMA Design\r\nPublisher=Rockstar Games\r\nPlayers=players/1\r\nPlayersText=1\r\n"
    );
    assert_eq!(backups(&fs).len(), 1);

    // The same plan again has nothing to write.
    let again = plan_game_info(&fs, &game(), &InfoWrite::all(), OPL, NOW).unwrap();
    assert!(again.transactions.is_empty());
    assert!(again.notices.iter().any(|n| n.code == "gameInfoUnchanged"));
}

#[test]
fn chosen_fields_and_a_shortened_description() {
    use yaoplm_core::settings::{DescriptionText, InfoField, InfoWrite, plan_game_info};
    let fs = gta();
    // Two keys the user already has keep their values when not chosen.
    fs.insert_file(
        "CFG/SLUS_200.62.cfg",
        b"Title=Retail\r\nDeveloper=Someone Else\r\nGenre=Various\r\n",
    );
    let text = "Grand Theft Auto III is a 2001 action-adventure game developed by DMA Design and published by Rockstar Games. It is the first 3D game in the Grand Theft Auto series. Set in Liberty City, loosely based on New York City, the game follows Claude, a silent protagonist who becomes entangled in a world of crime. The game is played from a third-person perspective.";
    assert!(text.len() > 255);
    let write = InfoWrite {
        fields: vec![InfoField::Genre],
        description: Some(DescriptionText {
            text: text.to_owned(),
            source: "https://en.wikipedia.org/wiki/Grand_Theft_Auto_III".to_owned(),
        }),
    };
    let plan = plan_game_info(&fs, &game(), &write, OPL, NOW).unwrap();
    let tx = &plan.transactions[0];
    let change = |key: &str| {
        tx.cfg_changes
            .iter()
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("no change to {key}: {:?}", tx.cfg_changes))
    };
    assert_eq!(change("Genre").before.as_deref(), Some("Various"));
    assert_eq!(change("Genre").after.as_deref(), Some("Action"));
    let description = change("Description").after.clone().unwrap();
    assert!(description.len() <= 255, "{}", description.len());
    assert!(
        description.ends_with("Grand Theft Auto series."),
        "{description}"
    );
    assert_eq!(
        change("DescriptionSource").after.as_deref(),
        Some("https://en.wikipedia.org/wiki/Grand_Theft_Auto_III")
    );
    assert!(tx.cfg_changes.iter().all(|c| c.key != "Developer"));
    let shortened = tx
        .notices
        .iter()
        .find(|n| n.code == "descriptionShortened")
        .unwrap();
    assert_eq!(
        shortened.phrase.as_ref().unwrap().english(),
        format!(
            "The description was shortened to {} bytes, the most OPL shows, at the end of a sentence",
            description.len()
        )
    );
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let bytes = fs.bytes("CFG/SLUS_200.62.cfg").unwrap();
    assert!(
        bytes
            .starts_with(b"Title=Retail\r\nDeveloper=Someone Else\r\nGenre=Action\r\nDescription=")
    );

    // Text without a sentence end is cut on a character boundary.
    let fs = gta();
    let write = InfoWrite {
        fields: Vec::new(),
        description: Some(DescriptionText {
            text: "я".repeat(200),
            source: String::new(),
        }),
    };
    let plan = plan_game_info(&fs, &game(), &write, OPL, NOW).unwrap();
    let description = plan.transactions[0]
        .cfg_changes
        .iter()
        .find(|c| c.key == "Description")
        .unwrap()
        .after
        .clone()
        .unwrap();
    assert!(description.len() <= 255 && description.chars().all(|c| c == 'я' || c == '…'));
    assert!(
        plan.transactions[0]
            .cfg_changes
            .iter()
            .all(|c| c.key != "DescriptionSource")
    );
}

#[test]
fn game_info_is_refused_for_wopl_and_unknown_games() {
    use yaoplm_core::settings::{InfoWrite, plan_game_info};
    let fs = gta();
    fs.insert_file("CFG/SLUS_200.62.cfg", b"title = \"Retail\";\n");
    let err = plan_game_info(&fs, &game(), &InfoWrite::all(), OPL, NOW).unwrap_err();
    assert!(
        matches!(&err, OpError::Invalid(p) if p.key == "errors.cfgNotOpl"),
        "{err:?}"
    );

    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("CD/SLPM_123.45.Long.iso", &Variant::LongId.build().to_vec());
    let unknown = GameRef::Image {
        path: RelPath::new("CD/SLPM_123.45.Long.iso").unwrap(),
    };
    let err = plan_game_info(&fs, &unknown, &InfoWrite::all(), OPL, NOW).unwrap_err();
    assert!(
        matches!(&err, OpError::Invalid(p) if p.key == "errors.noGameInfo"),
        "{err:?}"
    );
}
