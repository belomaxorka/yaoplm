//! A release of a catalogue channel as the GitHub Releases API describes it,
//! and what YAOPLM would download from it (SPEC LDR-2, §5.10.2, ADR-0042).
//!
//! Facts the rules rely on (research/17 §6–§7, 2026-10-25): upstream's
//! `latest` carries its version only in the versioned asset names and has
//! no `SHA256SUMS`; YAOPL's release name is the version and its
//! `SHA256SUMS` lists the ELF; `v1.1.0` has no digests, so the catalogue
//! pins its archives. The date of a rebuilt-in-place release is the upload
//! of its asset (`updated_at`), not `published_at`.

use serde::Deserialize;

use super::notes::{self, Changelog};
use super::version::BuildVersion;
use super::{Build, ElfSource, LoaderError, VersionRule};
use yaoplm_i18n::Phrase;

/// The fields of `GET /repos/{owner}/{repo}/releases/tags/{tag}` YAOPLM
/// reads; the rest is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    /// `sha256:<hex>`; missing on uploads older than about a year.
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub browser_download_url: String,
}

impl Release {
    pub fn parse(json: &[u8]) -> Result<Self, LoaderError> {
        serde_json::from_slice(json).map_err(|e| {
            LoaderError::new(Phrase::error("loaderReleaseUnreadable").with("error", e.to_string()))
        })
    }

    /// `GET /repos/{owner}/{repo}/releases`: every release, newest first.
    pub fn parse_list(json: &[u8]) -> Result<Vec<Self>, LoaderError> {
        serde_json::from_slice(json).map_err(|e| {
            LoaderError::new(Phrase::error("loaderReleaseUnreadable").with("error", e.to_string()))
        })
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

impl Asset {
    /// The hex of a `sha256:` digest.
    pub fn sha256(&self) -> Option<String> {
        let hex = self.digest.as_deref()?.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    }
}

/// What confirms the SHA-256 of a file before it is downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChecksumSource {
    /// GitHub's `digest` of the asset.
    Digest,
    /// The release's `SHA256SUMS`.
    Sums,
    /// The catalogue (releases GitHub has no digest for).
    Pinned,
}

/// A release file YAOPLM would download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOffer {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// `None` when nothing confirms it: YAOPLM then shows the hash it
    /// computes itself (SPEC LDR-2).
    pub sha256: Option<String>,
    pub sources: Vec<ChecksumSource>,
    /// `YYYY-MM-DD` of the upload.
    pub date: Option<String>,
}

/// Everything the user sees before a download, and what the download is
/// held to: the confirmed hash goes into the request (SPEC §5.10.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The catalogue build.
    pub build: String,
    pub tag: String,
    pub version: String,
    /// The ELF, or the archive it is inside.
    pub file: FileOffer,
    /// `OPNPS2LD/OPNPS2LD-v1.1.0.ELF` when `file` is an archive.
    pub member: Option<String>,
    /// The language pack, when the release has one.
    pub languages: Option<FileOffer>,
    pub release_url: String,
    /// The commit of the build, from its version string.
    pub commit_url: Option<String>,
    pub changelog: Changelog,
}

impl Offer {
    /// The date of the build: the upload of its file, else the release.
    pub fn date(&self) -> Option<&str> {
        self.file.date.as_deref()
    }
}

/// The version a release of `build` carries, by the channel's rule.
pub fn version(build: &Build, release: &Release) -> Option<BuildVersion> {
    match &build.channel.version {
        VersionRule::Tag => BuildVersion::parse(&release.tag_name),
        VersionRule::Name => release.name.as_deref().and_then(BuildVersion::parse),
        VersionRule::Asset(_) => {
            let mut found = release
                .assets
                .iter()
                .filter_map(|a| build.channel.version.from_asset(&a.name))
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            found.sort();
            found.dedup();
            // Two versions in one release name nothing for sure.
            match found.as_slice() {
                [one] => BuildVersion::parse(one),
                _ => None,
            }
        }
    }
}

