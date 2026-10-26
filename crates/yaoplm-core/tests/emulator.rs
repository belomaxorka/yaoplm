//! A memory card emulator's SD on a `MemFs` device (DEV-2, ADR-0048): what
//! the device card reads from it, what its files tell the doctor, and that
//! no plan touches its service files or puts a loader on it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use yaoplm_core::emulator::{BootChannel, guard, refusal, survey};
use yaoplm_core::exec::ExecError;
use yaoplm_core::loaders::built_in;
use yaoplm_core::loaders::rollback::plan_rollback;
use yaoplm_core::ops::plan_init;
use yaoplm_core::plan::{Level, Plan, PlanKind, Step, Transaction};
use yaoplm_core::{LoaderProfile, Phrase, detect, fragments};
use yaoplm_device::{Advice, DeviceFs, FsKind, RelPath};
use yaoplm_test_support::{MemFs, VirtualSources, emulator};

fn rel(path: &str) -> RelPath {
    RelPath::new(path).unwrap()
}

fn codes(advice: &[Advice]) -> Vec<&'static str> {
    advice.iter().map(Advice::code).collect()
}

fn errors(plan: &Plan) -> Vec<String> {
    plan.notices
        .iter()
        .filter(|n| n.level == Level::Error)
        .map(|n| n.code.clone())
        .collect()
}

#[test]
fn the_card_reads_channels_cards_and_settings() {
    let fs = emulator::memfs();
    let sd = fs.emulator().unwrap().unwrap();
    assert_eq!(sd.signs, [".sd2psx", "MemoryCards/PS2", "MemoryCards/PS1"]);
    let found = survey(&fs).unwrap();
    let names: Vec<&str> = found.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["PS2", "PS1"]);
    let ps2 = &found.variants[0];
    assert_eq!(
        ps2.boot,
        [
            BootChannel {
                channel: 1,
                name: Some("FMCB 1.966".into()),
                file: "MemoryCards/PS2/BOOT/BootCard-1.mcd".into(),
                bytes: 16 * 1024,
            },
            BootChannel {
                channel: 2,
                name: Some("PS2BBL".into()),
                file: "MemoryCards/PS2/BOOT/BootCard-2.mcd".into(),
                bytes: 16 * 1024,
            },
        ]
    );
    assert_eq!((ps2.max_channels, ps2.card_size), (Some(2), Some(8)));
    assert_eq!((ps2.cards, ps2.game_cards), (1, 1));
    assert_eq!(found.variants[1].boot[0].name.as_deref(), Some("Unirom"));
    let settings = found.settings.as_ref().unwrap();
    assert_eq!(settings.get("PS2", "Autoboot"), Some("ON"));
    // The factory card makes 64 MiB cards: more than the README's 8.
    assert_eq!(codes(&found.advice), ["emulatorCardSize"]);
    assert!(found.damaged.is_empty());
}

#[test]
fn damaged_cards_unread_lines_and_legacy_names() {
    let fs = MemFs::new(FsKind::ExFat);
    // The original sd2psx: settings in flash, no `.sd2psx/`, one legacy
    // boot card.
    fs.insert_file("MemoryCards/PS2/BOOT/BootCard.mcd", &[0xFF; 512]);
    fs.insert_file(
        "MemoryCards/PS2/BOOT/BootCard.ini",
        b"[ChannelName]\n1=Legacy\n2=No newline",
    );
    fs.insert_file("MemoryCards/PS2/Card3/Card3-1.crp.0", b"x");
    fs.insert_file("MemoryCards/PS2/Card3/Card3-1.crp.1", b"x");
    fs.insert_file("MemoryCards/PS2/SLUS-20062/SLUS-20062-2.crp.0", b"x");
    fs.insert_file("MemoryCards/PS2/Card4/Card4-1.crp.0", b"x");
    let sd = fs.emulator().unwrap().unwrap();
    assert_eq!(sd.signs, ["MemoryCards/PS2"]);
    let found = survey(&fs).unwrap();
    let ps2 = &found.variants[0];
    assert_eq!(ps2.boot.len(), 1);
    assert_eq!(ps2.boot[0].file, "MemoryCards/PS2/BOOT/BootCard.mcd");
    assert_eq!(ps2.boot[0].name.as_deref(), Some("Legacy"));
    assert_eq!((ps2.cards, ps2.game_cards), (2, 1));
    assert_eq!(found.settings, None);
    assert_eq!(found.damaged.len(), 4);
    assert_eq!(codes(&found.advice), ["emulatorIniTail", "emulatorDamaged"]);
    let english: Vec<String> = found.advice.iter().map(|a| a.phrase().english()).collect();
    assert!(
        english[0].contains("BootCard.ini") && english[0].contains("2=No newline"),
        "{english:?}"
    );
    assert!(
        english[1].contains("(4)") && english[1].ends_with(", …"),
        "{english:?}"
    );
    // `BootCard-1.mcd` wins over the legacy name.
    fs.insert_file("MemoryCards/PS2/BOOT/BootCard-1.mcd", &[0xFF; 1024]);
    let boot = survey(&fs).unwrap().variants.remove(0).boot;
    assert_eq!((boot.len(), boot[0].bytes), (1, 1024));
}

