//! Cross-checks against OPL's own code (TESTING.md, "Перекрёстные проверки").
//!
//! OPL is AFL-3.0, so its source is never copied into this repository
//! (ADR-0002). These tests read it from a checkout at run time:
//! - `YAOPLM_OPL_SRC`: an Open-PS2-Loader checkout. Locally
//!   `D:\yaoplm-testdata\refs\ps2homebrew__Open-PS2-Loader`; CI downloads the
//!   needed files at a pinned commit.
//! - `YAOPLM_PYTHON` (optional): Python with the `lz4` package for `ziso.py`.
//!
//! Run with `cargo test -p yaoplm-formats --test opl_oracles -- --ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use yaoplm_formats::usba_crc32::usba_crc32;

fn opl_src() -> PathBuf {
    let dir = std::env::var_os("YAOPLM_OPL_SRC")
        .expect("set YAOPLM_OPL_SRC to an Open-PS2-Loader checkout");
    PathBuf::from(dir)
}

/// Small deterministic PRNG, so failures reproduce.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Copies `crctab` and `USBA_crc32()` out of OPL's `src/system.c`.
fn extract_usba_crc32(system_c: &str) -> String {
    let lines: Vec<&str> = system_c.lines().collect();
    let table = lines
        .iter()
        .find(|l| l.trim_start().starts_with("static unsigned int crctab"))
        .expect("crctab declaration");
    let start = lines
        .iter()
        .position(|l| l.starts_with("unsigned int USBA_crc32("))
        .expect("USBA_crc32 definition");
    let end = start
        + lines[start..]
            .iter()
            .position(|l| *l == "}")
            .expect("end of function");
    format!("{table}\n{}\n", lines[start..=end].join("\n"))
}

const HARNESS: &str = r#"
#include <stdio.h>
#include <string.h>
#include "opl_usba_crc32.c"

static int hex(int c) { return c <= '9' ? c - '0' : (c | 0x20) - 'a' + 10; }

/* One hex-encoded name per line in, one %08X hash per line out. */
int main(void)
{
    static char line[1024], name[512];
    while (fgets(line, sizeof line, stdin)) {
        size_t n = 0;
        for (size_t i = 0; line[i] && line[i] != '\n' && line[i] != '\r'; i += 2)
            name[n++] = (char)(hex(line[i]) * 16 + hex(line[i + 1]));
        name[n] = 0;
        printf("%08X\n", USBA_crc32(name));
    }
    return 0;
}
"#;

fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "windows" => format!("{arch}-pc-windows-msvc"),
        "macos" => format!("{arch}-apple-darwin"),
        _ => format!("{arch}-unknown-linux-gnu"),
    }
}

/// Builds the harness with the platform C compiler (`cc` finds MSVC too).
fn compile_harness(dir: &Path) -> PathBuf {
    let system_c = std::fs::read(opl_src().join("src").join("system.c")).unwrap();
    let system_c = String::from_utf8_lossy(&system_c).replace("\r\n", "\n");
    std::fs::write(dir.join("opl_usba_crc32.c"), extract_usba_crc32(&system_c)).unwrap();
    let harness = dir.join("harness.c");
    std::fs::write(&harness, HARNESS).unwrap();

    let exe = dir.join(if cfg!(windows) {
        "harness.exe"
    } else {
        "harness"
    });
    let triple = target_triple();
    let tool = cc::Build::new()
        .cargo_metadata(false)
        .target(&triple)
        .host(&triple)
        .opt_level(0)
        .debug(false)
        .out_dir(dir)
        .get_compiler();
    let mut cmd = tool.to_command();
    cmd.current_dir(dir);
    if tool.is_like_msvc() {
        cmd.arg("/nologo")
            .arg(&harness)
            .arg(format!("/Fe{}", exe.display()));
    } else {
        // OPL relies on signed shifts wrapping, as GCC on the EE does.
        cmd.arg("-fwrapv").arg(&harness).arg("-o").arg(&exe);
    }
    let status = cmd.status().expect("run the C compiler");
    assert!(status.success(), "C compiler failed: {cmd:?}");
    exe
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
#[ignore = "needs YAOPLM_OPL_SRC and a C compiler"]
fn usba_crc32_matches_opl_c_function() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile_harness(dir.path());

    let mut names: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"A".to_vec(),
        b"Grand Theft Auto III".to_vec(),
        b"Metal Gear Solid 2 Substance".to_vec(),
        vec![b'Z'; 31],
        vec![b'Z'; 32],
        vec![b'Z'; 33],
        vec![b'Z'; 60],
        b"AG".to_vec(),
        b"f".to_vec(),
        format!("F{}", "O".repeat(31)).into_bytes(),
    ];
    let mut rng = XorShift(0x5EED_0F0C_0FFE_E001);
    for _ in 0..5000 {
        let len = rng.below(41) as usize;
        // Any 7-bit byte except NUL (the C side reads NUL-terminated names).
        names.push((0..len).map(|_| 1 + rng.below(127) as u8).collect());
    }

    let mut child = Command::new(&exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input: String = names.iter().map(|n| hex(n) + "\n").collect();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let expected: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(expected.len(), names.len());

    for (name, opl) in names.iter().zip(expected) {
        assert_eq!(
            format!("{:08X}", usba_crc32(name)),
            opl,
            "name {:?}",
            String::from_utf8_lossy(name)
        );
    }
}