fn date(text: Option<&str>) -> Option<String> {
    let day = text?.get(..10)?;
    let ok = day.bytes().enumerate().all(|(i, b)| {
        if i == 4 || i == 7 {
            b == b'-'
        } else {
            b.is_ascii_digit()
        }
    });
    ok.then(|| day.to_owned())
}

/// `https://github.com/<repo>/releases/download/…`: the only place a
/// release file is downloaded from (ADR-0014).
fn served_by_release(build: &Build, url: &str) -> bool {
    let prefix = format!("https://github.com/{}/releases/download/", build.repo);
    url.len() > prefix.len()
        && url[..prefix.len()].eq_ignore_ascii_case(&prefix)
        && !url[prefix.len()..].contains("..")
}

/// The hash of one release file and what confirms it. Sources that know
/// the file must agree.
fn file_offer(
    build: &Build,
    release: &Release,
    asset: &Asset,
    sums: Option<&str>,
) -> Result<FileOffer, LoaderError> {
    if !served_by_release(build, &asset.browser_download_url) {
        return Err(LoaderError::new(
            Phrase::error("loaderAssetUrl").with("asset", &asset.name),
        ));
    }
    let checksums = &build.channel.checksums;
    let mut known: Vec<(ChecksumSource, String)> = Vec::new();
    if checksums.digest
        && let Some(sha) = asset.sha256()
    {
        known.push((ChecksumSource::Digest, sha));
    }
    if checksums.sums.is_some()
        && let Some(sha) = sums.and_then(|text| notes::sums_line(text, &asset.name))
    {
        known.push((ChecksumSource::Sums, sha));
    }
    if let Some(sha) = checksums.pinned.get(&asset.name) {
        known.push((ChecksumSource::Pinned, sha.clone()));
    }
    if known.windows(2).any(|w| w[0].1 != w[1].1) {
        return Err(LoaderError::new(
            Phrase::error("loaderChecksumsDisagree").with("asset", &asset.name),
        ));
    }
    Ok(FileOffer {
        name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256: known.first().map(|(_, sha)| sha.clone()),
        sources: known.into_iter().map(|(source, _)| source).collect(),
        date: date(asset.updated_at.as_deref()).or_else(|| date(release.published_at.as_deref())),
    })
}