#[test]
fn opl_s_folders_go_next_to_the_emulator_s_files() {
    let fs = emulator::memfs();
    let plan = plan_init(&fs).unwrap();
    assert!(
        errors(&plan).is_empty() && plan.is_executable(),
        "{:?}",
        plan.notices
    );
    let report = common::run(&plan, &fs, &VirtualSources::new());
    assert!(report.error.is_none(), "{report:?}");
    for folder in [
        "CD", "DVD", "CFG", "ART", "VMC", "CHT", "APPS", "THM", "LNG",
    ] {
        assert!(fs.exists(folder), "{folder}");
    }
    let before = emulator::files();
    for (path, bytes) in before {
        assert_eq!(fs.bytes(&path).unwrap(), bytes, "{path}");
    }
}

fn touching(path: &str) -> Plan {
    let mut plan = Plan::titled(
        PlanKind::Vmc,
        Phrase::title("createFolders"),
        "MEM-0001".into(),
        64 << 30,
    );
    let mut tx = Transaction::titled(Phrase::title("createFolders"));
    tx.push(Step::CreateDir { path: rel(path) });
    plan.transactions.push(tx);
    plan
}

#[test]
fn no_plan_touches_the_emulator_s_files() {
    let fs = emulator::memfs();
    let sd = fs.emulator().unwrap().unwrap();
    for path in [
        "MemoryCards/PS2/Card2",
        ".sd2psx/backup",
        "memorycards/PS1/Card9",
    ] {
        let mut plan = touching(path);
        assert_eq!(
            refusal(&plan, &sd).map(|n| n.code),
            Some("emulatorServiceFile".into()),
            "{path}"
        );
        guard(&mut plan, &sd);
        guard(&mut plan, &sd);
        assert_eq!(errors(&plan), ["emulatorServiceFile"]);
        assert!(
            plan.notices[0].message.contains(path),
            "{}",
            plan.notices[0].message
        );
    }
    // A plan made without the guard is still stopped before its first write.
    let report = common::run(
        &touching("MemoryCards/PS2/Card2"),
        &fs,
        &VirtualSources::new(),
    );
    match report.error {
        Some(ExecError::Blocked(notice)) => assert_eq!(notice.code, "emulatorServiceFile"),
        other => panic!("{other:?}"),
    }
    assert!(!fs.exists("MemoryCards/PS2/Card2"));
    assert!(
        !fs.exists(".yaoplm/journal"),
        "no journal for a blocked plan"
    );
    // Everything else goes ahead.
    let mut plan = touching("CD");
    guard(&mut plan, &sd);
    assert!(errors(&plan).is_empty());
    let report = common::run(&plan, &fs, &VirtualSources::new());
    assert!(report.error.is_none(), "{report:?}");
    // Renames count at both ends.
    let mut plan = touching("CD/x");
    plan.transactions[0].push(Step::Rename {
        from: rel("CD/x"),
        to: rel("MemoryCards/x"),
    });
    assert!(refusal(&plan, &sd).is_some());
}

#[test]
fn no_loader_is_rolled_back_on_an_emulator_s_sd() {
    let fs = emulator::memfs();
    fs.insert_file("UPDT/OPL.ELF", b"elf");
    fs.insert_file(".yaoplm/backup/loader/20261025-120000/UPDT_OPL.ELF", b"old");
    let plan = plan_rollback(&fs, "UPDT/OPL.ELF", built_in().unwrap(), 1_761_400_000).unwrap();
    assert_eq!(errors(&plan), ["loaderOnEmulatorSd"]);
    assert!(plan.transactions.is_empty());
}

