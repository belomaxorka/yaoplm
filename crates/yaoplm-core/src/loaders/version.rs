//! Version strings of OPL builds: `v1.1.0`, `v1.2.0-Beta-2271-b3a2199`,
//! `v1.2.0-Beta-2370-33a2b19-yaopl` (upstream `Makefile` `OPL_VERSION`,
//! YAOPL `LOCALVERSION`; research/16 §1.1, §2), YAOPL's own versions
//! `v2.0.0-nightly.12-0123abc-yaopl`, `v2.0.0-rc1-yaopl`, `v2.0.0-yaopl`
//! (YAOPL `Makefile` `YAOPL_VERSION`, research/16 §2), and the two-part
//! releases before 1.0 (`v0.9`) that `data/loader-hashes/opl-stable.txt`
//! names.
//!
//! The beta number is `git rev-list --count HEAD` + 2 of the repository the
//! build comes from, so YAOPL and upstream reuse the same numbers for other
//! code: numbers compare only inside one channel (research/17 §9 g).

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVersion {
    /// `[1, 2, 0]`; two or three numbers.
    pub release: Vec<u32>,
    /// What the build is on the way to `release`; `None` for the release.
    pub stage: Option<Stage>,
    /// The commit after the build number, 7 hex digits.
    pub commit: Option<String>,
    /// What a fork appends: `yaopl`, `nosio2man`; a local build's `dirty`.
    pub suffix: Vec<String>,
}

/// A build before its release, in the order they come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// `Beta-<N>-<commit>`: upstream's builds, and YAOPL's before it
    /// numbered its own versions.
    Beta(u32),
    /// `nightly.<N>-<commit>`: a YAOPL build towards the release, N commits
    /// on `master` after the previous release.
    Nightly(u32),
    /// `rc<N>`: a release candidate.
    Rc(u32),
}

impl BuildVersion {
    /// `None` for anything that is not a version string: a tag like
    /// `latest`, a variant name with `PADEMU=1`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.strip_prefix('v')?.split('-');
        let release = parts
            .next()?
            .split('.')
            .map(number)
            .collect::<Option<Vec<u32>>>()?;
        if !(2..=3).contains(&release.len()) {
            return None;
        }
        let mut rest: Vec<&str> = parts.collect();
        let (mut stage, mut commit) = (None, None);
        // A counted build, and how many parts it takes with its commit.
        let counted = match rest.first() {
            Some(&"Beta") => Some((Stage::Beta(number(rest.get(1)?)?), 3)),
            Some(part) => match part.strip_prefix("nightly.") {
                Some(n) => Some((Stage::Nightly(number(n)?), 2)),
                None => None,
            },
            None => None,
        };
        if let Some((counted, len)) = counted {
            let hash = rest.get(len - 1)?;
            if hash.len() != 7 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            stage = Some(counted);
            commit = Some(hash.to_ascii_lowercase());
            rest.drain(..len);
        } else if let Some(n) = rest
            .first()
            .and_then(|p| p.strip_prefix("rc"))
            .and_then(number)
        {
            stage = Some(Stage::Rc(n));
            rest.drain(..1);
        }
        if rest
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return None;
        }
        Some(Self {
            release,
            stage,
            commit,
            suffix: rest.into_iter().map(str::to_owned).collect(),
        })
    }

    /// How two builds of one channel order: by release, then the builds
    /// before it by number, its candidates, the release itself. `None` when
    /// they cannot be ordered: other suffixes, a beta against a nightly, or
    /// the same number with another commit (a rewritten history).
    pub fn channel_order(&self, other: &Self) -> Option<Ordering> {
        if self.suffix != other.suffix {
            return None;
        }
        let order = match self.release.cmp(&other.release) {
            Ordering::Equal => stage_order(self.stage, other.stage)?,
            order => return Some(order),
        };
        if order == Ordering::Equal && self.commit != other.commit {
            return None;
        }
        Some(order)
    }
}