/// What YAOPLM would download from `release` of `build`. `sums` is the
/// text of the channel's checksum file when the release has one.
pub fn offer(build: &Build, release: &Release, sums: Option<&str>) -> Result<Offer, LoaderError> {
    let Some(version) = version(build, release) else {
        return Err(LoaderError::new(
            Phrase::error("loaderReleaseNoVersion").with("tag", &release.tag_name),
        ));
    };
    if !build.takes_version(&version) {
        return Err(LoaderError::new(
            Phrase::error("loaderReleaseOtherChannel")
                .with("version", version.to_string())
                .with("build", &build.name),
        ));
    }
    let missing = |name: &str| {
        LoaderError::new(
            Phrase::error("loaderAssetMissing")
                .with("tag", &release.tag_name)
                .with("asset", name),
        )
    };
    let (asset_name, member) = match &build.channel.elf {
        ElfSource::Asset(name) => (name.as_str(), None),
        ElfSource::Member { archive, member } => (archive.as_str(), Some(member.clone())),
    };
    let asset = release
        .asset(asset_name)
        .ok_or_else(|| missing(asset_name))?;
    let file = file_offer(build, release, asset, sums)?;
    let text = version.to_string();
    let languages = match &build.channel.languages {
        Some(template) => match release.asset(&template.replace("{version}", &text)) {
            Some(asset) => Some(file_offer(build, release, asset, sums)?),
            None => None,
        },
        None => None,
    };
    Ok(Offer {
        build: build.id.clone(),
        tag: release.tag_name.clone(),
        version: text,
        file,
        member,
        languages,
        release_url: release.html_url.clone(),
        commit_url: version
            .commit
            .as_ref()
            .map(|commit| format!("https://github.com/{}/commit/{commit}", build.repo)),
        changelog: notes::changelog(release.body.as_deref().unwrap_or_default()),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::loaders::built_in;

    pub const BETA_ELF: &str = "4b6d36eff917bbeb3d860fea7ca0b3dd38502c4f5b008803a1edf3f0edf12290";
    pub const YAOPL_ELF: &str = "aabb85cc5b3f2194ccb3f7e0488b48a49fcd5197be8e5936d7e3e27b9489e4fa";

    pub fn asset(repo: &str, tag: &str, name: &str, size: u64, sha: Option<&str>) -> Asset {
        Asset {
            name: name.to_owned(),
            size,
            digest: sha.map(|s| format!("sha256:{s}")),
            updated_at: Some("2026-10-13T04:07:08Z".to_owned()),
            browser_download_url: format!(
                "https://github.com/{repo}/releases/download/{tag}/{name}"
            ),
        }
    }

    /// Upstream `latest` as GitHub showed it on 2026-10-25 (research/17
    /// §7.1), with the body shortened.
    pub fn beta_release() -> Release {
        let repo = "ps2homebrew/Open-PS2-Loader";
        let v = "v1.2.0-Beta-2271-b3a2199";
        Release {
            tag_name: "latest".into(),
            name: Some("latest".into()),
            body: Some(format!(
                "## Documentation\n* Fix one thing by @someone in https://github.com/{repo}/commit/b3a2199\n"
            )),
            html_url: format!("https://github.com/{repo}/releases/tag/latest"),
            prerelease: true,
            draft: false,
            published_at: Some("2026-10-13T04:07:00Z".into()),
            assets: vec![
                asset(repo, "latest", "OPNPS2LD.ELF", 1_361_812, Some(BETA_ELF)),
                asset(
                    repo,
                    "latest",
                    "OPNPS2LD.7z",
                    1_406_597,
                    Some(&"5".repeat(64)),
                ),
                asset(
                    repo,
                    "latest",
                    &format!("OPNPS2LD-{v}.7z"),
                    1_406_597,
                    Some(&"5".repeat(64)),
                ),
                asset(
                    repo,
                    "latest",
                    &format!("OPNPS2LD-VARIANTS-{v}.7z"),
                    4_617_101,
                    Some(&"e".repeat(64)),
                ),
                asset(
                    repo,
                    "latest",
                    "OPNPS2LD-LANGS.7z",
                    3_404_283,
                    Some(&"a".repeat(64)),
                ),
            ],
        }
    }

    pub fn yaopl_release() -> Release {
        let repo = "belomaxorka/yaopl";
        Release {
            tag_name: "latest".into(),
            name: Some("v1.2.0-Beta-2370-33a2b19-yaopl".into()),
            body: Some("Automatic YAOPL build of v1.2.0-Beta-2370-33a2b19-yaopl.\n\nSHA-256 checksums of the files are in SHA256SUMS.\n".into()),
            html_url: format!("https://github.com/{repo}/releases/tag/latest"),
            prerelease: true,
            draft: false,
            published_at: Some("2026-10-23T07:19:25Z".into()),
            assets: vec![
                asset(repo, "latest", "OPNPS2LD.ELF", 1_377_492, Some(YAOPL_ELF)),
                asset(repo, "latest", "OPNPS2LD-LANGS-v1.2.0-Beta-2370-33a2b19-yaopl.7z", 3_411_527, Some(&"1".repeat(64))),
                asset(repo, "latest", "SHA256SUMS", 421, Some(&"2".repeat(64))),
            ],
        }
    }

    pub fn yaopl_sums() -> String {
        format!(
            "{YAOPL_ELF}  OPNPS2LD.ELF\n{}  OPNPS2LD-LANGS-v1.2.0-Beta-2370-33a2b19-yaopl.7z\n",
            "1".repeat(64)
        )
    }

    pub fn stable_release() -> Release {
        let repo = "ps2homebrew/Open-PS2-Loader";
        let mut archive = asset(repo, "v1.1.0", "OPNPS2LD-v1.1.0.7z", 1_238_940, None);
        archive.updated_at = Some("2021-09-09T19:08:07Z".into());
        Release {
            tag_name: "v1.1.0".into(),
            name: Some("v1.1.0".into()),
            body: Some("New in OPL v1.1.0\n- GCC 11 toolchain\n- NBD server\n".into()),
            html_url: format!("https://github.com/{repo}/releases/tag/v1.1.0"),
            prerelease: false,
            draft: false,
            published_at: Some("2021-09-09T19:08:06Z".into()),
            assets: vec![
                archive,
                asset(
                    repo,
                    "v1.1.0",
                    "OPNPS2LD-LANGUAGES-AND-FONTS-v1.1.0.7z",
                    2_511_479,
                    None,
                ),
            ],
        }
    }

    fn build(id: &str) -> &'static Build {
        built_in().unwrap().build(id).unwrap()
    }

    #[test]
    fn the_beta_offers_its_elf_by_digest() {
        let offer = offer(build("opl-beta"), &beta_release(), None).unwrap();
        assert_eq!(offer.version, "v1.2.0-Beta-2271-b3a2199");
        assert_eq!(offer.file.name, "OPNPS2LD.ELF");
        assert_eq!(offer.file.size, 1_361_812);
        assert_eq!(offer.file.sha256.as_deref(), Some(BETA_ELF));
        assert_eq!(offer.file.sources, [ChecksumSource::Digest]);
        assert_eq!(offer.date(), Some("2026-10-13"));
        assert_eq!(offer.member, None);
        assert_eq!(offer.languages.as_ref().unwrap().name, "OPNPS2LD-LANGS.7z");
        assert_eq!(
            offer.commit_url.as_deref(),
            Some("https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199")
        );
        assert_eq!(offer.changelog.items.len(), 1);
    }

    #[test]
    fn yaopl_needs_digest_and_sums_to_agree() {
        let release = yaopl_release();
        let offer = offer(build("yaopl"), &release, Some(&yaopl_sums())).unwrap();
        assert_eq!(offer.version, "v1.2.0-Beta-2370-33a2b19-yaopl");
        assert_eq!(offer.file.sha256.as_deref(), Some(YAOPL_ELF));
        assert_eq!(
            offer.file.sources,
            [ChecksumSource::Digest, ChecksumSource::Sums]
        );
        // The release text is service lines only.
        assert!(offer.changelog.items.is_empty());

        let wrong = yaopl_sums().replace(YAOPL_ELF, &"3".repeat(64));
        let err = super::offer(build("yaopl"), &release, Some(&wrong)).unwrap_err();
        assert_eq!(err.phrase.key, "errors.loaderChecksumsDisagree");
    }

    #[test]
    fn the_stable_release_is_held_to_the_pinned_archive() {
        let offer = offer(build("opl-stable"), &stable_release(), None).unwrap();
        assert_eq!(offer.version, "v1.1.0");
        assert_eq!(offer.file.name, "OPNPS2LD-v1.1.0.7z");
        assert_eq!(
            offer.member.as_deref(),
            Some("OPNPS2LD/OPNPS2LD-v1.1.0.ELF")
        );
        assert_eq!(
            offer.file.sha256.as_deref(),
            Some("869b51f2c9c442c79f9545a57d65ed1a28a47b19fb82ac7d307ad6cb30ddd4ed")
        );
        assert_eq!(offer.file.sources, [ChecksumSource::Pinned]);
        assert_eq!(offer.date(), Some("2021-09-09"));
        assert_eq!(offer.commit_url, None);
        assert_eq!(offer.languages.unwrap().sources, [ChecksumSource::Pinned]);
    }

    #[test]
    fn broken_releases_are_refused() {
        let beta = build("opl-beta");
        let mut release = beta_release();
        release.assets.retain(|a| a.name != "OPNPS2LD.ELF");
        let err = offer(beta, &release, None).unwrap_err();
        assert_eq!(err.phrase.key, "errors.loaderAssetMissing");

        let mut release = beta_release();
        release.assets.retain(|a| !a.name.contains("Beta"));
        assert_eq!(
            offer(beta, &release, None).unwrap_err().phrase.key,
            "errors.loaderReleaseNoVersion"
        );

        // A file served from anywhere but the project's releases.
        let mut release = beta_release();
        release.assets[0].browser_download_url = "https://example.org/OPNPS2LD.ELF".into();
        assert_eq!(
            offer(beta, &release, None).unwrap_err().phrase.key,
            "errors.loaderAssetUrl"
        );
        let mut release = beta_release();
        release.assets[0].browser_download_url =
            "https://github.com/someone/else/releases/download/latest/OPNPS2LD.ELF".into();
        assert_eq!(
            offer(beta, &release, None).unwrap_err().phrase.key,
            "errors.loaderAssetUrl"
        );

        // YAOPL's release read as upstream's: its version is another channel's.
        let mut release = yaopl_release();
        release.assets.push(asset(
            "ps2homebrew/Open-PS2-Loader",
            "latest",
            "OPNPS2LD-v1.2.0-Beta-2370-33a2b19-yaopl.7z",
            1,
            None,
        ));
        for a in &mut release.assets {
            a.browser_download_url = a
                .browser_download_url
                .replace("belomaxorka/yaopl", "ps2homebrew/Open-PS2-Loader");
        }
        assert_eq!(
            offer(beta, &release, None).unwrap_err().phrase.key,
            "errors.loaderReleaseOtherChannel"
        );
    }

    #[test]
    fn an_unconfirmed_file_has_no_hash_to_hold_it_to() {
        let mut release = beta_release();
        release.assets[0].digest = None;
        let offer = offer(build("opl-beta"), &release, None).unwrap();
        assert_eq!(offer.file.sha256, None);
        assert!(offer.file.sources.is_empty());
    }

    #[test]
    fn digests_in_any_case_and_versions_that_disagree() {
        let beta = build("opl-beta");
        let mut release = beta_release();
        release.assets[0].digest = Some(format!("sha256:{}", BETA_ELF.to_uppercase()));
        assert_eq!(
            offer(beta, &release, None).unwrap().file.sha256.as_deref(),
            Some(BETA_ELF)
        );
        // A digest that is no SHA-256 confirms nothing.
        release.assets[0].digest = Some("sha512:abc".into());
        assert_eq!(offer(beta, &release, None).unwrap().file.sha256, None);
        // Two builds named in one release: which one is it?
        let mut release = beta_release();
        release.assets.push(asset(
            "ps2homebrew/Open-PS2-Loader",
            "latest",
            "OPNPS2LD-v1.2.0-Beta-2280-0123abc.7z",
            1,
            None,
        ));
        assert_eq!(
            offer(beta, &release, None).unwrap_err().phrase.key,
            "errors.loaderReleaseNoVersion"
        );
    }

    #[test]
    fn github_json_reads() {
        let json = br#"{"tag_name":"latest","name":"latest","body":null,"html_url":"https://github.com/o/r/releases/tag/latest",
            "prerelease":true,"draft":false,"published_at":"2026-10-13T04:07:00Z","id":411255901,
            "assets":[{"name":"OPNPS2LD.ELF","size":5,"digest":null,"updated_at":"2026-10-13T04:07:08Z",
            "browser_download_url":"https://github.com/o/r/releases/download/latest/OPNPS2LD.ELF","id":1}]}"#;
        let release = Release::parse(json).unwrap();
        assert_eq!(release.assets[0].sha256(), None);
        assert!(Release::parse(b"{}").is_err());
    }
}
