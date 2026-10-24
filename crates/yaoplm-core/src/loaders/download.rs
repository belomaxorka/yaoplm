//! A downloaded release file held to what the user confirmed, and the
//! loader ELF taken out of it (SPEC LDR-2, ADR-0042): the size the release
//! gives, the SHA-256 shown before the download, then the version inside
//! the ELF (ADR-0041) against the release's. A file that fails any check is
//! never stored.

use std::io::{Cursor, Read};

use yaoplm_formats::archive::{self, Kind};
use yaoplm_formats::ps2_elf;
use yaoplm_i18n::Phrase;

use super::LoaderError;
use super::release::Offer;

/// Loader files are a few megabytes; nothing near this is one.
pub const MAX_FILE: u64 = 64 << 20;

/// A loader ELF ready to go onto a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderElf {
    pub bytes: Vec<u8>,
    pub sha256: String,
    /// The version inside the ELF, when it can be read.
    pub version: Option<String>,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    // Reading a slice cannot fail.
    yaoplm_hash::sha256_reader(bytes).unwrap_or_default()
}

/// Takes `member` out of the archive `name`, at most [`MAX_FILE`] bytes.
pub(crate) fn extract(name: &str, bytes: &[u8], member: &str) -> Result<Vec<u8>, LoaderError> {
    let unreadable = |error: String| {
        LoaderError::new(
            Phrase::error("loaderArchiveUnreadable")
                .with("archive", name)
                .with("error", error),
        )
    };
    let kind = archive_kind(name, bytes).ok_or_else(|| unreadable("not a zip or 7z".into()))?;
    let mut reader = archive::open_entry(kind, Box::new(Cursor::new(bytes.to_vec())), member)
        .map_err(|e| match e {
            archive::ArchiveError::NotFound(_) => LoaderError::new(
                Phrase::error("loaderArchiveMember")
                    .with("archive", name)
                    .with("member", member),
            ),
            other => unreadable(other.to_string()),
        })?;
    let mut out = Vec::new();
    (&mut reader)
        .take(MAX_FILE + 1)
        .read_to_end(&mut out)
        .map_err(|e| unreadable(e.to_string()))?;
    if out.len() as u64 > MAX_FILE {
        return Err(LoaderError::new(
            Phrase::error("loaderFileTooLarge").with("file", member),
        ));
    }
    Ok(out)
}

/// A zip or 7z by its name, or by its first bytes.
pub(crate) fn archive_kind(name: &str, bytes: &[u8]) -> Option<Kind> {
    let by_name = name
        .rsplit_once('.')
        .and_then(|(_, ext)| Kind::from_extension(ext));
    by_name.or(if bytes.starts_with(b"PK\x03\x04") {
        Some(Kind::Zip)
    } else if bytes.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(Kind::SevenZip)
    } else {
        None
    })
}

/// Reads `bytes` as a loader ELF called `name`; with `expected`, its
/// version must be that one when it can be read.
pub(crate) fn elf(
    name: &str,
    bytes: Vec<u8>,
    expected: Option<&str>,
) -> Result<LoaderElf, LoaderError> {
    let version = ps2_elf::read_version(&bytes)
        .map_err(|_| LoaderError::new(Phrase::error("loaderNotPs2Elf").with("file", name)))?
        .map(|v| v.version);
    if let (Some(found), Some(expected)) = (&version, expected)
        && found != expected
    {
        return Err(LoaderError::new(
            Phrase::error("loaderVersionMismatch")
                .with("file", name)
                .with("found", found)
                .with("expected", expected),
        ));
    }
    Ok(LoaderElf {
        sha256: sha256(&bytes),
        bytes,
        version,
    })
}

