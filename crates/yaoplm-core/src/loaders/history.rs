//! Hash histories: `data/loader-hashes/<id>.txt`, one line per build ever
//! seen, `version sha256 release-date file`, `#` for comments (SPEC
//! §5.10.1, ADR-0022). Floating tags replace their assets in place, so a
//! build stays recognisable only through these lines; they are only ever
//! added, by `.github/workflows/loader-hashes.yml` or by hand.

use super::CatalogError;
use super::version::BuildVersion;

/// A build whose SHA-256 is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBuild {
    pub version: String,
    /// Lowercase hex.
    pub sha256: String,
    /// `YYYY-MM-DD`: the release, or the upload of the asset.
    pub date: String,
    /// The asset or archive member that was hashed.
    pub file: String,
}

impl KnownBuild {
    pub fn parsed_version(&self) -> Option<BuildVersion> {
        BuildVersion::parse(&self.version)
    }
}

pub(super) fn parse(file: &str, text: &str) -> Result<Vec<KnownBuild>, CatalogError> {
    let mut builds = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |what: &str| CatalogError::new(file, format!("line {}: {what}", index + 1));
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [version, sha256, date, name] = fields[..] else {
            return Err(bad("expected `version sha256 date file`"));
        };
        if BuildVersion::parse(version).is_none() {
            return Err(bad("not a version string"));
        }
        if !is_sha256(sha256) {
            return Err(bad("not a lowercase SHA-256"));
        }
        if !is_date(date) {
            return Err(bad("not a YYYY-MM-DD date"));
        }
        let build = KnownBuild {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
            date: date.to_owned(),
            file: name.to_owned(),
        };
        if builds.contains(&build) {
            return Err(bad("repeats an earlier line"));
        }
        builds.push(build);
    }
    Ok(builds)
}

pub(super) fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "4b6d36eff917bbeb3d860fea7ca0b3dd38502c4f5b008803a1edf3f0edf12290";

    #[test]
    fn reads_lines_and_skips_comments() {
        let text = format!("# header\n\nv1.2.0-Beta-2271-b3a2199 {SHA} 2026-10-13 OPNPS2LD.ELF\n");
        let builds = parse("t.txt", &text).unwrap();
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].version, "v1.2.0-Beta-2271-b3a2199");
        assert_eq!(builds[0].file, "OPNPS2LD.ELF");
        assert_eq!(builds[0].parsed_version().unwrap().revision, Some(2271));
    }

    #[test]
    fn refuses_broken_lines() {
        for line in [
            format!("v1.2.0 {SHA} 2026-10-13"),
            format!("latest {SHA} 2026-10-13 OPNPS2LD.ELF"),
            format!("v1.1.0 {} 2026-10-13 OPNPS2LD.ELF", SHA.to_uppercase()),
            format!("v1.1.0 {} 2026-10-13 OPNPS2LD.ELF", &SHA[1..]),
            format!("v1.1.0 {SHA} 13.10.2026 OPNPS2LD.ELF"),
        ] {
            let err = parse("t.txt", &line).unwrap_err();
            assert!(err.to_string().starts_with("t.txt: line 1:"), "{err}");
        }
        let twice = format!("v1.1.0 {SHA} 2021-09-09 A.ELF\nv1.1.0 {SHA} 2021-09-09 A.ELF\n");
        assert!(
            parse("t.txt", &twice)
                .unwrap_err()
                .to_string()
                .contains("line 2")
        );
        // The same ELF under two names is fine: 1.1.0's variants share hashes.
        let renamed = format!("v1.1.0 {SHA} 2021-09-09 A.ELF\nv1.1.0 {SHA} 2021-09-09 B.ELF\n");
        assert_eq!(parse("t.txt", &renamed).unwrap().len(), 2);
    }
}
