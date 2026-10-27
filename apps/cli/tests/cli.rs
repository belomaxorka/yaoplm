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
    // The bundled game info (META-3), each value with its source.
    assert!(
        text.contains("Developer:      DMA Design (PlayStation DataCenter via libretro-database)"),
        "{text}"
    );
    assert!(text.contains("Released:       2001-10-23 ("), "{text}");
    assert!(text.contains("Genre:          Action ("), "{text}");
    assert!(
        text.contains("Wikidata:       https://www.wikidata.org/wiki/Q149106 (Wikipedia: Grand Theft Auto III)"),
        "{text}"
    );

    let json = yaoplm()
        .args(["info", "--json"])
        .arg(dir.path().join("CD/SLUS_200.62.GTA III.iso"))
        .output()
        .unwrap();
    let answer: serde_json::Value = serde_json::from_str(stdout(&json)).unwrap();
    assert_eq!(answer["info"]["developer"]["value"], "DMA Design");
    assert_eq!(answer["info"]["developer"]["source"], "libretro");
    assert_eq!(answer["info"]["players"]["value"], 1);
    assert_eq!(answer["info"]["genres"]["value"][0], "action");
    assert_eq!(answer["info"]["wikidata"]["qid"], "Q149106");

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

fn text_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
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
    // The bundled game info goes into the CFG with the game (META-3).
    assert!(
        text_of(&dry).contains("Game info of Retail"),
        "{}",
        text_of(&dry)
    );
    let cfg = std::fs::read(s.root.join("CFG/SLUS_200.62.cfg")).unwrap();
    assert!(
        cfg.starts_with(b"Title=Retail\r\nGenre=Action\r\n"),
        "{}",
        String::from_utf8_lossy(&cfg)
    );
    assert!(String::from_utf8_lossy(&cfg).contains("Developer=DMA Design\r\n"));

    // --no-info leaves the CFG folder empty.
    let plain = s.root.parent().unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let no_info = run(&[
        "add".as_ref(),
        plain.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--no-info".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        no_info.status.success(),
        "{}",
        String::from_utf8_lossy(&no_info.stderr)
    );
    assert!(!plain.join("CFG/SLUS_200.62.cfg").exists());

    // With --db-titles the bundled database names the games it knows.
    let other = s.root.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let named = run(&[
        "add".as_ref(),
        other.as_os_str(),
        s.games.as_os_str(),
        "--db-titles".as_ref(),
    ]);
    assert!(
        stdout(&named).contains("+ write CD/SLUS_200.62.Grand Theft Auto III.iso"),
        "{}",
        stdout(&named)
    );

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
    // The loader profile is read from the device by default (LIB-1); a temp
    // folder is on the computer, so OPL reads it over SMB (ADR-0047) and
    // the question is asked even though it is empty.
    let text = stdout(&device);
    assert!(
        text.contains("OPL:        OPL 1.2.0 beta: no signs of a loader on the device (auto)"),
        "{text}"
    );
    assert!(
        text.contains("Mode:       over the network (SMB): a folder on the computer’s disk (auto)"),
        "{text}"
    );
    assert!(
        text.contains("? Which OPL reads this folder over the network?"),
        "{text}"
    );
    let json = run(&["device".as_ref(), s.root.as_os_str(), "--json".as_ref()]);
    let answer: serde_json::Value = serde_json::from_str(stdout(&json)).unwrap();
    assert_eq!(answer["type"], "Device");
    assert_eq!(answer["loader"]["source"], "detected");
    assert_eq!(answer["mode"]["mode"], "smb");

    // As a stick plugged into the console: no question for an empty one.
    let ps2 = run(&[
        "device".as_ref(),
        s.root.as_os_str(),
        "--mode".as_ref(),
        "ps2".as_ref(),
    ]);
    let text = stdout(&ps2);
    assert!(
        text.contains("Mode:       plugged into the PS2 (--mode)"),
        "{text}"
    );
    assert!(!text.contains("? Which OPL"), "{text}");
    let scan = run(&[
        "scan".as_ref(),
        s.root.as_os_str(),
        "--mode".as_ref(),
        "ps2".as_ref(),
    ]);
    assert!(stdout(&scan).contains("Mode: plugged into the PS2 (--mode)"));

    // RiptOPL's app on the stick; `--profile` still decides when given. In
    // a folder read over SMB the ELF is a copy.
    let app = s.root.join("APPS").join("APP_RIPTOPL");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("RIPTOPL.ELF"), b"elf").unwrap();
    let device = run(&["device".as_ref(), s.root.as_os_str()]);
    let text = stdout(&device);
    assert!(
        text.contains(
            "OPL:        RiptOPL: RiptOPL at APPS/APP_RIPTOPL/RIPTOPL.ELF (a copy: over SMB the console starts OPL from a memory card or stick) (auto)"
        ),
        "{text}"
    );
    let device = run(&[
        "device".as_ref(),
        s.root.as_os_str(),
        "--mode".as_ref(),
        "ps2".as_ref(),
    ]);
    let text = stdout(&device);
    assert!(
        text.contains("OPL:        RiptOPL: RiptOPL at APPS/APP_RIPTOPL/RIPTOPL.ELF (auto)"),
        "{text}"
    );
    assert!(
        text.contains("Found:      RiptOPL at APPS/APP_RIPTOPL/RIPTOPL.ELF"),
        "{text}"
    );
    let given = run(&[
        "device".as_ref(),
        s.root.as_os_str(),
        "--profile".as_ref(),
        "opl-1.1".as_ref(),
    ]);
    assert!(stdout(&given).contains("OPL:        OPL 1.1.0 (--profile)"));
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
        "--skip-source".as_ref(),
        "oplm".as_ref(),
        "--skip-source".as_ref(),
        "screenscraper".as_ref(),
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
        "--skip-source".as_ref(),
        "oplm".as_ref(),
        "--skip-source".as_ref(),
        "screenscraper".as_ref(),
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
        "--skip-source".as_ref(),
        "oplm".as_ref(),
        "--skip-source".as_ref(),
        "screenscraper".as_ref(),
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
        "--skip-source".as_ref(),
        "oplm".as_ref(),
        "--skip-source".as_ref(),
        "screenscraper".as_ref(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&done);
    let errors = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "{text}\n{errors}");
    assert!(
        errors.contains("Pictures: 1 found, 1 not found, 0 already on the device"),
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

/// A cover search the API refuses stops `add --covers` before anything is
/// planned, as `art fetch` stops: only a search that fails while it runs
/// (the network) lets the games go on without covers.
#[test]
fn add_with_covers_refuses_bad_sources() {
    let s = stick();
    let missing = s.root.parent().unwrap().join("no-such-mirror");
    let mirror_arg = format!("xlenore={}", missing.display());
    for (flag, value, said) in [
        (
            "--skip-source",
            "oplmanager",
            "Unknown art source oplmanager",
        ),
        ("--mirror", mirror_arg.as_str(), "no-such-mirror"),
    ] {
        let refused = run(&[
            "add".as_ref(),
            s.root.as_os_str(),
            s.games.as_os_str(),
            "--covers".as_ref(),
            flag.as_ref(),
            value.as_ref(),
            "--yes".as_ref(),
        ]);
        let errors = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "{flag}: {errors}");
        assert!(errors.contains(said), "{flag}: {errors}");
        assert!(!errors.contains("without covers"), "{flag}: {errors}");
        assert!(stdout(&refused).is_empty(), "no plan: {}", stdout(&refused));
        assert!(
            std::fs::read_dir(&s.root).unwrap().next().is_none(),
            "{flag}: nothing written"
        );
    }
}