/// Holds a downloaded file to `offer` and takes its ELF out.
pub fn check(offer: &Offer, bytes: &[u8]) -> Result<LoaderElf, LoaderError> {
    let file = &offer.file;
    if bytes.len() as u64 != file.size {
        return Err(LoaderError::new(
            Phrase::error("loaderDownloadSize")
                .with("asset", &file.name)
                .with("expected", file.size)
                .with("actual", bytes.len() as u64),
        ));
    }
    if let Some(expected) = &file.sha256
        && *expected != sha256(bytes)
    {
        return Err(LoaderError::new(
            Phrase::error("loaderDownloadChecksum").with("asset", &file.name),
        ));
    }
    match &offer.member {
        Some(member) => elf(
            member,
            extract(&file.name, bytes, member)?,
            Some(&offer.version),
        ),
        None => elf(&file.name, bytes.to_vec(), Some(&offer.version)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;
    use crate::loaders::built_in;
    use crate::loaders::release::tests::{beta_release, stable_release};
    use crate::loaders::release::{FileOffer, offer};

    /// An unpacked PS2 ELF whose program holds `strings` (packed ones are
    /// `yaoplm_formats::ps2_elf`'s tests).
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

    pub fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut out);
            for (name, bytes) in files {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        out.into_inner()
    }

    /// The beta's offer, held to `elf`.
    fn beta_offer(elf: &[u8]) -> Offer {
        let mut offer = offer(
            built_in().unwrap().build("opl-beta").unwrap(),
            &beta_release(),
            None,
        )
        .unwrap();
        offer.file = FileOffer {
            size: elf.len() as u64,
            sha256: Some(sha256(elf)),
            ..offer.file
        };
        offer
    }

    #[test]
    fn a_confirmed_elf_passes() {
        let elf_bytes = plain_elf(&["conf_opl.cfg", "v1.2.0-Beta-2271-b3a2199"]);
        let got = check(&beta_offer(&elf_bytes), &elf_bytes).unwrap();
        assert_eq!(got.version.as_deref(), Some("v1.2.0-Beta-2271-b3a2199"));
        assert_eq!(got.sha256, sha256(&elf_bytes));
        assert_eq!(got.bytes, elf_bytes);
    }

    #[test]
    fn what_was_not_confirmed_is_refused() {
        let elf_bytes = plain_elf(&["conf_opl.cfg", "v1.2.0-Beta-2271-b3a2199"]);
        let offer = beta_offer(&elf_bytes);
        let key = |bytes: &[u8]| check(&offer, bytes).unwrap_err().phrase.key;
        assert_eq!(key(&elf_bytes[1..]), "errors.loaderDownloadSize");
        let mut other = elf_bytes.clone();
        *other.last_mut().unwrap() = 1;
        assert_eq!(key(&other), "errors.loaderDownloadChecksum");

        // The right size and hash, the wrong build inside.
        let wrong = plain_elf(&["conf_opl.cfg", "v1.2.0-Beta-2270-1234567"]);
        let mut offer = beta_offer(&wrong);
        offer.version = "v1.2.0-Beta-2271-b3a2199".into();
        assert_eq!(
            check(&offer, &wrong).unwrap_err().phrase.key,
            "errors.loaderVersionMismatch"
        );

        let not_elf = b"MZ not a PS2 program".to_vec();
        let offer = beta_offer(&not_elf);
        assert_eq!(
            check(&offer, &not_elf).unwrap_err().phrase.key,
            "errors.loaderNotPs2Elf"
        );
    }

    #[test]
    fn an_unreadable_version_is_no_mismatch() {
        // A packer YAOPLM cannot read yet: the hash still holds it.
        let elf_bytes = plain_elf(&["conf_opl.cfg"]);
        assert_eq!(
            check(&beta_offer(&elf_bytes), &elf_bytes).unwrap().version,
            None
        );
    }

    #[test]
    fn the_stable_elf_comes_out_of_its_archive() {
        let elf_bytes = plain_elf(&["conf_opl.cfg", "Open PS2 Loader %s", "v1.1.0"]);
        let archive = zip(&[
            ("OPNPS2LD/LICENSE", b"AFL-3.0".as_slice()),
            ("OPNPS2LD/OPNPS2LD-v1.1.0.ELF", &elf_bytes),
        ]);
        let mut offer = offer(
            built_in().unwrap().build("opl-stable").unwrap(),
            &stable_release(),
            None,
        )
        .unwrap();
        offer.file.name = "OPNPS2LD-v1.1.0.zip".into();
        offer.file.size = archive.len() as u64;
        offer.file.sha256 = Some(sha256(&archive));
        let got = check(&offer, &archive).unwrap();
        assert_eq!(got.bytes, elf_bytes);
        assert_eq!(got.version.as_deref(), Some("v1.1.0"));

        offer.member = Some("OPNPS2LD/OPNPS2LD.ELF".into());
        assert_eq!(
            check(&offer, &archive).unwrap_err().phrase.key,
            "errors.loaderArchiveMember"
        );
    }
}
