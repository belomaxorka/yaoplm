//! Loader releases for tests (LDR-1…LDR-4): unpacked PS2 ELFs that carry a
//! version string the way OPL builds do, and a mirror folder with GitHub's
//! layout that `yaoplm_net::loaders::ReleaseSource::Mirror` reads. Nothing
//! here is a real build; packed ELFs are `yaoplm_formats::ps2_elf`'s tests.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// An unpacked 32-bit little-endian MIPS ELF whose program holds `strings`
/// as C strings.
pub fn plain_elf(strings: &[&str]) -> Vec<u8> {
    let mut out = vec![0u8; 0x80];
    out[..6].copy_from_slice(&[0x7F, b'E', b'L', b'F', 1, 1]);
    out[0x12] = 8;
    out[0x2A] = 32;
    out[0x2C] = 2;
    for s in strings {
        out.push(0);
        out.extend_from_slice(s.as_bytes());
    }
    out.push(0);
    out
}

/// An OPL-family ELF of `version` (`v1.2.0-Beta-2271-b3a2199`).
pub fn opl_elf(version: &str) -> Vec<u8> {
    plain_elf(&["conf_opl.cfg", "Open PS2 Loader %s", version])
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// A file of a mirrored release.
pub struct MirrorAsset<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
    /// Publish GitHub's `digest` of it.
    pub digest: bool,
}

/// `<root>/repos/<owner>/<repo>/releases/tags/<tag>.json`,
/// `<root>/<owner>/<repo>/releases/download/<tag>/<asset>` and
/// `<root>/data/loaders/<file>`.
pub struct ReleaseMirror {
    pub root: PathBuf,
}

fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl ReleaseMirror {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Writes release `tag` of `repo` with its files, uploaded on `date`
    /// (`2026-10-13`).
    pub fn release(
        &self,
        repo: &str,
        tag: &str,
        name: &str,
        body: &str,
        date: &str,
        assets: &[MirrorAsset<'_>],
    ) -> io::Result<()> {
        let tags = self.root.join("repos").join(repo).join("releases/tags");
        fs::create_dir_all(&tags)?;
        let files = self.root.join(repo).join("releases/download").join(tag);
        fs::create_dir_all(&files)?;
        let mut list = Vec::new();
        for asset in assets {
            fs::write(files.join(asset.name), asset.bytes)?;
            let digest = if asset.digest {
                json_string(&format!("sha256:{}", sha256(asset.bytes)))
            } else {
                "null".to_owned()
            };
            list.push(format!(
                "{{\"name\":{},\"size\":{},\"digest\":{digest},\"updated_at\":\"{date}T04:07:08Z\",\"browser_download_url\":{}}}",
                json_string(asset.name),
                asset.bytes.len(),
                json_string(&format!(
                    "https://github.com/{repo}/releases/download/{tag}/{}",
                    asset.name
                )),
            ));
        }
        let json = format!(
            "{{\"tag_name\":{},\"name\":{},\"body\":{},\"html_url\":{},\"prerelease\":true,\"draft\":false,\"published_at\":\"{date}T04:07:00Z\",\"assets\":[{}]}}",
            json_string(tag),
            json_string(name),
            json_string(body),
            json_string(&format!("https://github.com/{repo}/releases/tag/{tag}")),
            list.join(","),
        );
        fs::write(tags.join(format!("{tag}.json")), json)
    }

    /// A file of the catalogue on `main`: `loaders/index.toml`.
    pub fn catalog_file(&self, path: &str, text: &str) -> io::Result<()> {
        let file = self.root.join("data").join(path);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(file, text)
    }

    /// The three catalogue builds as GitHub publishes them (research/17
    /// §6–§7), with test ELFs: `latest` of upstream (digests), `latest` of
    /// YAOPL (digests and `SHA256SUMS`) and upstream's `v1.1.0` (no
    /// digests; its archive holds the ELF). Returns the 1.1.0 archive:
    /// the catalogue pins the real one's hash, so tests that download it
    /// pin this one through a catalogue of their own.
    pub fn official(&self) -> io::Result<Vec<u8>> {
        let beta = opl_elf("v1.2.0-Beta-2271-b3a2199");
        let beta_7z =
            crate::packed::seven_zip(&[("OPNPS2LD/OPNPS2LD-v1.2.0-Beta-2271-b3a2199.ELF", &beta)])?;
        self.release(
            "ps2homebrew/Open-PS2-Loader",
            "latest",
            "latest",
            "## Documentation\n* Add a note by @someone in https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199\n",
            "2026-10-13",
            &[
                MirrorAsset { name: "OPNPS2LD.ELF", bytes: &beta, digest: true },
                MirrorAsset { name: "OPNPS2LD.7z", bytes: &beta_7z, digest: true },
                MirrorAsset { name: "OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z", bytes: &beta_7z, digest: true },
            ],
        )?;
        let yaopl = opl_elf("v1.2.0-Beta-2370-33a2b19-yaopl");
        let sums = format!("{}  OPNPS2LD.ELF\n", sha256(&yaopl));
        self.release(
            "belomaxorka/yaopl",
            "latest",
            "v1.2.0-Beta-2370-33a2b19-yaopl",
            "Automatic YAOPL build of v1.2.0-Beta-2370-33a2b19-yaopl.\n\nSHA-256 checksums of the files are in SHA256SUMS.\n",
            "2026-10-23",
            &[
                MirrorAsset { name: "OPNPS2LD.ELF", bytes: &yaopl, digest: true },
                MirrorAsset { name: "SHA256SUMS", bytes: sums.as_bytes(), digest: true },
            ],
        )?;
        let stable = opl_elf("v1.1.0");
        let archive = crate::packed::seven_zip(&[
            ("OPNPS2LD/LICENSE", b"Academic Free License 3.0".as_slice()),
            ("OPNPS2LD/OPNPS2LD-v1.1.0.ELF", &stable),
        ])?;
        self.release(
            "ps2homebrew/Open-PS2-Loader",
            "v1.1.0",
            "v1.1.0",
            "New in OPL v1.1.0\n- GCC 11 toolchain\n- NBD server\n",
            "2021-09-09",
            &[MirrorAsset {
                name: "OPNPS2LD-v1.1.0.7z",
                bytes: &archive,
                digest: false,
            }],
        )?;
        Ok(archive)
    }
}
