//! Loader detection on real loader ELFs (LIB-1, research/16 §9–§10), in
//! installs that are clean, renamed, mixed with another loader's leftovers,
//! cut short, or paired with another build. Needs `YAOPLM_TESTDATA` with
//! `downloads/loaders/` and its `elf-versions.tsv` (see
//! `crates/yaoplm-formats/tests/loader_elfs.rs`); run with
//! `cargo test -p yaoplm-core --test loader_corpus -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use yaoplm_core::detect::{Confidence, Detection, Family, detect_profile};
use yaoplm_device::FsKind;
use yaoplm_formats::LoaderProfile;
use yaoplm_test_support::MemFs;

struct Case {
    rel: String,
    family: Family,
    /// From the release and file names, never from reading the ELF.
    version: String,
}

fn cases() -> Option<(PathBuf, Vec<Case>)> {
    let root = PathBuf::from(std::env::var_os("YAOPLM_TESTDATA")?).join("downloads/loaders");
    let tsv = std::fs::read_to_string(root.join("elf-versions.tsv")).ok()?;
    let cases = tsv
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, v)| *v != "*")
        .filter_map(|(rel, version)| {
            let repo = rel.split('/').next()?;
            let tag = rel.split('/').nth(1)?;
            let family = match repo {
                r if r.starts_with("NathanNeurotic__") => Family::RiptOpl,
                "ps2homebrew__wOPL" if tag == "beta" => Family::Uopl,
                "ps2homebrew__wOPL" => Family::Wopl,
                "pcm720__nhddl" => Family::Nhddl,
                _ => Family::Opl,
            };
            Some(Case {
                rel: rel.to_owned(),
                family,
                version: version.to_owned(),
            })
        })
        .collect();
    Some((root, cases))
}

/// The rules each family's builds need today: 1.1.0's for OPL before 1.2.
fn profile(case: &Case) -> LoaderProfile {
    match case.family {
        Family::RiptOpl => LoaderProfile::RiptOpl,
        Family::Opl
            if case.version == "-"
                || case.version.starts_with("v0.")
                || case.version.starts_with("v1.0")
                || case.version.starts_with("v1.1") =>
        {
            LoaderProfile::Opl110
        }
        _ => LoaderProfile::Opl120Beta,
    }
}

/// Where the family's ELF usually sits, and the name FMCB's menu starts.
fn install_path(family: Family) -> &'static str {
    match family {
        Family::RiptOpl => "APPS/APP_RIPTOPL/RIPTOPL.ELF",
        Family::Wopl => "APPS/WOPL/WOPNPS2LD.ELF",
        Family::Nhddl => "APPS/NHDDL/nhddl.elf",
        Family::Opl | Family::Uopl => "UPDT/OPL.ELF",
    }
}

/// A settings file only another family writes.
fn leftover(family: Family) -> (&'static str, &'static [u8], Family) {
    match family {
        Family::RiptOpl => ("wopl_settings.cfg", b"display : {};\n", Family::Wopl),
        _ => ("settings_riptopl.cfg", b"enable_usb=1\r\n", Family::RiptOpl),
    }
}

fn codes(d: &Detection) -> Vec<&str> {
    d.notices.iter().map(|n| n.code.as_str()).collect()
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA/downloads/loaders"]
fn real_loaders_in_real_and_broken_installs() {
    let Some((root, cases)) = cases() else {
        panic!("set YAOPLM_TESTDATA to a folder with downloads/loaders/elf-versions.tsv");
    };
    let mut wrong = Vec::new();
    let mut runs = 0;
    for case in &cases {
        let data = std::fs::read(root.join(&case.rel)).unwrap();
        let want = profile(case);
        let check = |what: &str, d: &Detection, wrong: &mut Vec<String>| {
            if d.profile != want {
                wrong.push(format!(
                    "{} ({what}): profile {:?}, want {want:?}; {}",
                    case.rel,
                    d.profile,
                    d.reason.english()
                ));
            }
        };

        // 1. A clean install at the usual place.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file(install_path(case.family), &data);
        let d = detect_profile(&fs).unwrap();
        runs += 1;
        check("clean", &d, &mut wrong);
        if d.loaders.first().map(|l| l.family) != Some(case.family) {
            wrong.push(format!(
                "{} (clean): family {:?}",
                case.rel,
                d.loaders.first().map(|l| l.family)
            ));
        }
        let versioned = case.version != "-";
        if versioned && d.confidence != Confidence::Exact {
            wrong.push(format!(
                "{} (clean): {:?}, {}",
                case.rel,
                d.confidence,
                d.reason.english()
            ));
        }
        if !d.notices.is_empty() {
            wrong.push(format!("{} (clean): notices {:?}", case.rel, codes(&d)));
        }

        // 2. Renamed to what FMCB starts: the program still names its family.
        // NHDDL's has no family strings, so only its name tells.
        if case.family != Family::Nhddl {
            let fs = MemFs::new(FsKind::Fat32);
            fs.insert_file("UPDT/OPL.ELF", &data);
            let d = detect_profile(&fs).unwrap();
            runs += 1;
            if versioned || case.family == Family::Opl {
                check("renamed", &d, &mut wrong);
            }
        }

        // 3. Another loader's settings left next to it: still this one, and
        // the card says the files are left over.
        let (file, body, other) = leftover(case.family);
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file(install_path(case.family), &data);
        fs.insert_file(file, body);
        let d = detect_profile(&fs).unwrap();
        runs += 1;
        if versioned {
            check("leftovers", &d, &mut wrong);
            if other != case.family && !codes(&d).contains(&"leftoverLoaderFiles") {
                wrong.push(format!("{} (leftovers): notices {:?}", case.rel, codes(&d)));
            }
        }

        // 4. Cut in half (an interrupted copy): never an exact answer that the
        // intact file would not give.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file(install_path(case.family), &data[..data.len() / 2]);
        let d = detect_profile(&fs).unwrap();
        runs += 1;
        if d.confidence == Confidence::Exact && d.profile != want {
            wrong.push(format!("{} (cut): exact {:?}", case.rel, d.profile));
        }
    }

    // 5. Two upstream builds of other generations on one device decide
    // nothing at the exact level; the card lists both.
    let pick = |prefix: &str| {
        cases
            .iter()
            .find(|c| c.family == Family::Opl && c.version.starts_with(prefix))
    };
    if let (Some(old), Some(new)) = (pick("v1.1.0"), pick("v1.2.0-Beta-2271")) {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("UPDT/OPL.ELF", &std::fs::read(root.join(&old.rel)).unwrap());
        fs.insert_file(
            "APPS/OPNPS2LD.ELF",
            &std::fs::read(root.join(&new.rel)).unwrap(),
        );
        let d = detect_profile(&fs).unwrap();
        runs += 1;
        if d.confidence == Confidence::Exact || !codes(&d).contains(&"severalLoaders") {
            wrong.push(format!(
                "1.1.0 + 1.2: {:?} {}, {:?}",
                d.confidence,
                d.reason.english(),
                codes(&d)
            ));
        }
    }

    println!("{} ELFs, {runs} devices", cases.len());
    assert!(
        wrong.is_empty(),
        "{} problems:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
