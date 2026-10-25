//! What is installed, updates and rollbacks on a `MemFs` device (LDR-4,
//! ADR-0045): the manifest counts only while its hash is the file's, an
//! update replaces only the ELF (and its language pack) with a backup, and
//! a rollback swaps the newest backup back so that a second one undoes it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use yaoplm_core::detect::detect_profile;
use yaoplm_core::loaders::install::{ElfFile, InstallRequest, InstallTarget, plan_install};
use yaoplm_core::loaders::installed::{Freshness, Installed, KnownBy, freshness, installed};
use yaoplm_core::loaders::languages::LanguageFile;
use yaoplm_core::loaders::manifest::Manifest;
use yaoplm_core::loaders::rollback::plan_rollback;
use yaoplm_core::loaders::{Build, Catalog, Target, built_in};
use yaoplm_core::plan::{Level, Plan, PlanKind};
use yaoplm_device::{DeviceFs, FsKind, RelPath, SourceFs};
use yaoplm_test_support::loaders::{opl_elf, plain_elf, sha256};
use yaoplm_test_support::{Content, MemFs, VirtualSources};

const NOW: i64 = 1_761_400_000; // 2025-10-25 13:46:40 UTC
const OLD: &str = "v1.2.0-Beta-2271-b3a2199";
const NEW: &str = "v1.2.0-Beta-2280-0123abc";

fn catalog() -> &'static Catalog {
    built_in().unwrap()
}

fn build(id: &str) -> &'static Build {
    catalog().build(id).unwrap()
}

/// A device and the store the ELFs come from.
struct Device {
    fs: MemFs,
    sources: VirtualSources,
}

impl Device {
    fn new() -> Self {
        Self {
            fs: MemFs::new(FsKind::Fat32),
            sources: VirtualSources::new(),
        }
    }

    /// An ELF in the store, ready to install.
    fn elf(&self, bytes: &[u8], version: Option<&str>) -> ElfFile {
        let path = PathBuf::from("store").join(format!("{}.elf", sha256(bytes)));
        self.sources
            .add_file(path.clone(), Content::Bytes(bytes.to_vec()));
        let modified = self.sources.metadata(&path).unwrap().modified;
        ElfFile {
            path,
            sha256: sha256(bytes),
            len: bytes.len() as u64,
            modified,
            version: version.map(str::to_owned),
            tag: Some("latest".into()),
            source: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest".into(),
        }
    }

    fn run(&self, plan: &Plan) {
        let errors: Vec<_> = plan
            .notices
            .iter()
            .filter(|n| n.level == Level::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        let report = common::run(plan, &self.fs, &self.sources);
        assert!(report.error.is_none() && !report.cancelled, "{report:?}");
    }

    fn installed(&self) -> Vec<Installed> {
        installed(&self.fs, &detect_profile(&self.fs).unwrap(), catalog()).unwrap()
    }

    fn install(&self, request: &InstallRequest<'_>) {
        self.run(&plan_install(&self.fs, request).unwrap());
    }
}

fn russian() -> (String, Vec<LanguageFile>) {
    (
        "Russian".into(),
        vec![LanguageFile {
            name: "lang_Russian.lng".into(),
            bytes: b"ru".to_vec(),
        }],
    )
}

fn request<'a>(
    build: &'a Build,
    elf: ElfFile,
    path: &str,
    preset: Option<&str>,
    now: i64,
) -> InstallRequest<'a> {
    InstallRequest {
        build,
        elf,
        targets: vec![InstallTarget {
            path: path.to_owned(),
            preset: preset.map(str::to_owned),
            title: None,
        }],
        device: Target::Usb,
        language: None,
        starter_settings: false,
        replace_foreign: false,
        update: false,
        now,
    }
}

