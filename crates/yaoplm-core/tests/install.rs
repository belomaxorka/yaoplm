//! Installing loader builds on a `MemFs` device and running the plans
//! (LDR-3, ADR-0042): folders, the ELF with a backup of what it replaces,
//! app folders, language packs, the manifest, starter settings, refused
//! paths, foreign files, the backup rule and a failed write rolled back.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use yaoplm_core::loaders::install::{ElfFile, InstallRequest, InstallTarget, plan_install};
use yaoplm_core::loaders::languages::LanguageFile;
use yaoplm_core::loaders::manifest::{self, Manifest};
use yaoplm_core::loaders::{Build, Target, built_in};
use yaoplm_core::plan::{Level, Plan, Step};
use yaoplm_device::{FsKind, SourceFs};
use yaoplm_test_support::loaders::{opl_elf, plain_elf, sha256};
use yaoplm_test_support::{Content, MemFs, VirtualSources};

const NOW: i64 = 1_761_400_000; // 2025-10-25 13:46:40 UTC

fn build(id: &str) -> &'static Build {
    built_in().unwrap().build(id).unwrap()
}

struct Setup {
    fs: MemFs,
    sources: VirtualSources,
    elf: Vec<u8>,
    file: ElfFile,
}

fn setup(version: &str) -> Setup {
    let elf = opl_elf(version);
    let sources = VirtualSources::new();
    let path = PathBuf::from("store").join(format!("{}.elf", sha256(&elf)));
    sources.add_file(path.clone(), Content::Bytes(elf.clone()));
    let modified = sources.metadata(&path).unwrap().modified;
    Setup {
        fs: MemFs::new(FsKind::Fat32),
        file: ElfFile {
            path,
            sha256: sha256(&elf),
            len: elf.len() as u64,
            modified,
            version: Some(version.to_owned()),
            tag: Some("latest".into()),
            source: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest".into(),
        },
        elf,
        sources,
    }
}

fn preset(build: &Build, id: &str) -> InstallTarget {
    let preset = build.preset(id).unwrap();
    InstallTarget {
        path: preset.path.clone(),
        preset: Some(id.to_owned()),
        title: preset.title.clone(),
    }
}

fn request<'a>(build: &'a Build, s: &Setup, targets: Vec<InstallTarget>) -> InstallRequest<'a> {
    InstallRequest {
        build,
        elf: s.file.clone(),
        targets,
        device: Target::Usb,
        language: None,
        starter_settings: false,
        replace_foreign: false,
        update: false,
        retire: Vec::new(),
        now: NOW,
    }
}

fn errors(plan: &Plan) -> Vec<String> {
    plan.notices
        .iter()
        .filter(|n| n.level == Level::Error)
        .map(|n| n.code.clone())
        .collect()
}

fn run(s: &Setup, plan: &Plan) {
    assert!(errors(plan).is_empty(), "{:?}", plan.notices);
    let report = common::run(plan, &s.fs, &s.sources);
    assert!(report.error.is_none() && !report.cancelled, "{report:?}");
}

#[test]
fn a_fresh_stick_gets_folders_the_elf_and_a_manifest() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    run(&s, &plan);
    assert_eq!(s.fs.bytes("UPDT/OPL.ELF").unwrap(), s.elf);
    for folder in [
        "CD", "DVD", "CFG", "ART", "VMC", "CHT", "APPS", "THM", "LNG",
    ] {
        assert!(s.fs.exists(folder), "{folder}");
    }
    let (manifest, _) = Manifest::read(&s.fs).unwrap();
    let entry = manifest.entry("UPDT/OPL.ELF").unwrap();
    assert_eq!(
        (entry.build.as_str(), entry.device.as_str()),
        ("opl-beta", "usb")
    );
    assert_eq!(entry.sha256, s.file.sha256);
    assert_eq!(entry.version.as_deref(), Some("v1.2.0-Beta-2271-b3a2199"));
    assert!(
        !s.fs.exists(".yaoplm/backup/loader"),
        "nothing was replaced"
    );
    // No `.yaoplm-part` left behind.
    assert!(
        s.fs.files()
            .iter()
            .all(|(p, _)| !p.ends_with(".yaoplm-part"))
    );
}

