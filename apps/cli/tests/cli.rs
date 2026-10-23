#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

thread_local! {
    /// The app data folder of the running test (each test has a thread):
    /// writes check dumps and keep hashes, never in the real folder.
    static DATA_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn yaoplm() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_yaoplm"));
    DATA_DIR.with(|dir| {
        if let Some(dir) = &*dir.borrow() {
            command.env("YAOPLM_DATA_DIR", dir);
        }
    });
    command
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap().trim()
}

#[test]
fn prints_version() {
    let output = yaoplm().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        stdout(&output),
        format!("yaoplm {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rpc_ping_from_argument() {
    let output = yaoplm()
        .args(["rpc", r#"{"type":"Ping"}"#])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(stdout(&output), r#"{"type":"Pong"}"#);
}

fn rpc_stdin(input: &[u8]) -> Output {
    let mut child = yaoplm()
        .arg("rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn rpc_reads_request_from_stdin() {
    let output = rpc_stdin(br#"{"type":"Version"}"#);
    assert!(output.status.success());

    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "Version");
    assert_eq!(answer["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn rpc_stdin_tolerates_utf8_bom() {
    let output = rpc_stdin(b"\xef\xbb\xbf{\"type\":\"Ping\"}\r\n");
    assert!(output.status.success());
    assert_eq!(stdout(&output), r#"{"type":"Pong"}"#);
}

#[test]
fn rpc_error_prints_json_and_fails() {
    let output = yaoplm()
        .args(["rpc", r#"{"type":"Nope"}"#])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));

    let error: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(error["code"], "BadRequest");
}

fn library() -> tempfile::TempDir {
    use yaoplm_test_support::Variant;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("CD")).unwrap();
    std::fs::create_dir_all(dir.path().join("DVD")).unwrap();
    let retail = Variant::Retail.build().to_vec();
    std::fs::write(dir.path().join("CD/SLUS_200.62.GTA III.iso"), retail).unwrap();
    std::fs::write(
        dir.path().join("DVD/No Boot.iso"),
        Variant::NoBoot2.build().to_vec(),
    )
    .unwrap();
    dir
}

#[test]
fn scan_prints_the_library() {
    let dir = library();
    let output = yaoplm().arg("scan").arg(dir.path()).output().unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("OPL 1.2.0 beta"), "{text}");
    assert!(text.contains("SLUS_200.62"), "{text}");
    assert!(text.contains("GTA III"), "{text}");
    assert!(text.contains("Not listed by OPL (1)"), "{text}");
    assert!(text.contains("No Boot.iso"), "{text}");
}

#[test]
fn scan_json_matches_the_api() {
    let dir = library();
    let output = yaoplm()
        .args(["scan", "--json", "--profile", "opl-1.1"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "Library");
    assert_eq!(answer["profile"], "Opl110");
    assert_eq!(answer["games"][0]["nameFormat"], "old");
}

#[test]
fn info_describes_an_image() {
    let dir = library();
    let output = yaoplm()
        .arg("info")
        .arg(dir.path().join("CD/SLUS_200.62.GTA III.iso"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Game ID:        SLUS_200.62"), "{text}");
    assert!(text.contains("old style"), "{text}");

    let hidden = yaoplm()
        .args(["info", "--json"])
        .arg(dir.path().join("DVD/No Boot.iso"))
        .output()
        .unwrap();
    let answer: serde_json::Value = serde_json::from_str(stdout(&hidden)).unwrap();
    assert_eq!(answer["startup"], serde_json::Value::Null);
}

#[test]
fn scan_of_a_missing_folder_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = yaoplm()
        .arg("scan")
        .arg(dir.path().join("nope"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("yaoplm:"));
}

struct Stick {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    games: std::path::PathBuf,
}

fn stick() -> Stick {
    use yaoplm_test_support::Variant;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("stick");
    let games = dir.path().join("games");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&games).unwrap();
    DATA_DIR.with(|data| *data.borrow_mut() = Some(dir.path().join("data")));
    std::fs::write(games.join("Retail.iso"), Variant::Retail.build().to_vec()).unwrap();
    std::fs::write(games.join("Dvd Nine.iso"), Variant::Dvd9.build().to_vec()).unwrap();
    Stick {
        root,
        games,
        _dir: dir,
    }
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    yaoplm().args(args).stdin(Stdio::null()).output().unwrap()
}

#[test]
fn add_prints_the_plan_and_needs_yes() {
    let s = stick();
    let dry = run(&["add".as_ref(), s.root.as_os_str(), s.games.as_os_str()]);
    assert!(dry.status.success());
    let text = stdout(&dry);
    assert!(text.contains("Plan: Import 2 games"), "{text}");
    assert!(text.contains("+ write CD/SLUS_200.62.Retail.iso"), "{text}");
    assert!(
        text.contains("+ write DVD/SLUS_212.18.Dvd Nine.iso"),
        "{text}"
    );
    assert!(String::from_utf8_lossy(&dry.stderr).contains("--yes"));
    assert!(
        std::fs::read_dir(&s.root).unwrap().next().is_none(),
        "nothing written"
    );

    let done = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(stdout(&done).contains("done  Copy SLUS_200.62.Retail.iso to CD"));
    assert!(s.root.join("CD/SLUS_200.62.Retail.iso").is_file());
    assert!(s.root.join("APPS").is_dir());

    // The same games again: skipped, nothing to apply.
    let again = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--yes".as_ref(),
    ]);
    assert_eq!(again.status.code(), Some(1));
    assert!(stdout(&again).contains("already exists"));
}

#[test]
fn rename_move_and_remove_by_id() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());

    let rename = run(&[
        "rename".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "New Name".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rename.status.success(),
        "{}",
        String::from_utf8_lossy(&rename.stderr)
    );
    assert!(s.root.join("CD/SLUS_200.62.New Name.iso").is_file());

    let mv = run(&[
        "mv".as_ref(),
        s.root.as_os_str(),
        "New Name".as_ref(),
        "--to".as_ref(),
        "dvd".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        mv.status.success(),
        "{}",
        String::from_utf8_lossy(&mv.stderr)
    );
    assert!(s.root.join("DVD/SLUS_200.62.New Name.iso").is_file());

    // Deleting shows DELETE in the plan and asks for --yes.
    let dry = run(&["rm".as_ref(), s.root.as_os_str(), "SLUS_200.62".as_ref()]);
    assert!(stdout(&dry).contains("- DELETE DVD/SLUS_200.62.New Name.iso"));
    assert!(s.root.join("DVD/SLUS_200.62.New Name.iso").is_file());
    let rm = run(&[
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(rm.status.success());
    assert!(!s.root.join("DVD/SLUS_200.62.New Name.iso").exists());

    let missing = run(&[
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_999.99".as_ref(),
        "--yes".as_ref(),
    ]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no game matches"));
}

#[test]
fn ul_games_on_request_and_recover_without_journals() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--ul".as_ref(),
        "--title".as_ref(),
        "Retail Ul".as_ref(),
        "--yes".as_ref(),
        "--json".as_ref(),
    ]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(s.root.join("ul.cfg").is_file());
    let rename = run(&[
        "rename".as_ref(),
        s.root.as_os_str(),
        "ul:0".as_ref(),
        "Short".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rename.status.success(),
        "{}",
        String::from_utf8_lossy(&rename.stderr)
    );
    let scan = run(&["scan".as_ref(), s.root.as_os_str()]);
    assert!(
        stdout(&scan).contains("Short (1 part)"),
        "{}",
        stdout(&scan)
    );

    let recover = run(&["recover".as_ref(), s.root.as_os_str()]);
    assert!(recover.status.success());
    assert!(stdout(&recover).contains("No unfinished operation"));
}

#[test]
fn devices_and_device_doctor() {
    let s = stick();
    let all = run(&["devices".as_ref(), "--all".as_ref()]);
    assert!(all.status.success());
    assert!(!stdout(&all).is_empty());
    let device = run(&["device".as_ref(), s.root.as_os_str()]);
    assert!(device.status.success());
    let json = run(&["device".as_ref(), s.root.as_os_str(), "--json".as_ref()]);
    let answer: serde_json::Value = serde_json::from_str(stdout(&json)).unwrap();
    assert_eq!(answer["type"], "Device");
}

#[test]
fn rpc_waits_for_jobs() {
    let s = stick();
    let request = serde_json::json!({"type": "ScanLibrary", "root": s.root}).to_string();
    let output = yaoplm().args(["rpc", &request]).output().unwrap();
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "JobState");
    assert_eq!(answer["status"], "finished");
    assert_eq!(answer["result"]["type"], "Library");
}

/// `art fetch` from local mirrors, `art set`, `art rm`, `art cache` and
/// `about` (ART-1, ART-3, ART-4). The cache goes to the temp folder
/// through YAOPLM_DATA_DIR, never to the real app data folder.
#[test]
fn art_fetch_set_and_remove_from_mirrors() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());
    let mirror = s.root.parent().unwrap().join("mirror");
    let covers = mirror.join("covers/default");
    std::fs::create_dir_all(&covers).unwrap();
    let jpeg = {
        let img = image::ImageBuffer::from_fn(300, 420, |x, y| image::Rgb([x as u8, y as u8, 99]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    };
    std::fs::write(covers.join("SLUS-20062.jpg"), jpeg).unwrap();
    let data = s.root.parent().unwrap().join("data");
    let art = |args: &[&std::ffi::OsStr]| {
        let mut all: Vec<&std::ffi::OsStr> = vec!["art".as_ref()];
        all.extend_from_slice(args);
        yaoplm()
            .args(&all)
            .env("YAOPLM_DATA_DIR", &data)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let mirror_arg = format!("xlenore={}", mirror.display());
    let dry = art(&[
        "fetch".as_ref(),
        s.root.as_os_str(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
        "--skip-source".as_ref(),
        "libretro".as_ref(),
    ]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let text = stdout(&dry);
    assert!(text.contains("Plan: Write 1 cover"), "{text}");
    assert!(text.contains("+ write ART/SLUS_200.62_COV.png"), "{text}");
    assert!(
        text.contains("SLUS_212.18"),
        "the DVD-9 game is listed as not found: {text}"
    );
    assert!(!s.root.join("ART/SLUS_200.62_COV.png").exists(), "dry run");

    let done = art(&[
        "fetch".as_ref(),
        s.root.as_os_str(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
        "--skip-source".as_ref(),
        "libretro".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(s.root.join("ART/SLUS_200.62_COV.png").is_file());
    let scan = run(&["scan".as_ref(), s.root.as_os_str(), "--json".as_ref()]);
    let library: serde_json::Value = serde_json::from_str(stdout(&scan)).unwrap();
    let retail = library["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["startup"] == "SLUS_200.62")
        .unwrap();
    assert_eq!(retail["art"][0]["kind"], "COV");
    assert_eq!(retail["art"][0]["width"], 140);

    // Everything missing was already tried: nothing to apply, exit 1.
    let again = art(&[
        "fetch".as_ref(),
        s.root.as_os_str(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
        "--skip-source".as_ref(),
        "libretro".as_ref(),
        "--yes".as_ref(),
    ]);
    assert_eq!(again.status.code(), Some(1));

    // A manual cover for the DVD-9 game, then its removal.
    let set = art(&[
        "set".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        covers.join("SLUS-20062.jpg").as_os_str(),
        "-y".as_ref(),
    ]);
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(s.root.join("ART/SLUS_212.18_COV.png").is_file());
    let rm = art(&[
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rm.status.success(),
        "{}",
        String::from_utf8_lossy(&rm.stderr)
    );
    assert!(!s.root.join("ART/SLUS_212.18_COV.png").exists());
    assert!(s.root.join(".yaoplm/backup").is_dir());

    let cache = art(&["cache".as_ref()]);
    assert!(cache.status.success());
    assert!(stdout(&cache).contains("objects"), "{}", stdout(&cache));
    let cleared = art(&["cache".as_ref(), "--clear".as_ref()]);
    assert!(
        stdout(&cleared).contains("0 objects"),
        "{}",
        stdout(&cleared)
    );

    let about = run(&["about".as_ref()]);
    assert!(about.status.success());
    assert!(stdout(&about).contains("PCSX2 GameIndex"));
    assert!(stdout(&about).contains("xlenore/ps2-covers"));
}

#[test]
fn redump_import_and_verify() {
    use yaoplm_test_support::Variant;
    use yaoplm_test_support::dat::DatBuilder;
    let lib = library();
    let data = tempfile::tempdir().unwrap();
    let cli = |args: &[&std::ffi::OsStr]| {
        yaoplm()
            .args(args)
            .env("YAOPLM_DATA_DIR", data.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let status = cli(&["redump".as_ref()]);
    assert!(status.status.success());
    assert!(
        stdout(&status).starts_with("No Redump DAT yet"),
        "{}",
        stdout(&status)
    );

    let dat = data.path().join("ps2.dat");
    let retail = Variant::Retail.build().to_vec();
    std::fs::write(
        &dat,
        DatBuilder::new()
            .iso("Grand Theft Auto III (USA)", Some("SLUS-20062"), &retail)
            .build(),
    )
    .unwrap();
    let imported = cli(&["redump".as_ref(), "--import".as_ref(), dat.as_os_str()]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(
        stdout(&imported).contains("Imported."),
        "{}",
        stdout(&imported)
    );
    assert!(stdout(&imported).contains("1 games with serials"));

    let verified = cli(&["verify".as_ref(), lib.path().as_os_str()]);
    let text = stdout(&verified);
    assert!(verified.status.success(), "{text}");
    assert!(text.contains("SLUS_200.62  verified"), "{text}");
    assert!(
        text.contains("Redump: Grand Theft Auto III (USA)"),
        "{text}"
    );

    let json = cli(&[
        "verify".as_ref(),
        lib.path().as_os_str(),
        "--game".as_ref(),
        "SLUS_200.62".as_ref(),
        "--md5".as_ref(),
        "--json".as_ref(),
    ]);
    assert!(json.status.success());
    let report: serde_json::Value = serde_json::from_str(stdout(&json)).unwrap();
    assert_eq!(report["games"][0]["status"], "verified");
    assert_eq!(
        report["games"][0]["hashes"]["md5"].as_str().map(str::len),
        Some(32)
    );

    // `redump --json` prints the DAT itself, as --import does its result.
    let shown = cli(&["redump".as_ref(), "--json".as_ref()]);
    let shown: serde_json::Value = serde_json::from_str(stdout(&shown)).unwrap();
    assert_eq!(shown["games"], 1);

    // A DAT that lists the ID with other hashes: a bad dump, exit code 1.
    let other = Variant::LfOnly.build().to_vec();
    std::fs::write(
        &dat,
        DatBuilder::new()
            .iso("Grand Theft Auto III (USA)", Some("SLUS-20062"), &other)
            .build(),
    )
    .unwrap();
    let imported = cli(&["redump".as_ref(), "--import".as_ref(), dat.as_os_str()]);
    assert!(imported.status.success());
    let bad = cli(&[
        "verify".as_ref(),
        lib.path().as_os_str(),
        "--fresh".as_ref(),
    ]);
    assert_eq!(bad.status.code(), Some(1), "{}", stdout(&bad));
    assert!(stdout(&bad).contains("BAD DUMP"), "{}", stdout(&bad));
}

#[test]
fn add_compressed_then_convert() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--zso".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(
        stdout(&add).contains("+ compress to CD/SLUS_200.62.Retail.zso"),
        "{}",
        stdout(&add)
    );
    assert!(s.root.join("CD/SLUS_200.62.Retail.zso").is_file());

    let dry = run(&[
        "convert".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--to".as_ref(),
        "iso".as_ref(),
    ]);
    let text = stdout(&dry);
    assert!(dry.status.success(), "{text}");
    assert!(text.contains("Plan: Convert 1 game to ISO"), "{text}");
    assert!(
        text.contains("- DELETE CD/SLUS_200.62.Retail.zso"),
        "{text}"
    );
    assert!(
        s.root.join("CD/SLUS_200.62.Retail.zso").is_file(),
        "dry run"
    );

    let done = run(&[
        "convert".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--to".as_ref(),
        "iso".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(s.root.join("CD/SLUS_200.62.Retail.iso").is_file());
    assert!(!s.root.join("CD/SLUS_200.62.Retail.zso").exists());
}

/// `add --covers` (ART-8): the covers of the games being added are looked
/// up first and written in the same plan; the dump check runs by default
/// (GAME-5) and `--no-verify` turns it off.
#[test]
fn add_with_covers_and_dump_checks() {
    let s = stick();
    let mirror = s.root.parent().unwrap().join("mirror");
    let covers = mirror.join("covers/default");
    std::fs::create_dir_all(&covers).unwrap();
    let jpeg = {
        let img = image::ImageBuffer::from_fn(300, 420, |x, y| image::Rgb([x as u8, y as u8, 7]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    };
    std::fs::write(covers.join("SLUS-20062.jpg"), jpeg).unwrap();
    let mirror_arg = format!("xlenore={}", mirror.display());
    let done = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--covers".as_ref(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
        "--skip-source".as_ref(),
        "libretro".as_ref(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&done);
    let errors = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "{text}\n{errors}");
    assert!(
        errors.contains("Covers: 1 found, 1 not found, 0 already on the device"),
        "{errors}"
    );
    assert!(text.contains("+ write ART/SLUS_200.62_COV.png"), "{text}");
    assert!(s.root.join("ART/SLUS_200.62_COV.png").is_file());
    // No DAT yet: the images were hashed, nothing to compare with.
    assert!(text.contains("Dump checks"), "{text}");
    assert!(
        text.contains("0 verified, 0 bad, 2 unknown, 0 failed"),
        "{text}"
    );

    // --mirror needs --covers.
    let bad = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
    ]);
    assert!(!bad.status.success());

    let other = s.root.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let quiet = run(&[
        "add".as_ref(),
        other.as_os_str(),
        s.games.as_os_str(),
        "--no-verify".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(quiet.status.success());
    assert!(!stdout(&quiet).contains("Dump checks"));
}
