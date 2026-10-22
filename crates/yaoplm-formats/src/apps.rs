//! Homebrew apps: `APPS/<folder>/title.cfg` and legacy `conf_apps.cfg`.
//!
//! Format reference: `docs/research/02-opl-formats.md` §11. Mirrors
//! `appScanCallback()` and the legacy list reader in `src/appsupport.c`,
//! `oplScanApps()` in `src/opl.c` and the limits in `include/appsupport.h`
//! of ps2homebrew/Open-PS2-Loader@b3a21997.
//!
//! - `title.cfg` uses the normal config syntax ([`crate::cfg`]); keys are
//!   lower case and case-sensitive. `title` and `boot` must both exist
//!   (even empty), `argv1` is optional.
//! - OPL truncates title and argv1 to 128 bytes and boot to 64.
//! - `boot` is a file name inside the app folder; its name is the art key.
//! - Legacy `conf_apps.cfg` lines are `Title=device:path/FILE.ELF`.

use crate::cfg::{CfgDocument, CfgError};

pub const APPS_FOLDER: &str = "APPS";
pub const TITLE_CFG: &str = "title.cfg";
pub const TITLE_MAX: usize = 128;
pub const BOOT_MAX: usize = 64;
pub const ARGV1_MAX: usize = 128;
/// `APP_PATH_MAX`: longest folder path OPL keeps.
pub const PATH_MAX: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    pub title: String,
    pub boot: String,
    pub argv1: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    #[error("title.cfg has no title= key")]
    MissingTitle,
    #[error("title.cfg has no boot= key")]
    MissingBoot,
    #[error("boot must be a plain ELF file name, got {0:?}")]
    BadBoot(String),
    #[error("{field} is {len} bytes; OPL keeps {max}")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error(transparent)]
    Cfg(#[from] CfgError),
}

fn cut(value: &str, max: usize) -> String {
    let mut end = value.len().min(max);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Reads `title.cfg` as OPL lists the app.
pub fn parse_title_cfg(bytes: &[u8]) -> Result<AppEntry, AppError> {
    let doc = CfgDocument::parse(bytes);
    let title = doc.get("title").ok_or(AppError::MissingTitle)?;
    let boot = doc.get("boot").ok_or(AppError::MissingBoot)?;
    Ok(AppEntry {
        title: cut(&title, TITLE_MAX),
        boot: cut(&boot, BOOT_MAX),
        argv1: doc.get("argv1").map(|a| cut(&a, ARGV1_MAX)),
    })
}

/// Writes a new `title.cfg` (CRLF, OPL key order).
pub fn build_title_cfg(entry: &AppEntry) -> Result<Vec<u8>, AppError> {
    let checks = [
        ("title", entry.title.len(), TITLE_MAX),
        ("boot", entry.boot.len(), BOOT_MAX),
        (
            "argv1",
            entry.argv1.as_ref().map_or(0, String::len),
            ARGV1_MAX,
        ),
    ];
    for (field, len, max) in checks {
        if len > max {
            return Err(AppError::TooLong { field, len, max });
        }
    }
    if entry.boot.is_empty() || entry.boot.contains(['/', '\\', ':']) {
        return Err(AppError::BadBoot(entry.boot.clone()));
    }
    let mut doc = CfgDocument::new();
    doc.set("title", &entry.title)?;
    doc.set("boot", &entry.boot)?;
    if let Some(argv1) = &entry.argv1 {
        doc.set("argv1", argv1)?;
    }
    Ok(doc.to_bytes())
}

/// One line of a legacy `conf_apps.cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyApp {
    pub title: String,
    /// Full path, e.g. `mass:/APPS/WLE.ELF`; OPL rewrites `mass:` to every
    /// BDM unit.
    pub path: String,
}

impl LegacyApp {
    /// The ELF file name: the art key and the `CFG/<ELF>.cfg` name.
    pub fn elf_name(&self) -> &str {
        let after_device = self
            .path
            .rsplit_once(':')
            .map_or(self.path.as_str(), |(_, p)| p);
        after_device
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(after_device)
    }
}

pub fn parse_conf_apps(bytes: &[u8]) -> Vec<LegacyApp> {
    CfgDocument::parse(bytes)
        .entries()
        .into_iter()
        .map(|e| LegacyApp {
            title: e.key,
            path: e.value,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_title_cfg() {
        let app = parse_title_cfg(b"title=wLaunchELF\r\nboot=WLE.ELF\r\n").unwrap();
        assert_eq!(app.title, "wLaunchELF");
        assert_eq!(app.boot, "WLE.ELF");
        assert_eq!(app.argv1, None);

        // Keys are case-sensitive; `Title=` does not count.
        assert_eq!(
            parse_title_cfg(b"Title=x\nboot=a.elf\n"),
            Err(AppError::MissingTitle)
        );
        assert_eq!(parse_title_cfg(b"title=x\n"), Err(AppError::MissingBoot));
        // Empty values are accepted.
        assert_eq!(parse_title_cfg(b"title=\nboot=\n").unwrap().title, "");
    }

    #[test]
    fn truncates_like_opl() {
        let cfg = format!(
            "title={}\nboot={}.ELF\nargv1=x\n",
            "T".repeat(200),
            "B".repeat(100)
        );
        let app = parse_title_cfg(cfg.as_bytes()).unwrap();
        assert_eq!(app.title.len(), TITLE_MAX);
        assert_eq!(app.boot.len(), BOOT_MAX);
        assert_eq!(app.argv1.as_deref(), Some("x"));
    }

    #[test]
    fn writes_title_cfg() {
        let entry = AppEntry {
            title: "Open PS2 Loader".into(),
            boot: "OPNPS2LD.ELF".into(),
            argv1: None,
        };
        let bytes = build_title_cfg(&entry).unwrap();
        assert_eq!(bytes, b"title=Open PS2 Loader\r\nboot=OPNPS2LD.ELF\r\n");
        assert_eq!(parse_title_cfg(&bytes).unwrap(), entry);

        let bad = AppEntry {
            boot: "sub/APP.ELF".into(),
            ..entry
        };
        assert!(matches!(build_title_cfg(&bad), Err(AppError::BadBoot(_))));
    }

    #[test]
    fn legacy_list() {
        let apps = parse_conf_apps(b"wLaunchELF=mass:/APPS/WLE.ELF\r\nSMS=mc0:SMS/SMS.ELF\r\n");
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].elf_name(), "WLE.ELF");
        assert_eq!(apps[1].elf_name(), "SMS.ELF");
    }
}