#[test]
fn i_dont_know_puts_it_where_both_stock_launchers_look() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    let beta = build("opl-beta");
    let plan = plan_install(
        &s.fs,
        &request(beta, &s, vec![preset(beta, "fmcb"), preset(beta, "ps2bbl")]),
    )
    .unwrap();
    run(&s, &plan);
    assert_eq!(s.fs.bytes("UPDT/OPL.ELF").unwrap(), s.elf);
    assert_eq!(s.fs.bytes("APPS/OPNPS2LD.ELF").unwrap(), s.elf);
    assert_eq!(Manifest::read(&s.fs).unwrap().0.loaders.len(), 2);
}

#[test]
fn another_opl_is_replaced_and_kept_as_a_backup() {
    let s = setup("v1.2.0-Beta-2280-0123abc");
    let old = opl_elf("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file("UPDT/OPL.ELF", &old);
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    run(&s, &plan);
    assert_eq!(s.fs.bytes("UPDT/OPL.ELF").unwrap(), s.elf);
    let backup = ".yaoplm/backup/loader/20251025-134640/UPDT_OPL.ELF";
    assert_eq!(s.fs.bytes(backup).unwrap(), old);
}

#[test]
fn the_same_elf_is_left_alone() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file("UPDT/OPL.ELF", &s.elf);
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    let writes: Vec<&Step> = plan
        .transactions
        .iter()
        .flat_map(|tx| &tx.steps)
        .filter(|step| matches!(step, Step::WriteFile { .. } | Step::Rename { .. }))
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
    let codes: Vec<&str> = plan
        .transactions
        .iter()
        .flat_map(|tx| &tx.notices)
        .map(|n| n.code.as_str())
        .collect();
    assert!(codes.contains(&"loaderAlreadyThere"));
    run(&s, &plan);
    // The manifest still learns of it.
    assert!(
        Manifest::read(&s.fs)
            .unwrap()
            .0
            .entry("UPDT/OPL.ELF")
            .is_some()
    );
}

#[test]
fn a_foreign_file_needs_the_users_word() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    // wLaunchELF where PS2BBL looks for OPL: a PS2 program without OPL's files.
    let foreign = plain_elf(&["LAUNCHELF.CNF", "wLaunchELF 4.43x_isr"]);
    s.fs.insert_file("APPS/OPNPS2LD.ELF", &foreign);
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "ps2bbl")])).unwrap();
    assert_eq!(errors(&plan), ["loaderForeignFile"]);
    let notice = plan
        .notices
        .iter()
        .find(|n| n.code == "loaderForeignFile")
        .unwrap();
    assert!(
        notice.message.contains(&sha256(&foreign)),
        "{}",
        notice.message
    );

    let mut confirmed = request(beta, &s, vec![preset(beta, "ps2bbl")]);
    confirmed.replace_foreign = true;
    let plan = plan_install(&s.fs, &confirmed).unwrap();
    run(&s, &plan);
    assert_eq!(s.fs.bytes("APPS/OPNPS2LD.ELF").unwrap(), s.elf);
    assert_eq!(
        s.fs.bytes(".yaoplm/backup/loader/20251025-134640/APPS_OPNPS2LD.ELF")
            .unwrap(),
        foreign
    );
    // RiptOPL at the same place is another family too.
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file(
        "UPDT/OPL.ELF",
        &plain_elf(&[
            "settings_riptopl.cfg",
            "conf_opl.cfg",
            "v1.2.0-Beta-4257-55261de",
        ]),
    );
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    assert_eq!(errors(&plan), ["loaderForeignFile"]);
}

