//! The install manifest, `.yaoplm/manifest.json` on the device (SPEC
//! §5.10.1, LDR-3, ADR-0040): which build YAOPLM put where, with its
//! SHA-256. It never decides alone: detection takes it only while the file
//! at its path still has that hash (ADR-0040), and a manifest it cannot
//! read is as good as none.

use serde::{Deserialize, Serialize};
use yaoplm_device::{DeviceFs, RelPath};

/// Where the manifest lives; `.yaoplm/` is hidden from OPL and NHDDL
/// (ADR-0021).
pub const PATH: &str = ".yaoplm/manifest.json";
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    /// One entry per installed path.
    pub loaders: Vec<Installed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// The catalogue build: `opl-beta`.
    pub build: String,
    /// The release tag it came from; `null` for the user's own file.
    pub tag: Option<String>,
    pub version: Option<String>,
    pub sha256: String,
    /// Relative to the device root: `UPDT/OPL.ELF`.
    pub path: String,
    /// The catalogue preset, when one was used.
    pub preset: Option<String>,
    /// `usb` or `mx4sio`.
    pub device: String,
    /// The OPL language pack installed with it (`Russian`).
    #[serde(default)]
    pub language: Option<String>,
    /// The release page or the user's file.
    pub source: String,
    /// Unix seconds.
    pub installed_at: i64,
}

impl Manifest {
    /// The manifest on `fs`; `None` when there is none or it does not read.
    pub fn read(fs: &dyn DeviceFs) -> Option<(Self, Vec<u8>)> {
        let path = RelPath::new(PATH).ok()?;
        let bytes = fs.read(&path).ok()?;
        let manifest: Self = serde_json::from_slice(&bytes).ok()?;
        (manifest.schema <= SCHEMA).then_some((manifest, bytes))
    }

    /// Sets the entry for `entry.path`, keeping the others.
    pub fn put(&mut self, entry: Installed) {
        self.schema = SCHEMA;
        self.loaders
            .retain(|l| !l.path.eq_ignore_ascii_case(&entry.path));
        self.loaders.push(entry);
        self.loaders.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Drops the entry for `path`: what is there is not ours any more.
    pub fn remove(&mut self, path: &str) {
        self.loaders.retain(|l| !l.path.eq_ignore_ascii_case(path));
    }

    pub fn entry(&self, path: &str) -> Option<&Installed> {
        self.loaders
            .iter()
            .find(|l| l.path.eq_ignore_ascii_case(path))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

#[cfg(test)]
mod tests {
    use yaoplm_device::FsKind;
    use yaoplm_test_support::MemFs;

    use super::*;

    fn installed(path: &str, version: &str) -> Installed {
        Installed {
            build: "opl-beta".into(),
            tag: Some("latest".into()),
            version: Some(version.into()),
            sha256: "a".repeat(64),
            path: path.into(),
            preset: Some("fmcb".into()),
            device: "usb".into(),
            language: None,
            source: "https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest".into(),
            installed_at: 1_761_400_000,
        }
    }

    #[test]
    fn one_entry_per_path() {
        let mut manifest = Manifest::default();
        manifest.put(installed("UPDT/OPL.ELF", "v1.2.0-Beta-2271-b3a2199"));
        manifest.put(installed("APPS/OPNPS2LD.ELF", "v1.2.0-Beta-2271-b3a2199"));
        manifest.put(installed("updt/opl.elf", "v1.2.0-Beta-2280-0123abc"));
        assert_eq!(manifest.loaders.len(), 2);
        assert_eq!(
            manifest.entry("UPDT/OPL.ELF").unwrap().version.as_deref(),
            Some("v1.2.0-Beta-2280-0123abc")
        );
        assert_eq!(manifest.schema, SCHEMA);
    }

    #[test]
    fn reads_back_and_ignores_what_it_cannot_read() {
        let fs = MemFs::new(FsKind::Fat32);
        assert!(Manifest::read(&fs).is_none());
        let mut manifest = Manifest::default();
        manifest.put(installed("UPDT/OPL.ELF", "v1.1.0"));
        fs.insert_file(PATH, &manifest.to_bytes());
        assert_eq!(Manifest::read(&fs).unwrap().0, manifest);
        fs.insert_file(PATH, b"{ broken");
        assert!(Manifest::read(&fs).is_none());
        fs.insert_file(PATH, b"{\"schema\": 99, \"loaders\": []}");
        assert!(Manifest::read(&fs).is_none());
    }
}