fn stage_order(a: Option<Stage>, b: Option<Stage>) -> Option<Ordering> {
    let rank = |stage| match stage {
        Some(Stage::Beta(n) | Stage::Nightly(n)) => (0, n),
        Some(Stage::Rc(n)) => (1, n),
        None => (2, 0),
    };
    match (a, b) {
        (Some(Stage::Beta(_)), Some(Stage::Nightly(_)))
        | (Some(Stage::Nightly(_)), Some(Stage::Beta(_))) => None,
        _ => Some(rank(a).cmp(&rank(b))),
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
        let release: Vec<String> = self.release.iter().map(u32::to_string).collect();
        write!(f, "v{}", release.join("."))?;
        match (self.stage, &self.commit) {
            (Some(Stage::Beta(n)), Some(commit)) => write!(f, "-Beta-{n}-{commit}")?,
            (Some(Stage::Nightly(n)), Some(commit)) => write!(f, "-nightly.{n}-{commit}")?,
            (Some(Stage::Rc(n)), _) => write!(f, "-rc{n}")?,
            _ => {}
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
        assert_eq!(stable.stage, None);

        let beta = BuildVersion::parse("v1.2.0-Beta-2271-b3a2199").unwrap();
        assert_eq!(beta.stage, Some(Stage::Beta(2271)));
        assert_eq!(beta.commit.as_deref(), Some("b3a2199"));
        assert!(beta.suffix.is_empty());

        let fork = BuildVersion::parse("v1.2.0-Beta-2370-33a2b19-yaopl").unwrap();
        assert_eq!(fork.suffix, ["yaopl"]);
        let old = BuildVersion::parse("v0.9").unwrap();
        assert_eq!(old.release, [0, 9]);
        for text in [
            "v0.5",
            "v0.9.1",
            "v1.1.0",
            "v1.2.0-Beta-2271-b3a2199",
            "v1.2.0-Beta-2370-33a2b19-yaopl",
            "v1.2.0-Beta-2370-33a2b19-yaopl-nosio2man",
            "v1.2.0-Beta-2370-33a2b19-dirty-yaopl",
            "v1.0.0-rc1",
            "v2.0.0-nightly.12-0123abc-yaopl",
            "v2.0.0-nightly.12-0123abc-yaopl-nosio2man",
            "v2.0.0-nightly.12-0123abc-dirty-yaopl",
            "v2.0.0-rc1-yaopl",
            "v2.0.0-yaopl",
            "v2.0.0-dirty-yaopl",
        ] {
            assert_eq!(BuildVersion::parse(text).unwrap().to_string(), text);
        }
    }

    /// YAOPL's own versions (its `Makefile`, `YAOPL_VERSION`, at a9772842 of
    /// `release/versioning`).
    #[test]
    fn parses_yaopl_versions() {
        let nightly = BuildVersion::parse("v2.0.0-nightly.12-0123ABC-yaopl").unwrap();
        assert_eq!(nightly.release, [2, 0, 0]);
        assert_eq!(nightly.stage, Some(Stage::Nightly(12)));
        assert_eq!(nightly.commit.as_deref(), Some("0123abc"));
        assert_eq!(nightly.suffix, ["yaopl"]);

        let candidate = BuildVersion::parse("v2.0.0-rc1-yaopl").unwrap();
        assert_eq!(candidate.stage, Some(Stage::Rc(1)));
        assert_eq!(candidate.commit, None);
        assert_eq!(candidate.suffix, ["yaopl"]);

        let release = BuildVersion::parse("v2.0.0-yaopl-nosio2man").unwrap();
        assert_eq!(release.stage, None);
        assert_eq!(release.suffix, ["yaopl", "nosio2man"]);

        // `rc` without a number is a suffix like any other.
        let other = BuildVersion::parse("v2.0.0-rcx-yaopl").unwrap();
        assert_eq!(other.stage, None);
        assert_eq!(other.suffix, ["rcx", "yaopl"]);
    }

    /// Every version the frozen hash table names, which detection reports.
    #[test]
    fn reads_every_version_of_the_frozen_table() {
        let table = include_str!("../../../../data/loader-hashes/opl-stable.txt");
        let versions: Vec<&str> = table
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        assert!(versions.len() > 30);
        for version in versions {
            assert!(BuildVersion::parse(version).is_some(), "{version}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_version() {
        for text in [
            "latest",
            "1.1.0",
            "v1",
            "v1.1.0.1",
            "v1.2.0-Beta-2271",
            "v1.2.0-Beta-x-b3a2199",
            "v1.2.0-Beta-2271-b3a21",
            "v1.2.0-Beta-2271-b3a2199-PADEMU=1",
            "v1.2.0--x",
            "v2.0.0-nightly.-0123abc-yaopl",
            "v2.0.0-nightly.x-0123abc-yaopl",
            "v2.0.0-nightly.12-yaopl",
            "v2.0.0-nightly.12",
            "v2.0.0-nightly.1.2-0123abc-yaopl",
            "v2.0.0-Nightly.12-0123abc-yaopl",
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
        assert_eq!(
            v("v0.9.1").channel_order(&v("v0.9")),
            Some(Ordering::Greater)
        );
        assert_eq!(
            v("v1.0.0").channel_order(&v("v1.0.0-rc1")),
            Some(Ordering::Greater)
        );
    }

    /// Old beta builds, then the nightlies towards 2.0.0 by number, its
    /// candidates, 2.0.0, and the nightlies towards 2.1.0.
    #[test]
    fn orders_yaopl_versions() {
        let ascending = [
            "v1.2.0-Beta-2370-33a2b19-yaopl",
            "v1.2.0-Beta-2420-03edb47-yaopl",
            "v2.0.0-nightly.1-0123abc-yaopl",
            "v2.0.0-nightly.2-4567def-yaopl",
            "v2.0.0-nightly.10-89abcde-yaopl",
            "v2.0.0-rc1-yaopl",
            "v2.0.0-rc2-yaopl",
            "v2.0.0-yaopl",
            "v2.1.0-nightly.1-fedcba9-yaopl",
            "v2.1.0-yaopl",
        ];
        let v = |text| BuildVersion::parse(text).unwrap();
        for (i, a) in ascending.iter().enumerate() {
            for (j, b) in ascending.iter().enumerate() {
                assert_eq!(v(a).channel_order(&v(b)), Some(i.cmp(&j)), "{a} {b}");
            }
        }
        let nightly = v("v2.0.0-nightly.2-4567def-yaopl");
        assert_eq!(
            v("v2.0.0-nightly.2-0000000-yaopl").channel_order(&nightly),
            None
        );
        // The A/B build and a local one are other channels.
        for other in [
            "v2.0.0-nightly.1-0123abc-yaopl-nosio2man",
            "v2.0.0-nightly.1-0123abc-dirty-yaopl",
            "v2.0.0-nightly.1-0123abc",
        ] {
            assert_eq!(v(other).channel_order(&nightly), None, "{other}");
        }
        // Betas and nightlies count other things.
        assert_eq!(
            v("v2.0.0-Beta-2420-03edb47-yaopl").channel_order(&nightly),
            None
        );
    }
}