#[test]
fn three_backups_a_path_are_kept() {
    let s = setup("v1.2.0-Beta-2280-0123abc");
    s.fs.insert_file("UPDT/OPL.ELF", &opl_elf("v1.2.0-Beta-2279-0000003"));
    for (stamp, n) in [
        ("20251001-000000", 1),
        ("20251002-000000", 2),
        ("20251003-000000", 3),
    ] {
        s.fs.insert_file(
            &format!(".yaoplm/backup/loader/{stamp}/UPDT_OPL.ELF"),
            &opl_elf(&format!("v1.2.0-Beta-227{n}-000000{n}")),
        );
    }
    // Another path's backup is not this path's.
    s.fs.insert_file(
        ".yaoplm/backup/loader/20251001-000000/APPS_OPNPS2LD.ELF",
        b"x",
    );
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    run(&s, &plan);
    let kept: Vec<String> =
        s.fs.files()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| p.ends_with("UPDT_OPL.ELF"))
            .collect();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(!kept.iter().any(|p| p.contains("20251001")));
    assert!(s.fs.exists(".yaoplm/backup/loader/20251001-000000/APPS_OPNPS2LD.ELF"));
    assert!(
        !s.fs
            .exists(".yaoplm/backup/loader/20251002-000000/APPS_OPNPS2LD.ELF")
    );
}

#[test]
fn an_app_folder_gets_its_title_cfg_and_keeps_other_keys() {
    let s = setup("v1.2.0-Beta-2370-33a2b19-yaopl");
    let yaopl = build("yaopl");
    let mut req = request(yaopl, &s, vec![preset(yaopl, "app")]);
    req.device = Target::Mx4sio;
    run(&s, &plan_install(&s.fs, &req).unwrap());
    assert_eq!(s.fs.bytes("APPS/YAOPL/OPNPS2LD.ELF").unwrap(), s.elf);
    let cfg = String::from_utf8(s.fs.bytes("APPS/YAOPL/title.cfg").unwrap()).unwrap();
    assert!(
        cfg.contains("title=YAOPL") && cfg.contains("boot=OPNPS2LD.ELF"),
        "{cfg}"
    );
    assert_eq!(Manifest::read(&s.fs).unwrap().0.loaders[0].device, "mx4sio");

    let s = setup("v1.2.0-Beta-2370-33a2b19-yaopl");
    s.fs.insert_file("APPS/YAOPL/title.cfg", b"title=Old\r\nDescription=Mine\r\n");
    run(
        &s,
        &plan_install(&s.fs, &request(yaopl, &s, vec![preset(yaopl, "app")])).unwrap(),
    );
    let cfg = String::from_utf8(s.fs.bytes("APPS/YAOPL/title.cfg").unwrap()).unwrap();
    assert!(
        cfg.contains("title=YAOPL") && cfg.contains("Description=Mine"),
        "{cfg}"
    );
}

#[test]
fn reserved_and_odd_paths_are_refused() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    let beta = build("opl-beta");
    let target = |path: &str| InstallTarget {
        path: path.to_owned(),
        preset: None,
        title: None,
    };
    for (path, code) in [
        ("MemoryCards/PS2/OPL.ELF", "loaderPathReserved"),
        (".sd2psx/OPL.ELF", "loaderPathReserved"),
        ("os/OPL.ELF", "loaderPathReserved"),
        (".yaoplm/OPL.ELF", "loaderPathReserved"),
        ("APPS/OPL.BIN", "loaderPathInvalid"),
        ("../OPL.ELF", "loaderPathInvalid"),
        ("", "loaderPathInvalid"),
    ] {
        let plan = plan_install(&s.fs, &request(beta, &s, vec![target(path)])).unwrap();
        assert_eq!(errors(&plan), [code], "{path}");
    }
    s.fs.insert_dir("UPDT/OPL.ELF");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    assert_eq!(errors(&plan), ["loaderPathIsFolder"]);
    let plan = plan_install(&s.fs, &request(beta, &s, vec![])).unwrap();
    assert_eq!(errors(&plan), ["loaderNoPath"]);
    // A memory card emulator's SD gets no loader at all (ADR-0048): on
    // main the plan put OPL into `UPDT/` next to `MemoryCards/`.
    s.fs.insert_file("MemoryCards/PS2/BOOT/BootCard-1.mcd", b"card");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    assert_eq!(errors(&plan), ["loaderOnEmulatorSd"]);
    assert!(plan.transactions.is_empty() && !plan.is_executable());
}