/// The owner's factory SD, from its verified copy (research/19 §2):
/// `$YAOPLM_TESTDATA\backups\psxmemcard-gen2-sd-2026-10-26\files`. Reads
/// only; nothing is ever written there.
#[test]
#[ignore = "needs YAOPLM_TESTDATA with the factory SD's copy"]
fn the_owner_s_factory_sd() {
    let Some(data) = std::env::var_os("YAOPLM_TESTDATA") else {
        panic!("set YAOPLM_TESTDATA");
    };
    let copy = std::path::Path::new(&data)
        .join("backups")
        .join("psxmemcard-gen2-sd-2026-10-26")
        .join("files");
    let fs = yaoplm_device::HostFs::new(&copy).unwrap();
    let sd = fs.emulator().unwrap().unwrap();
    assert_eq!(
        sd.signs,
        [
            ".sd2psx",
            "MemoryCards/PS2",
            "MemoryCards/PS1",
            "MemoryCards/COH"
        ]
    );
    assert!(sd.sd2psx && !sd.mcp2 && sd.at.is_empty());
    let found = survey(&fs).unwrap();
    let names = |variant: usize| -> Vec<(u32, String)> {
        found.variants[variant]
            .boot
            .iter()
            .map(|b| (b.channel, b.name.clone().unwrap_or_default()))
            .collect()
    };
    // The names BootCard.ini gives; for channels 3–6 they disagree with
    // the images (research/19 §4), which only reading the cards can tell.
    assert_eq!(
        names(0),
        [
            (1, "FMCB 1.966".into()),
            (2, "FunTuna".into()),
            (3, "FMCB 1.965".into()),
            (4, "FMCB 1.963".into()),
            (5, "FMCB 1.953".into()),
            (6, "PS2BBL".into()),
        ]
    );
    assert!(found.variants[0].boot.iter().all(|b| b.bytes == 8 << 20));
    assert_eq!(
        (found.variants[0].cards, found.variants[0].game_cards),
        (3, 0)
    );
    assert_eq!(names(1), [(1, "Unirom".into())]);
    assert_eq!(found.variants[1].cards, 2);
    assert_eq!(names(2), [(1, "wLaunchELF 4.43x_isr_coh".into())]);
    assert_eq!(found.variants[2].boot[0].bytes, 1 << 20);
    let settings = found.settings.as_ref().unwrap();
    assert_eq!(settings.get("PS2", "Autoboot"), Some("ON"));
    assert_eq!(settings.get("PS1", "Autoboot"), Some("OFF"));
    assert_eq!(codes(&found.advice), ["emulatorCardSize"]);
    assert!(found.damaged.is_empty());
    // OPL's folders may go there; the emulator's files may not be touched.
    let mut plan = plan_init(&fs).unwrap();
    guard(&mut plan, &sd);
    assert!(errors(&plan).is_empty() && plan.is_executable());
    let mut plan = touching("MemoryCards/PS2/BOOT/BootCard-7.mcd");
    guard(&mut plan, &sd);
    assert_eq!(errors(&plan), ["emulatorServiceFile"]);
}

#[test]
fn exfat_is_no_sign_and_fragments_do_not_count_through_the_emulator() {
    // The factory card is exFAT: through MMCE that says nothing of the
    // loader, so the profile stays the default and the user is asked.
    let fs = emulator::memfs().with_mmce(true);
    let detection = detect::detect_profile(&fs).unwrap();
    assert_eq!(detection.profile, LoaderProfile::Opl120Beta);
    assert_eq!(detection.confidence, detect::Confidence::Default);
    assert!(!fragments::applies(&fs));
    // Plugged in as a stick, the same exFAT rules out OPL 1.1.0.
    let stick = emulator::memfs();
    assert_eq!(
        detect::detect_profile(&stick).unwrap().confidence,
        detect::Confidence::Indirect
    );
    assert!(fragments::applies(&stick));
}