fn python() -> String {
    std::env::var("YAOPLM_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}

/// Runs OPL's `pc/ziso.py` from a private copy.
fn ziso_py(dir: &Path, args: &[&Path], level: &str) {
    let script = dir.join("ziso.py");
    if !script.exists() {
        std::fs::copy(opl_src().join("pc").join("ziso.py"), &script).unwrap();
    }
    let output = Command::new(python())
        .arg(&script)
        .args(["-c", level])
        .args(args)
        .output()
        .expect("run python");
    assert!(
        output.status.success(),
        "ziso.py -c {level} failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn mixed_iso() -> Vec<u8> {
    use yaoplm_test_support::{Content, fixtures};
    fixtures::boot_iso(fixtures::placeholder_elf(), "SLUS_200.62", 0)
        .file(
            "NOISE.BIN;1",
            Content::Noise {
                len: 300_000,
                seed: 7,
            },
        )
        .file("ZEROS.BIN;1", Content::Zeros(300_000))
        .build()
        .to_vec()
}

fn read_zso(bytes: Vec<u8>) -> Vec<u8> {
    use yaoplm_formats::iso9660::SectorSource;
    let mut reader = yaoplm_formats::zso::ZsoReader::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    let mut sector = [0u8; 2048];
    for lba in 0..reader.sector_count() {
        reader.read_sector(lba, &mut sector).unwrap();
        out.extend_from_slice(&sector);
    }
    out
}

#[test]
#[ignore = "needs YAOPLM_OPL_SRC and Python with lz4"]
fn zso_interoperates_with_ziso_py() {
    let dir = tempfile::tempdir().unwrap();
    let iso = mixed_iso();
    let iso_path = dir.path().join("in.iso");
    std::fs::write(&iso_path, &iso).unwrap();

    // ziso.py output, default and high compression, read by us.
    for level in ["1", "9"] {
        let zso_path = dir.path().join(format!("py{level}.zso"));
        ziso_py(dir.path(), &[&iso_path, &zso_path], level);
        let zso = std::fs::read(&zso_path).unwrap();
        assert_eq!(read_zso(zso), iso, "ziso.py -c {level}");
    }

    // Our output, plain and with an index shift, read by ziso.py.
    for align in [0, 2] {
        let mut zso = std::io::Cursor::new(Vec::new());
        yaoplm_formats::zso::compress_aligned(
            &mut std::io::Cursor::new(&iso),
            iso.len() as u64,
            &mut zso,
            align,
            &mut |_| {},
        )
        .unwrap();
        let zso_path = dir.path().join(format!("ours{align}.zso"));
        let back = dir.path().join(format!("back{align}.iso"));
        std::fs::write(&zso_path, zso.into_inner()).unwrap();
        ziso_py(dir.path(), &[&zso_path, &back], "0");
        assert_eq!(std::fs::read(&back).unwrap(), iso, "align {align}");
    }
}

/// Builds OPL's `pc/genvmc`, the PC copy of the VMC formatter. MSVC lacks
/// the POSIX headers it includes (and does not use): empty ones stand in.
fn compile_genvmc(dir: &Path) -> PathBuf {
    let src = opl_src().join("pc").join("genvmc").join("src");
    let stubs = dir.join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();
    for header in ["unistd.h", "dirent.h"] {
        std::fs::write(stubs.join(header), "").unwrap();
    }
    for file in ["genvmc.c", "genvmc.h"] {
        std::fs::copy(src.join(file), dir.join(file)).unwrap();
    }
    let exe = dir.join(if cfg!(windows) {
        "genvmc.exe"
    } else {
        "genvmc"
    });
    let triple = target_triple();
    let tool = cc::Build::new()
        .cargo_metadata(false)
        .target(&triple)
        .host(&triple)
        .opt_level(0)
        .debug(false)
        .out_dir(dir)
        .get_compiler();
    let mut cmd = tool.to_command();
    cmd.current_dir(dir);
    if tool.is_like_msvc() {
        cmd.arg("/nologo")
            .arg(format!("/I{}", stubs.display()))
            .arg("genvmc.c")
            .arg(format!("/Fe{}", exe.display()));
    } else {
        cmd.arg("-fwrapv").arg("genvmc.c").arg("-o").arg(&exe);
    }
    let status = cmd.status().expect("run the C compiler");
    assert!(status.success(), "C compiler failed: {cmd:?}");
    exe
}

/// The `created` and `modified` times of the `.` and `..` entries: genvmc
/// stamps the moment it runs (and its PC copy reads the decimal clock as if
/// it were BCD), so they never match.
fn without_root_times(card: &mut [u8]) {
    let sb = yaoplm_formats::vmc::Superblock::parse(card);
    let root = sb.root_dir_offset() as usize;
    for entry in [root, root + 512] {
        card[entry + 0x08..entry + 0x10].fill(0);
        card[entry + 0x18..entry + 0x20].fill(0);
    }
}

#[test]
#[ignore = "needs YAOPLM_OPL_SRC and a C compiler"]
fn blank_vmc_matches_opl_genvmc() {
    use yaoplm_formats::vmc::{McTime, SIZES_MB, blank_card};
    let dir = tempfile::tempdir().unwrap();
    let exe = compile_genvmc(dir.path());
    for mb in SIZES_MB {
        let path = dir.path().join(format!("opl{mb}.bin"));
        let status = Command::new(&exe)
            .arg(mb.to_string())
            .arg(&path)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "genvmc {mb}");
        let mut opl = std::fs::read(&path).unwrap();
        let mut ours = blank_card(mb, McTime::FALLBACK).unwrap();
        assert_eq!(opl.len(), ours.len(), "{mb} MiB");
        without_root_times(&mut opl);
        without_root_times(&mut ours);
        if let Some(at) = opl.iter().zip(&ours).position(|(a, b)| a != b) {
            panic!(
                "{mb} MiB card differs at {at:#x} (cluster {}): OPL {:#04x}, ours {:#04x}",
                at / 1024,
                opl[at],
                ours[at]
            );
        }
    }
}