#[test]
fn opl_1_1_0_is_not_put_on_exfat() {
    // Replacing already refused it (`replaceExfatOpl110`); on main a plain
    // install planned it, and the device card showed the conflict after.
    let mut s = setup("1.1.0");
    s.fs = MemFs::new(FsKind::ExFat);
    let stable = build("opl-stable");
    let plan = plan_install(&s.fs, &request(stable, &s, vec![preset(stable, "fmcb")])).unwrap();
    assert_eq!(errors(&plan), ["loaderExfatOpl110"]);
    assert!(plan.transactions.is_empty() && !plan.is_executable());
    // Every other build reads exFAT.
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    assert!(errors(&plan).is_empty(), "{:?}", plan.notices);
    // Over SMB OPL never sees the computer's file system (ADR-0047).
    s.fs = MemFs::new(FsKind::ExFat).with_smb(true);
    let plan = plan_install(&s.fs, &request(stable, &s, vec![preset(stable, "fmcb")])).unwrap();
    assert!(errors(&plan).is_empty(), "{:?}", plan.notices);
}

#[test]
fn a_language_pack_and_starter_settings() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file("LNG/lang_Russian.lng", b"old");
    s.fs.insert_file("conf_opl.cfg", b"theme=dark\r\nusb_mode=0\r\nmy_key=1\r\n");
    let beta = build("opl-beta");
    let mut req = request(beta, &s, vec![preset(beta, "fmcb")]);
    req.language = Some((
        "Russian".into(),
        vec![
            LanguageFile {
                name: "lang_Russian.lng".into(),
                bytes: b"ru".to_vec(),
            },
            LanguageFile {
                name: "font_Russian.ttf".into(),
                bytes: b"font".to_vec(),
            },
        ],
    ));
    req.starter_settings = true;
    run(&s, &plan_install(&s.fs, &req).unwrap());
    assert_eq!(s.fs.bytes("LNG/lang_Russian.lng").unwrap(), b"ru");
    assert_eq!(s.fs.bytes("LNG/font_Russian.ttf").unwrap(), b"font");
    let cfg = String::from_utf8(s.fs.bytes("conf_opl.cfg").unwrap()).unwrap();
    assert!(
        cfg.starts_with("theme=dark\r\nusb_mode=2\r\nmy_key=1\r\n"),
        "{cfg}"
    );
    assert!(cfg.contains("language_text=Russian"), "{cfg}");
    assert_eq!(
        Manifest::read(&s.fs).unwrap().0.loaders[0]
            .language
            .as_deref(),
        Some("Russian")
    );

    // MX4SIO: OPL would not see the file before MX4SIO is on.
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    let mut req = request(beta, &s, vec![preset(beta, "app")]);
    req.device = Target::Mx4sio;
    req.starter_settings = true;
    let plan = plan_install(&s.fs, &req).unwrap();
    assert!(
        plan.notices
            .iter()
            .any(|n| n.code == "starterSettingsMx4sio")
    );
    run(&s, &plan);
    assert!(!s.fs.exists("conf_opl.cfg"));
}

#[test]
fn a_broken_manifest_is_replaced_as_it_was_read() {
    let s = setup("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file(manifest::PATH, b"{ not json");
    let beta = build("opl-beta");
    run(
        &s,
        &plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap(),
    );
    assert_eq!(Manifest::read(&s.fs).unwrap().0.loaders.len(), 1);
}

#[test]
fn a_failed_write_puts_the_old_loader_back() {
    let s = setup("v1.2.0-Beta-2280-0123abc");
    let old = opl_elf("v1.2.0-Beta-2271-b3a2199");
    s.fs.insert_file("UPDT/OPL.ELF", &old);
    let beta = build("opl-beta");
    let plan = plan_install(&s.fs, &request(beta, &s, vec![preset(beta, "fmcb")])).unwrap();
    s.fs.unplug_after(16);
    let report = common::run(&plan, &s.fs, &s.sources);
    assert!(report.error.is_some());
    s.fs.replug();
    // Rolled back: the old ELF is where it was, no manifest names the new one.
    assert_eq!(s.fs.bytes("UPDT/OPL.ELF").unwrap(), old);
    assert!(Manifest::read(&s.fs).is_none_or(|(m, _)| m.entry("UPDT/OPL.ELF").is_none()));
}
