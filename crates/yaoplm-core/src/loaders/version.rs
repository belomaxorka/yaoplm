//! Version strings of OPL builds: `v1.1.0`, `v1.2.0-Beta-2271-b3a2199`,
//! `v1.2.0-Beta-2370-33a2b19-yaopl` (upstream `Makefile` `OPL_VERSION`,
//! YAOPL `LOCALVERSION`; research/16 §1.1, §2).
//!
//! The beta number is `git rev-list --count HEAD` + 2 of the repository the
//! build comes from, so YAOPL and upstream reuse the same numbers for other
//! code: numbers compare only inside one channel (research/17 §9 g).

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVersion {
    /// `1.2.0`.
    pub release: [u32; 3],
    /// The beta's build number.
    pub revision: Option<u32>,
    /// The commit after the build number, 7 hex digits.
    pub commit: Option<String>,
    /// What a fork appends: `yaopl`, `nosio2man`.
    pub suffix: Vec<String>,
}

impl BuildVersion {
    /// `None` for anything that is not a version string: a tag like
    /// `latest`, a variant name with `PADEMU=1`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.strip_prefix('v')?.split('-');
        let mut numbers = parts.next()?.split('.');
        let mut release = [0; 3];
        for slot in &mut release {
            *slot = number(numbers.next()?)?;
        }
        if numbers.next().is_some() {
            return None;
        }
        let mut rest: Vec<&str> = parts.collect();
        let (mut revision, mut commit) = (None, None);
        if rest.first() == Some(&"Beta") {
            let (Some(n), Some(hash)) = (rest.get(1), rest.get(2)) else {
                return None;
            };
            if hash.len() != 7 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            revision = Some(number(n)?);
            commit = Some(hash.to_ascii_lowercase());
            rest.drain(..3);
        }
        if rest
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return None;
        }
        Some(Self {
            release,
            revision,
            commit,
            suffix: rest.into_iter().map(str::to_owned).collect(),
        })
    }

    /// How two builds of one channel order: by release, then by build
    /// number. `None` when they cannot be ordered: other suffixes, or the
    /// same number with another commit (a rewritten history).
    pub fn channel_order(&self, other: &Self) -> Option<Ordering> {
        if self.suffix != other.suffix {
            return None;
        }
        let order = self
            .release
            .cmp(&other.release)
            .then(self.revision.cmp(&other.revision));
        if order == Ordering::Equal && self.commit != other.commit {
            return None;
        }
        Some(order)
    }
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

impl fmt::Display for BuildVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.release;
        write!(f, "v{major}.{minor}.{patch}")?;
        if let (Some(revision), Some(commit)) = (self.revision, &self.commit) {
            write!(f, "-Beta-{revision}-{commit}")?;
        }
        for part in &self.suffix {
            write!(f, "-{part}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_releases_carry() {
        let stable = BuildVersion::parse("v1.1.0").unwrap();
        assert_eq!(stable.release, [1, 1, 0]);
        assert_eq!(stable.revision, None);

        let beta = BuildVersion::parse("v1.2.0-Beta-2271-b3a2199").unwrap();
        assert_eq!(beta.revision, Some(2271));
        assert_eq!(beta.commit.as_deref(), Some("b3a2199"));
        assert!(beta.suffix.is_empty());

        let fork = BuildVersion::parse("v1.2.0-Beta-2370-33a2b19-yaopl").unwrap();
        assert_eq!(fork.suffix, ["yaopl"]);
        for text in [
            "v1.1.0",
            "v1.2.0-Beta-2271-b3a2199",
            "v1.2.0-Beta-2370-33a2b19-yaopl",
            "v1.2.0-Beta-2370-33a2b19-yaopl-nosio2man",
        ] {
            assert_eq!(BuildVersion::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn refuses_what_is_not_a_version() {
        for text in [
            "latest",
            "1.1.0",
            "v1.1",
            "v1.1.0.1",
            "v1.2.0-Beta-2271",
            "v1.2.0-Beta-x-b3a2199",
            "v1.2.0-Beta-2271-b3a21",
            "v1.2.0-Beta-2271-b3a2199-PADEMU=1",
            "v1.2.0--x",
            "",
        ] {
            assert_eq!(BuildVersion::parse(text), None, "{text}");
        }
    }

    #[test]
    fn orders_only_inside_a_channel() {
        let v = |text| BuildVersion::parse(text).unwrap();
        let old = v("v1.2.0-Beta-2271-b3a2199");
        let new = v("v1.2.0-Beta-2280-0123abc");
        assert_eq!(new.channel_order(&old), Some(Ordering::Greater));
        assert_eq!(old.channel_order(&new), Some(Ordering::Less));
        assert_eq!(old.channel_order(&old), Some(Ordering::Equal));
        assert_eq!(v("v1.2.0-Beta-2271-1234567").channel_order(&old), None);
        assert_eq!(
            v("v1.2.0-Beta-2370-33a2b19-yaopl").channel_order(&old),
            None
        );
        assert_eq!(old.channel_order(&v("v1.1.0")), Some(Ordering::Greater));
    }
}