#[test]
fn the_manifest_names_the_build_only_while_its_hash_is_the_files() {
    let d = Device::new();
    let beta = build("opl-beta");
    let old = opl_elf(OLD);
    d.install(&request(
        beta,
        d.elf(&old, Some(OLD)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW,
    ));
    let found = d.installed();
    assert_eq!(found.len(), 1);
    let loader = &found[0];
    assert_eq!(loader.path, "UPDT/OPL.ELF");
    assert_eq!(loader.build.as_deref(), Some("opl-beta"));
    assert_eq!(loader.known_by, KnownBy::Manifest);
    assert_eq!(loader.version.as_deref(), Some(OLD));
    assert_eq!(loader.sha256, sha256(&old));
    assert!(!loader.manifest_stale);
    assert_eq!(
        loader.entry.as_ref().and_then(|e| e.preset.as_deref()),
        Some("fmcb")
    );
    assert_eq!(loader.backup, None);

    // Replaced by hand with YAOPL: the file decides, the manifest is stale.
    d.fs.insert_file("UPDT/OPL.ELF", &opl_elf("v1.2.0-Beta-2370-33a2b19-yaopl"));
    let loader = &d.installed()[0];
    assert_eq!(loader.build.as_deref(), Some("yaopl"));
    assert_eq!(loader.known_by, KnownBy::Version);
    assert!(loader.manifest_stale);
    assert_eq!(loader.entry, None);

    // An OPL that says no version: the family, not the build.
    d.fs.insert_file("UPDT/OPL.ELF", &plain_elf(&["conf_opl.cfg"]));
    let loader = &d.installed()[0];
    assert_eq!(loader.build, None);
    assert_eq!(loader.known_by, KnownBy::Name);
}

#[test]
fn a_path_of_the_users_own_is_known_from_the_manifest() {
    let d = Device::new();
    let beta = build("opl-beta");
    let elf = opl_elf(OLD);
    d.install(&request(
        beta,
        d.elf(&elf, Some(OLD)),
        "APPS/MINE/BOOT.ELF",
        None,
        NOW,
    ));
    let found = d.installed();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, "APPS/MINE/BOOT.ELF");
    assert_eq!(found[0].known_by, KnownBy::Manifest);
    assert_eq!(found[0].version.as_deref(), Some(OLD));
    // Another file there: no longer ours, and the detection does not look.
    d.fs.insert_file("APPS/MINE/BOOT.ELF", b"\x7fELF other");
    assert!(d.installed().is_empty());
}

#[test]
fn freshness_orders_builds_inside_a_channel_only() {
    assert_eq!(
        freshness(Some(OLD), Some(NEW)),
        Freshness::Update(NEW.to_owned())
    );
    assert_eq!(freshness(Some(NEW), Some(NEW)), Freshness::Current);
    assert_eq!(freshness(Some(NEW), Some(OLD)), Freshness::Current);
    assert_eq!(freshness(Some(OLD), None), Freshness::Unknown);
    assert_eq!(freshness(None, Some(NEW)), Freshness::Unknown);
    // The same number from another commit is another build, not an update.
    assert_eq!(
        freshness(Some(OLD), Some("v1.2.0-Beta-2271-1234567")),
        Freshness::Unknown
    );
    // Numbers of a fork count its own commits.
    assert_eq!(
        freshness(Some(OLD), Some("v1.2.0-Beta-2370-33a2b19-yaopl")),
        Freshness::Unknown
    );
    assert_eq!(
        freshness(Some("v1.1.0"), Some("v1.1.0")),
        Freshness::Current
    );
}

#[test]
fn an_update_replaces_only_the_elf_and_its_language_with_a_backup() {
    let d = Device::new();
    let beta = build("opl-beta");
    let old = opl_elf(OLD);
    let mut first = request(
        beta,
        d.elf(&old, Some(OLD)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW,
    );
    first.language = Some(russian());
    d.install(&first);
    // The user removed a folder and wrote their own settings since.
    d.fs.remove_dir(&RelPath::new("CHT").unwrap()).unwrap();
    d.fs.insert_file("conf_opl.cfg", b"usb_mode=1\r\n");

    let new = opl_elf(NEW);
    let mut update = request(
        beta,
        d.elf(&new, Some(NEW)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW + 3600,
    );
    update.update = true;
    update.starter_settings = true;
    update.language = Some((
        "Russian".into(),
        vec![LanguageFile {
            name: "lang_Russian.lng".into(),
            bytes: b"ru v2".to_vec(),
        }],
    ));
    let plan = plan_install(&d.fs, &update).unwrap();
    assert_eq!(plan.kind, PlanKind::UpdateLoader);
    assert_eq!(
        plan.title,
        "Update OPL 1.2.0 beta to v1.2.0-Beta-2280-0123abc"
    );
    let titles: Vec<&str> = plan.transactions.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Put OPL 1.2.0 beta in UPDT/OPL.ELF",
            "OPL menu language: Russian",
            "Remember the installed loader"
        ],
        "no folders, no settings"
    );
    d.run(&plan);
    assert_eq!(d.fs.bytes("UPDT/OPL.ELF").unwrap(), new);
    assert_eq!(d.fs.bytes("LNG/lang_Russian.lng").unwrap(), b"ru v2");
    assert!(!d.fs.exists("CHT"), "folders are not recreated");
    assert_eq!(d.fs.bytes("conf_opl.cfg").unwrap(), b"usb_mode=1\r\n");
    let entry = Manifest::read(&d.fs).unwrap().0;
    let entry = entry.entry("UPDT/OPL.ELF").unwrap();
    assert_eq!(entry.version.as_deref(), Some(NEW));
    assert_eq!(entry.language.as_deref(), Some("Russian"));
    let loader = &d.installed()[0];
    let backup = loader.backup.as_ref().unwrap();
    assert_eq!(
        backup.file,
        ".yaoplm/backup/loader/20251025-144640/UPDT_OPL.ELF"
    );
    assert_eq!(backup.version.as_deref(), Some(OLD));
    assert_eq!(d.fs.bytes(&backup.file).unwrap(), old);
}