/// What the factory SD's boot cards hold, from their own file systems
/// (research/19 §4); reads only.
#[test]
#[ignore = "needs YAOPLM_TESTDATA with the factory SD's copy"]
fn the_factory_boot_cards_by_their_content() {
    use yaoplm_core::boot_card::{Launcher, channel_advice, contents};
    let data = std::env::var_os("YAOPLM_TESTDATA").expect("set YAOPLM_TESTDATA");
    let copy = std::path::Path::new(&data)
        .join("backups")
        .join("psxmemcard-gen2-sd-2026-10-26")
        .join("files");
    let fs = yaoplm_device::HostFs::new(&copy).unwrap();
    let found = survey(&fs).unwrap();
    let mut seen = Vec::new();
    for variant in &found.variants {
        for boot in &variant.boot {
            let held = contents(&fs, &rel(&boot.file), boot.bytes).unwrap();
            assert_eq!(held.unreadable, None, "{}", boot.file);
            let loaders: Vec<String> = held
                .loaders
                .iter()
                .map(|l| {
                    format!(
                        "{} {} mmce={}",
                        l.path,
                        l.version.as_deref().unwrap_or("?"),
                        l.mmce
                    )
                })
                .collect();
            println!(
                "{} {} {:?}: {:?} builder={:?} {loaders:?}",
                variant.name,
                boot.channel,
                boot.name,
                held.launchers,
                held.builder.as_ref().map(|b| b.file)
            );
            let warning = channel_advice(&variant.name, boot.channel, boot.name.as_deref(), &held)
                .map(|a| a.phrase().english());
            seen.push((variant.name.clone(), boot.channel, held, warning));
        }
    }
    let at = |variant: &str, channel: u32| {
        seen.iter()
            .find(|(v, c, _, _)| v == variant && *c == channel)
            .unwrap()
    };
    // Channel 1: the factory's own FMCB 1.966 with three OPL builds.
    let (_, _, one, warning) = at("PS2", 1);
    assert_eq!(
        (one.launchers.as_slice(), one.builder.as_ref()),
        (&[Launcher::FreeMcBoot][..], None)
    );
    assert_eq!(*warning, None);
    let version = |path: &str| {
        one.loaders
            .iter()
            .find(|l| l.path == path)
            .map(|l| (l.version.clone(), l.mmce))
    };
    assert_eq!(
        version("APPS/OPNPS2LD.ELF"),
        Some((Some("v1.2.0-Beta-2049-513e9b3".into()), false))
    );
    assert_eq!(
        version("APPS/OPL-MMCE-BETA2.ELF"),
        Some((Some("v1.2.0-Beta-2216-bb415d2".into()), true))
    );
    // Channel 2: FunTuna, not an SD Builder image.
    assert_eq!(at("PS2", 2).2.launchers, [Launcher::FunTuna]);
    assert_eq!(at("PS2", 2).3, None);
    // Channels 3–6: SD Builder images under the wrong names.
    for (channel, file, holds) in [
        (3, "FMCB-1963.mcd", "FMCB 1.963"),
        (4, "FMCB-1953.mcd", "FMCB 1.953"),
        (5, "PS2BBL.mcd", "PS2BBL"),
        (6, "FMCB-1965.mcd", "FMCB 1.965"),
    ] {
        let (_, _, held, warning) = at("PS2", channel);
        assert_eq!(
            held.builder.as_ref().map(|b| b.file),
            Some(file),
            "{channel}"
        );
        assert!(
            warning.as_deref().is_some_and(|w| w.contains(holds)),
            "{channel}: {warning:?}"
        );
    }
    // Channel 5's swapped names: the MMCE build is the one called OPNPS2LD.
    let five = &at("PS2", 5).2;
    assert_eq!(five.launchers, [Launcher::Ps2bbl]);
    let mmce: Vec<&str> = five
        .loaders
        .iter()
        .filter(|l| l.mmce)
        .map(|l| l.path.as_str())
        .collect();
    assert_eq!(mmce, ["APPS/OPNPS2LD.ELF"]);
    // PS1: FreePSXBoot, matching none of SD Builder's images; COH: SD
    // Builder's wLaunchELF.
    assert_eq!(at("PS1", 1).2.launchers, [Launcher::FreePsxBoot]);
    assert_eq!(at("PS1", 1).2.builder, None);
    assert_eq!(
        at("COH", 1).2.builder.as_ref().map(|b| b.names[0]),
        Some("wLE COH")
    );
    // Its own name differs from SD Builder's, and that is no warning.
    assert_eq!(at("COH", 1).3, None);
    assert_eq!(at("PS1", 1).3, None);
}