/// `--kind` on `art fetch`, `art set` and `art rm` (ART-2): screenshots
/// from a libretro mirror, a logo from a file, its removal by type.
#[test]
fn art_kinds_from_the_command_line() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--no-verify".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());
    let mirror = s.root.parent().unwrap().join("libretro");
    let snaps = mirror.join("Named_Snaps");
    std::fs::create_dir_all(&snaps).unwrap();
    let png = {
        let img = image::ImageBuffer::from_fn(320, 240, |x, y| image::Rgb([x as u8, y as u8, 3]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    // SLUS_200.62 by its Redump name in the bundled database.
    std::fs::write(snaps.join("Grand Theft Auto III (USA, Canada).png"), &png).unwrap();
    let mirror_arg = format!("libretro={}", mirror.display());
    // A type named twice, with another one in between, is searched once.
    let fetched = run(&[
        "art".as_ref(),
        "fetch".as_ref(),
        s.root.as_os_str(),
        "--kind".as_ref(),
        "SCR".as_ref(),
        "--kind".as_ref(),
        "LGO".as_ref(),
        "--kind".as_ref(),
        "scr".as_ref(),
        "--mirror".as_ref(),
        mirror_arg.as_ref(),
        "--skip-source".as_ref(),
        "xlenore".as_ref(),
        "--skip-source".as_ref(),
        "oplm".as_ref(),
        "--skip-source".as_ref(),
        "screenscraper".as_ref(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&fetched);
    assert!(
        fetched.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    assert!(text.contains("Plan: Write 1 picture"), "{text}");
    // One line, written or skipped, per game and type.
    assert!(text.contains("Skipped (3):"), "{text}");
    for line in [
        "+ write ART/SLUS_200.62_SCR.png",
        "SLUS_200.62 Retail (LGO):",
        "SLUS_212.18 Dvd Nine (SCR):",
        "SLUS_212.18 Dvd Nine (LGO):",
    ] {
        assert_eq!(text.matches(line).count(), 1, "{line}\n{text}");
    }
    assert!(s.root.join("ART/SLUS_200.62_SCR.png").is_file());
    assert!(!s.root.join("ART/SLUS_200.62_COV.png").exists());

    let unknown = run(&[
        "art".as_ref(),
        "fetch".as_ref(),
        s.root.as_os_str(),
        "--kind".as_ref(),
        "SCR3".as_ref(),
    ]);
    assert!(!unknown.status.success());

    let image = s.root.parent().unwrap().join("logo.png");
    std::fs::write(&image, &png).unwrap();
    let set = run(&[
        "art".as_ref(),
        "set".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        image.as_os_str(),
        "--kind".as_ref(),
        "lgo".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(s.root.join("ART/SLUS_212.18_LGO.png").is_file());
    assert!(!s.root.join("ART/SLUS_212.18_COV.png").exists());

    // The cover is not there; the logo is.
    let rm_cover = run(&[
        "art".as_ref(),
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        "-y".as_ref(),
    ]);
    assert_eq!(rm_cover.status.code(), Some(1));
    let rm = run(&[
        "art".as_ref(),
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        "--kind".as_ref(),
        "LGO".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rm.status.success(),
        "{}",
        String::from_utf8_lossy(&rm.stderr)
    );
    assert!(!s.root.join("ART/SLUS_212.18_LGO.png").exists());
    let bad = run(&[
        "art".as_ref(),
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_212.18".as_ref(),
        "--kind".as_ref(),
        "LOGO2".as_ref(),
        "-y".as_ref(),
    ]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("Unknown picture type LOGO2"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

/// `art import`: a folder set fills the icon of a game on the device; the
/// picture of a game that is not there is left out (ART-5).
#[test]
fn art_import_from_a_folder_set() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--no-verify".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());
    let set = s.root.parent().unwrap().join("set");
    std::fs::create_dir_all(&set).unwrap();
    let png = {
        let img = image::ImageBuffer::from_fn(32, 32, |x, y| image::Rgb([x as u8, y as u8, 1]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    std::fs::write(set.join("SLUS_200.62_ICO.png"), &png).unwrap();
    std::fs::write(set.join("SLES_999.99_ICO.png"), &png).unwrap();
    let done = run(&[
        "art".as_ref(),
        "import".as_ref(),
        s.root.as_os_str(),
        set.as_os_str(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&done);
    assert!(
        done.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(text.contains("+ write ART/SLUS_200.62_ICO.png"), "{text}");
    assert!(
        text.contains("Skipped pictures of games not on this device: 1"),
        "{text}"
    );
    assert!(s.root.join("ART/SLUS_200.62_ICO.png").is_file());
}

#[test]
fn cfg_shows_and_changes_a_games_settings() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());
    std::fs::write(
        s.root.join("CFG/SLUS_200.62.cfg"),
        "Title=Retail\r\n$Compatibility=1\r\n",
    )
    .unwrap();

    let show = run(&["cfg".as_ref(), s.root.as_os_str(), "SLUS_200.62".as_ref()]);
    assert!(show.status.success());
    let text = stdout(&show);
    assert!(text.contains("Modes:        1 (accurate reads)"), "{text}");
    assert!(text.contains("VMC slot 1:   -"), "{text}");
    assert!(text.contains("Title=Retail"), "{text}");

    let change = [
        "cfg".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--mode".as_ref(),
        "2".as_ref(),
        "--no-mode".as_ref(),
        "1".as_ref(),
        "--set".as_ref(),
        "Genre=Action".as_ref(),
    ];
    let dry = run(&change);
    let text = stdout(&dry);
    assert!(text.contains("$Compatibility: 1 -> 2"), "{text}");
    assert!(text.contains("Genre: (none) -> Action"), "{text}");
    assert!(String::from_utf8_lossy(&dry.stderr).contains("--yes"));
    let done = run(&[&change[..], &["--yes".as_ref()]].concat());
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        std::fs::read(s.root.join("CFG/SLUS_200.62.cfg")).unwrap(),
        b"Title=Retail\r\n$Compatibility=2\r\n$ConfigSource=1\r\nGenre=Action\r\n"
    );

    // The bundled game info (META-3): shown next to the CFG, written by a
    // plan, only the chosen fields.
    let info = run(&[
        "cfg".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--info".as_ref(),
    ]);
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    let text = stdout(&info);
    assert!(text.contains("Developer:    DMA Design"), "{text}");
    assert!(text.contains("CFG: -"), "{text}");
    assert!(text.contains("Genre:        Action"), "{text}");
    assert!(text.contains("CFG: Action"), "{text}");
    let written = run(&[
        "cfg".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--write-info".as_ref(),
        "--fields".as_ref(),
        "developer,players".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    assert_eq!(
        std::fs::read(s.root.join("CFG/SLUS_200.62.cfg")).unwrap(),
        b"Title=Retail\r\n$Compatibility=2\r\n$ConfigSource=1\r\nGenre=Action\r\nDeveloper=DMA Design\r\nPlayers=players/1\r\nPlayersText=1\r\n"
    );

    // Mode 7 is RiptOPL's.
    let refused = run(&[
        "cfg".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--mode".as_ref(),
        "7".as_ref(),
        "--yes".as_ref(),
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("OPL cannot use this setting"));
}

#[test]
fn vmc_create_assign_and_list() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());

    let create = run(&[
        "vmc".as_ref(),
        "create".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62_0".as_ref(),
        "--game".as_ref(),
        "SLUS_200.62".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let text = stdout(&create);
    assert!(
        text.contains("+ format memory card VMC/SLUS_200.62_0.bin  8.0 MiB"),
        "{text}"
    );
    assert!(text.contains("$VMC_0: (none) -> SLUS_200.62_0"), "{text}");
    let card = std::fs::read(s.root.join("VMC/SLUS_200.62_0.bin")).unwrap();
    assert_eq!(
        yaoplm_formats::vmc::opl_check(&card, card.len() as u64),
        Ok(8)
    );

    let list = run(&["vmc".as_ref(), "ls".as_ref(), s.root.as_os_str()]);
    let text = stdout(&list);
    assert!(
        text.contains("SLUS_200.62_0  8 MiB, 0 saves  used by SLUS_200.62 slot 1"),
        "{text}"
    );

    // The name is taken: only --replace makes a new card, and the old one is kept.
    let again = [
        "vmc".as_ref(),
        "create".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62_0".as_ref(),
        "--size".as_ref(),
        "16".as_ref(),
        "--yes".as_ref(),
    ];
    let refused = run(&again);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("already exists"));
    let replaced = run(&[&again[..], &["--replace".as_ref()]].concat());
    assert!(replaced.status.success());
    assert_eq!(
        std::fs::metadata(s.root.join("VMC/SLUS_200.62_0.bin"))
            .unwrap()
            .len(),
        16 << 20
    );
    let bad = run(&[
        "vmc".as_ref(),
        "create".as_ref(),
        s.root.as_os_str(),
        "x".as_ref(),
        "--size".as_ref(),
        "12".as_ref(),
    ]);
    assert_eq!(bad.status.code(), Some(2), "clap refuses the size");
}

#[test]
fn loader_ls_lists_the_catalogue() {
    // The built-in catalogue, not one a "Check for updates" stored in the
    // real app data folder.
    let data = tempfile::tempdir().unwrap();
    let yaoplm = || {
        let mut command = yaoplm();
        command.env("YAOPLM_DATA_DIR", data.path());
        command
    };
    let output = yaoplm().args(["loader", "ls"]).output().unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    for line in [
        "opl-stable  OPL 1.1.0",
        "opl-beta  OPL 1.2.0 beta  [recommended]",
        "yaopl  YAOPL",
        "official, ps2homebrew/Open-PS2-Loader @ v1.1.0 (fixed), profile OPL 1.1.0",
        "Preset fmcb: UPDT/OPL.ELF (usb), default on usb",
        "Preset app: APPS/YAOPL/OPNPS2LD.ELF (usb, mx4sio), title.cfg \"YAOPL\", default on mx4sio",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }

    let ru = yaoplm()
        .args(["loader", "ls", "--lang", "ru"])
        .output()
        .unwrap();
    assert!(
        stdout(&ru).contains("Свежая официальная сборка"),
        "{}",
        stdout(&ru)
    );

    let json = yaoplm().args(["loader", "ls", "--json"]).output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["type"], "LoaderCatalog");
    assert_eq!(value["builds"].as_array().unwrap().len(), 3);
}

#[test]
fn loader_release_download_import_and_files() {
    use yaoplm_test_support::loaders::{ReleaseMirror, opl_elf, sha256};
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("mirror");
    ReleaseMirror::new(&mirror).official().unwrap();
    let data = dir.path().join("data");
    let cli = |args: &[&std::ffi::OsStr]| {
        yaoplm()
            .args(args)
            .env("YAOPLM_DATA_DIR", &data)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let m = mirror.as_os_str();
    let release = cli(&[
        "loader".as_ref(),
        "release".as_ref(),
        "yaopl".as_ref(),
        "--mirror".as_ref(),
        m,
    ]);
    assert!(
        release.status.success(),
        "{}",
        String::from_utf8_lossy(&release.stderr)
    );
    let text = stdout(&release);
    let yaopl = opl_elf("v1.2.0-Beta-2370-33a2b19-yaopl");
    for line in [
        "yaopl v1.2.0-Beta-2370-33a2b19-yaopl (release latest, 2026-10-23)",
        &format!(
            "SHA-256: {} (from GitHub digest and the release's SHA256SUMS)",
            sha256(&yaopl)
        ),
        "Licence: AFL-3.0",
        "No change list was published.",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }

    // Without --yes and without a terminal nothing is downloaded.
    let dry = cli(&[
        "loader".as_ref(),
        "download".as_ref(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
    ]);
    assert!(dry.status.success());
    assert!(String::from_utf8_lossy(&dry.stderr).contains("add --yes"));
    let files = cli(&["loader".as_ref(), "files".as_ref()]);
    assert_eq!(stdout(&files), "No loader stored yet.");

    let got = cli(&[
        "loader".as_ref(),
        "download".as_ref(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
        "--yes".as_ref(),
    ]);
    assert!(
        got.status.success(),
        "{}",
        String::from_utf8_lossy(&got.stderr)
    );
    let beta = sha256(&opl_elf("v1.2.0-Beta-2271-b3a2199"));
    assert!(
        stdout(&got).contains(&format!("SHA-256: {beta}")),
        "{}",
        stdout(&got)
    );
    assert!(stdout(&got).contains("confirmed by its release before the download"));

    let own = dir.path().join("OPL.ELF");
    std::fs::write(&own, opl_elf("v1.2.0-Beta-1990-0123abc")).unwrap();
    let imported = cli(&["loader".as_ref(), "import".as_ref(), own.as_os_str()]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(
        stdout(&imported).contains("opl-beta v1.2.0-Beta-1990-0123abc"),
        "{}",
        stdout(&imported)
    );
    assert!(stdout(&imported).contains("your own file; nothing vouches for it"));

    let listed = cli(&["loader".as_ref(), "files".as_ref(), "--json".as_ref()]);
    let value: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(value["files"].as_array().unwrap().len(), 2);

    // A substituted 1.1.0 archive fails, with the reason, and exit 1.
    let bad = cli(&[
        "loader".as_ref(),
        "download".as_ref(),
        "opl-stable".as_ref(),
        "--mirror".as_ref(),
        m,
        "--yes".as_ref(),
    ]);
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("does not match its SHA-256"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

#[test]
fn loader_install_writes_through_a_plan_and_says_how_to_start_it() {
    use yaoplm_test_support::loaders::{ReleaseMirror, opl_elf};
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("mirror");
    ReleaseMirror::new(&mirror).official().unwrap();
    let data = dir.path().join("data");
    let stick = dir.path().join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    let cli = |args: &[&std::ffi::OsStr]| {
        yaoplm()
            .args(args)
            .env("YAOPLM_DATA_DIR", &data)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let m = mirror.as_os_str();
    // Without --yes: the download is offered, nothing is written.
    let dry = cli(&[
        "loader".as_ref(),
        "install".as_ref(),
        stick.as_os_str(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
    ]);
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(!stick.join("UPDT").exists());

    let done = cli(&[
        "loader".as_ref(),
        "install".as_ref(),
        stick.as_os_str(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
        "--language".as_ref(),
        "Russian".as_ref(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&done);
    assert!(
        done.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        std::fs::read(stick.join("UPDT/OPL.ELF")).unwrap(),
        opl_elf("v1.2.0-Beta-2271-b3a2199")
    );
    assert!(stick.join("LNG/lang_Russian.lng").is_file());
    assert!(stick.join(".yaoplm/manifest.json").is_file());
    for line in [
        "How to start it on the PS2:",
        "FreeMcBoot: its stock config starts mass:/UPDT/OPL.ELF; nothing to edit.",
        "turn on the USB device",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }

    let mx = dir.path().join("sd");
    std::fs::create_dir_all(&mx).unwrap();
    let done = cli(&[
        "loader".as_ref(),
        "install".as_ref(),
        mx.as_os_str(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
        "--mx4sio".as_ref(),
        "--yes".as_ref(),
    ]);
    let text = stdout(&done);
    assert!(done.status.success(), "{text}");
    assert!(mx.join("APPS/OPL/OPNPS2LD.ELF").is_file() && mx.join("APPS/OPL/title.cfg").is_file());
    assert!(
        text.contains("LK_AUTO_E1 = massX:/APPS/OPL/OPNPS2LD.ELF"),
        "{text}"
    );
    assert!(!text.contains("FreeMcBoot"), "{text}");
}

#[test]
fn an_emulator_s_sd_is_offered_no_loader_command() {
    // ADR-0048, review of PR 1: `yaoplm loader update` and `rollback` are
    // always refused on a memory card emulator's SD, so `device` names
    // neither, though it still lists the ELF it finds.
    use yaoplm_test_support::loaders::opl_elf;
    let dir = tempfile::tempdir().unwrap();
    let sd = dir.path().join("sd");
    yaoplm_test_support::emulator::write_to(&sd).unwrap();
    std::fs::create_dir_all(sd.join("UPDT")).unwrap();
    std::fs::write(sd.join("UPDT/OPL.ELF"), opl_elf("v1.2.0-Beta-2271-b3a2199")).unwrap();
    let backup = sd.join(".yaoplm/backup/loader/20261025-120000");
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(
        backup.join("UPDT_OPL.ELF"),
        opl_elf("v1.2.0-Beta-2260-0000000"),
    )
    .unwrap();
    let output = yaoplm()
        .args(["device".as_ref(), sd.as_os_str()])
        .env("YAOPLM_DATA_DIR", dir.path().join("data"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(
        text.contains("Mode:       in a memory card emulator (MMCE)"),
        "{text}"
    );
    assert!(
        text.contains("Installed:  opl-beta v1.2.0-Beta-2271-b3a2199 at UPDT/OPL.ELF"),
        "{text}"
    );
    assert!(!text.contains("yaoplm loader"), "{text}");
}

#[test]
fn loader_update_and_rollback_through_plans() {
    use yaoplm_test_support::loaders::{ReleaseMirror, opl_elf};
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("mirror");
    let releases = ReleaseMirror::new(&mirror);
    releases.official().unwrap();
    let data = dir.path().join("data");
    let stick = dir.path().join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    let cli = |args: &[&std::ffi::OsStr]| {
        yaoplm()
            .args(args)
            .env("YAOPLM_DATA_DIR", &data)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let (m, at) = (mirror.as_os_str(), stick.as_os_str());
    let ok = |out: &std::process::Output| {
        assert!(
            out.status.success(),
            "{}\n{}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        );
        stdout(out).to_owned()
    };
    // Nothing to update yet.
    let none = cli(&[
        "loader".as_ref(),
        "update".as_ref(),
        at,
        "--mirror".as_ref(),
        m,
    ]);
    assert!(!none.status.success());
    assert!(String::from_utf8_lossy(&none.stderr).contains("no loader YAOPLM knows"));

    ok(&cli(&[
        "loader".as_ref(),
        "install".as_ref(),
        at,
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
        "--yes".as_ref(),
    ]));
    let text = ok(&cli(&["device".as_ref(), at]));
    assert!(
        text.contains("Installed:  opl-beta v1.2.0-Beta-2271-b3a2199 at UPDT/OPL.ELF (installed by YAOPLM); up to date"),
        "{text}"
    );
    let text = ok(&cli(&[
        "loader".as_ref(),
        "update".as_ref(),
        at,
        "--mirror".as_ref(),
        m,
    ]));
    assert!(
        text.contains("is the newest build of its channel"),
        "{text}"
    );

    releases
        .beta("v1.2.0-Beta-2280-0123abc", "2026-10-20")
        .unwrap();
    let text = ok(&cli(&[
        "loader".as_ref(),
        "update".as_ref(),
        at,
        "--mirror".as_ref(),
        m,
        "--yes".as_ref(),
    ]));
    assert!(
        text.contains("Update OPL 1.2.0 beta to v1.2.0-Beta-2280-0123abc"),
        "{text}"
    );
    assert_eq!(
        std::fs::read(stick.join("UPDT/OPL.ELF")).unwrap(),
        opl_elf("v1.2.0-Beta-2280-0123abc")
    );
    let text = ok(&cli(&["device".as_ref(), at]));
    assert!(
        text.contains("v1.2.0-Beta-2280-0123abc at UPDT/OPL.ELF (installed by YAOPLM); up to date; backup v1.2.0-Beta-2271-b3a2199: `yaoplm loader rollback`"),
        "{text}"
    );

    let text = ok(&cli(&[
        "loader".as_ref(),
        "rollback".as_ref(),
        at,
        "--yes".as_ref(),
    ]));
    assert!(
        text.contains("Roll back the loader in UPDT/OPL.ELF"),
        "{text}"
    );
    assert_eq!(
        std::fs::read(stick.join("UPDT/OPL.ELF")).unwrap(),
        opl_elf("v1.2.0-Beta-2271-b3a2199")
    );
    let text = ok(&cli(&["device".as_ref(), at]));
    assert!(
        text.contains("update to v1.2.0-Beta-2280-0123abc: `yaoplm loader update`"),
        "{text}"
    );
}

#[test]
fn loader_replace_says_what_changes_and_converts_first() {
    use std::io::Cursor;
    use yaoplm_formats::zso;
    use yaoplm_test_support::Variant;
    use yaoplm_test_support::loaders::{ReleaseMirror, opl_elf};
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("mirror");
    ReleaseMirror::new(&mirror).official().unwrap();
    let data = dir.path().join("data");
    let stick = dir.path().join("stick");
    let iso = Variant::Retail.build().to_vec();
    let mut packed = Cursor::new(Vec::new());
    zso::compress(
        &mut Cursor::new(&iso),
        iso.len() as u64,
        &mut packed,
        &mut |_| {},
    )
    .unwrap();
    std::fs::create_dir_all(stick.join("DVD")).unwrap();
    std::fs::write(
        stick.join("DVD/SLUS_200.62.Retail Game.zso"),
        packed.into_inner(),
    )
    .unwrap();
    let stable = dir.path().join("OPNPS2LD-v1.1.0.ELF");
    std::fs::write(&stable, opl_elf("v1.1.0")).unwrap();
    let cli = |args: &[&std::ffi::OsStr]| {
        yaoplm()
            .args(args)
            .env("YAOPLM_DATA_DIR", &data)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let (m, at) = (mirror.as_os_str(), stick.as_os_str());
    let ok = |out: &std::process::Output| {
        assert!(
            out.status.success(),
            "{}\n{}",
            stdout(out),
            String::from_utf8_lossy(&out.stderr)
        );
        stdout(out).to_owned()
    };
    let text = ok(&cli(&[
        "loader".as_ref(),
        "releases".as_ref(),
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
    ]));
    assert!(text.contains("latest  v1.2.0-Beta-2271-b3a2199"), "{text}");
    assert!(text.contains("(default)"), "{text}");

    ok(&cli(&[
        "loader".as_ref(),
        "install".as_ref(),
        at,
        "opl-beta".as_ref(),
        "--mirror".as_ref(),
        m,
        "--yes".as_ref(),
    ]));
    // A dry run shows what changes and the plan, and writes nothing.
    let text = ok(&cli(&[
        "loader".as_ref(),
        "replace".as_ref(),
        at,
        "opl-stable".as_ref(),
        "--file".as_ref(),
        stable.as_os_str(),
        "--dry-run".as_ref(),
    ]));
    for line in [
        "What changes for the games:",
        "ZSO the new build cannot show, converted to ISO first (--keep-zso leaves them): 1",
        "SLUS_200.62 Retail Game",
        "Replace OPL 1.2.0 beta with OPL 1.1.0",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    assert!(stick.join("DVD/SLUS_200.62.Retail Game.zso").exists());

    let text = ok(&cli(&[
        "loader".as_ref(),
        "replace".as_ref(),
        at,
        "opl-stable".as_ref(),
        "--file".as_ref(),
        stable.as_os_str(),
        "--yes".as_ref(),
    ]));
    assert!(
        text.contains("Replace OPL 1.2.0 beta with OPL 1.1.0"),
        "{text}"
    );
    assert_eq!(
        std::fs::read(stick.join("DVD/SLUS_200.62.Retail Game.iso")).unwrap(),
        iso
    );
    assert_eq!(
        std::fs::read(stick.join("UPDT/OPL.ELF")).unwrap(),
        opl_elf("v1.1.0")
    );
    let text = ok(&cli(&["device".as_ref(), at]));
    assert!(
        text.contains("Installed:  opl-stable v1.1.0 at UPDT/OPL.ELF"),
        "{text}"
    );
}