#[test]
fn a_rollback_puts_the_backup_back_and_a_second_one_undoes_it() {
    let d = Device::new();
    let beta = build("opl-beta");
    let (old, new) = (opl_elf(OLD), opl_elf(NEW));
    d.install(&request(
        beta,
        d.elf(&old, Some(OLD)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW,
    ));
    let mut update = request(
        beta,
        d.elf(&new, Some(NEW)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW + 60,
    );
    update.update = true;
    d.install(&update);

    let plan = plan_rollback(&d.fs, "UPDT/OPL.ELF", catalog(), NOW + 120).unwrap();
    assert_eq!(plan.kind, PlanKind::RollbackLoader);
    d.run(&plan);
    assert_eq!(d.fs.bytes("UPDT/OPL.ELF").unwrap(), old);
    let loader = &d.installed()[0];
    assert_eq!(loader.version.as_deref(), Some(OLD));
    assert_eq!(loader.known_by, KnownBy::Manifest, "the manifest follows");
    assert_eq!(
        loader.entry.as_ref().and_then(|e| e.preset.as_deref()),
        Some("fmcb")
    );
    let backup = loader.backup.as_ref().unwrap();
    assert_eq!(backup.version.as_deref(), Some(NEW));
    // The emptied backup folder is gone; one backup, the newer build.
    let backups: Vec<String> =
        d.fs.files()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| p.starts_with(".yaoplm/backup/"))
            .collect();
    assert_eq!(
        backups,
        [".yaoplm/backup/loader/20251025-134840/UPDT_OPL.ELF"]
    );

    d.run(&plan_rollback(&d.fs, "UPDT/OPL.ELF", catalog(), NOW + 180).unwrap());
    assert_eq!(d.fs.bytes("UPDT/OPL.ELF").unwrap(), new);
    assert_eq!(d.installed()[0].version.as_deref(), Some(NEW));
}

#[test]
fn rolling_back_to_another_program_drops_the_manifest_entry() {
    let d = Device::new();
    let beta = build("opl-beta");
    let foreign = plain_elf(&["LAUNCHELF.CNF", "wLaunchELF 4.43x_isr"]);
    d.fs.insert_file("APPS/OPNPS2LD.ELF", &foreign);
    let elf = opl_elf(OLD);
    let mut first = request(
        beta,
        d.elf(&elf, Some(OLD)),
        "APPS/OPNPS2LD.ELF",
        Some("ps2bbl"),
        NOW,
    );
    first.replace_foreign = true;
    d.install(&first);

    d.run(&plan_rollback(&d.fs, "APPS/OPNPS2LD.ELF", catalog(), NOW).unwrap());
    assert_eq!(d.fs.bytes("APPS/OPNPS2LD.ELF").unwrap(), foreign);
    assert!(Manifest::read(&d.fs).unwrap().0.loaders.is_empty());
    // Within the same second: the OPL went to a folder of its own.
    assert_eq!(
        d.fs.bytes(".yaoplm/backup/loader/20251025-134640-1/APPS_OPNPS2LD.ELF")
            .unwrap(),
        elf
    );
}

#[test]
fn without_a_backup_there_is_nothing_to_roll_back() {
    let d = Device::new();
    let beta = build("opl-beta");
    let elf = opl_elf(OLD);
    d.install(&request(
        beta,
        d.elf(&elf, Some(OLD)),
        "UPDT/OPL.ELF",
        Some("fmcb"),
        NOW,
    ));
    let plan = plan_rollback(&d.fs, "UPDT/OPL.ELF", catalog(), NOW).unwrap();
    let codes: Vec<&str> = plan.notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["loaderNoBackup"]);
    assert!(plan.transactions.is_empty());
    let plan = plan_rollback(&d.fs, ".yaoplm/manifest.json", catalog(), NOW).unwrap();
    let codes: Vec<&str> = plan.notices.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["loaderPathInvalid"]);
}
