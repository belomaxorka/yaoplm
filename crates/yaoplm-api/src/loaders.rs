//! Loaders as the front ends see them (LDR-1, LDR-2, LDR-6): the build
//! cards with their channels and presets, what a download of a build
//! would be, and the loader ELFs YAOPLM keeps. The texts of the build
//! cards come in every interface language from the catalogue itself
//! (ADR-0038); the front end picks one. Downloads and the catalogue from
//! `main` are jobs: the network is used only on a user action (ADR-0007).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use yaoplm_core::Phrase as Said;
use yaoplm_core::detect;
use yaoplm_core::loaders::release::{self, ChecksumSource, FileOffer, Offer, Release};
use yaoplm_core::loaders::{
    self, Build, Catalog, DocKind, Launcher, LoaderError, Publisher, Target, download, local,
};
use yaoplm_net::NetError;
use yaoplm_net::cache::now_secs;
use yaoplm_net::loaders::{ElfRecord, LoaderNet, LoaderStore, MAX_TEXT, ReleaseSource};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::art::Net;
use crate::error::net_error;
use crate::jobs::JobResult;
use crate::library::LoaderProfile;
use crate::{ApiError, ErrorCode, display_path, io_error};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderCatalog {
    pub schema: u32,
    /// In catalogue order.
    pub builds: Vec<LoaderBuild>,
    pub source: LoaderCatalogSource,
    /// When the catalogue from `main` was read, milliseconds since 1970.
    pub fetched_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderBuild {
    pub id: String,
    /// `opl`, `riptopl`, `wopl`, `uopl`, `nhddl`.
    pub family: String,
    pub name: String,
    pub publisher: LoaderPublisher,
    /// `owner/name` on GitHub.
    pub repo: String,
    pub profile: LoaderProfile,
    pub recommended: bool,
    pub devices: Vec<LoaderTarget>,
    pub license: LoaderLicense,
    /// By interface language (`en`, `ru`).
    pub summary: BTreeMap<String, String>,
    pub limits: BTreeMap<String, String>,
    pub docs: Vec<LoaderDoc>,
    pub channel: LoaderChannel,
    pub presets: Vec<LoaderPreset>,
    /// The preset a wizard starts with on each device.
    pub default_presets: Vec<LoaderDefaultPreset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderPublisher {
    Official,
    Fork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderTarget {
    Usb,
    Mx4sio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderLicense {
    pub spdx: String,
    pub url: String,
    pub authors: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderDocKind {
    Readme,
    Site,
    Wiki,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderDoc {
    pub kind: LoaderDocKind,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderChannel {
    pub tag: String,
    /// The tag is rebuilt in place (`latest`).
    pub floating: bool,
    pub prerelease: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderLauncher {
    Fmcb,
    Ps2bbl,
    Ps2bblMx4sio,
    Osdmenu,
    OplApps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderPreset {
    /// `fmcb`, `ps2bbl`, `app`.
    pub id: String,
    /// Relative to the device root.
    pub path: String,
    /// The `title.cfg` title of an app folder.
    pub title: Option<String>,
    pub devices: Vec<LoaderTarget>,
    pub launchers: Vec<LoaderLauncher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderDefaultPreset {
    pub device: LoaderTarget,
    pub preset: String,
}

fn target(target: Target) -> LoaderTarget {
    match target {
        Target::Usb => LoaderTarget::Usb,
        Target::Mx4sio => LoaderTarget::Mx4sio,
    }
}

fn launcher(launcher: Launcher) -> LoaderLauncher {
    match launcher {
        Launcher::Fmcb => LoaderLauncher::Fmcb,
        Launcher::Ps2bbl => LoaderLauncher::Ps2bbl,
        Launcher::Ps2bblMx4sio => LoaderLauncher::Ps2bblMx4sio,
        Launcher::Osdmenu => LoaderLauncher::Osdmenu,
        Launcher::OplApps => LoaderLauncher::OplApps,
    }
}

fn build(build: &Build) -> LoaderBuild {
    LoaderBuild {
        id: build.id.clone(),
        family: build.family.name().to_ascii_lowercase(),
        name: build.name.clone(),
        publisher: match build.publisher {
            Publisher::Official => LoaderPublisher::Official,
            Publisher::Fork => LoaderPublisher::Fork,
        },
        repo: build.repo.clone(),
        profile: build.profile.into(),
        recommended: build.recommended,
        devices: build.devices.iter().copied().map(target).collect(),
        license: LoaderLicense {
            spdx: build.license.spdx.clone(),
            url: build.license.url.clone(),
            authors: build.license.authors.clone(),
        },
        summary: build.summary.clone(),
        limits: build.limits.clone(),
        docs: build
            .docs
            .iter()
            .map(|doc| LoaderDoc {
                kind: match doc.kind {
                    DocKind::Readme => LoaderDocKind::Readme,
                    DocKind::Site => LoaderDocKind::Site,
                    DocKind::Wiki => LoaderDocKind::Wiki,
                    DocKind::Fork => LoaderDocKind::Fork,
                },
                url: doc.url.clone(),
            })
            .collect(),
        channel: LoaderChannel {
            tag: build.channel.tag.clone(),
            floating: build.channel.floating,
            prerelease: build.channel.prerelease,
        },
        presets: build
            .layout
            .presets
            .iter()
            .map(|preset| LoaderPreset {
                id: preset.id.clone(),
                path: preset.path.clone(),
                title: preset.title.clone(),
                devices: preset.devices.iter().copied().map(target).collect(),
                launchers: preset.launchers.iter().copied().map(launcher).collect(),
            })
            .collect(),
        default_presets: build
            .layout
            .defaults
            .iter()
            .map(|(device, preset)| LoaderDefaultPreset {
                device: target(*device),
                preset: preset.clone(),
            })
            .collect(),
    }
}

impl Net {
    pub(crate) fn loader_store(&self) -> LoaderStore {
        LoaderStore::new(self.data_dir())
    }
}

/// Where the catalogue in force comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderCatalogSource {
    /// Compiled into the program.
    BuiltIn,
    /// Read from YAOPLM's `main` by "Check for updates" (LDR-1).
    Main,
}

/// What confirms a file's SHA-256 before it is downloaded (LDR-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderChecksumSource {
    /// GitHub's digest of the asset.
    Digest,
    /// The release's `SHA256SUMS`.
    Sums,
    /// The catalogue: releases GitHub has no digest for.
    Pinned,
}

/// A release file YAOPLM would download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderFileOffer {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// `null` when nothing confirms it: the download then shows the hash
    /// YAOPLM computes itself.
    pub sha256: Option<String>,
    pub sources: Vec<LoaderChecksumSource>,
}

/// One item of a release's change list: plain text, a github.com link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderChange {
    pub text: String,
    pub link: Option<String>,
}

/// What a download of a build would be, shown before it starts (LDR-2,
/// SPEC §5.10.2). `DownloadLoader` is held to `file.sha256`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderOffer {
    pub build: String,
    pub tag: String,
    /// `v1.2.0-Beta-2271-b3a2199`.
    pub version: String,
    /// `YYYY-MM-DD` of the upload.
    pub date: Option<String>,
    /// The ELF, or the archive it is in.
    pub file: LoaderFileOffer,
    pub member: Option<String>,
    pub languages: Option<LoaderFileOffer>,
    pub release_url: String,
    pub commit_url: Option<String>,
    /// At most ten items; empty: no change list was published.
    pub changelog: Vec<LoaderChange>,
    /// The release lists more.
    pub changelog_more: bool,
    pub license: LoaderLicense,
    /// GitHub could not be reached: the last stored answer is shown.
    pub offline: bool,
    /// The SHA-256 of the ELF already stored from this very file.
    pub stored: Option<String>,
}

/// A loader ELF YAOPLM keeps, ready to install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderFile {
    pub sha256: String,
    pub size: u64,
    /// The version inside the ELF, or the release's.
    pub version: Option<String>,
    /// The catalogue build.
    pub build: Option<String>,
    /// The release tag; `null` for the user's own file.
    pub tag: Option<String>,
    /// The release file or the user's file it came from.
    pub file: String,
    pub member: Option<String>,
    /// The release page, or the path of the user's file.
    pub source: String,
    /// A release or the catalogue confirmed its hash; for the user's own
    /// file, its hash is a frozen release's.
    pub confirmed: bool,
    /// The frozen upstream release it is, by SHA-256 (`v1.1.0`).
    pub stable: Option<String>,
    /// Milliseconds since 1970.
    pub stored_at: u64,
}

fn file_view(record: &ElfRecord) -> LoaderFile {
    LoaderFile {
        sha256: record.sha256.clone(),
        size: record.size,
        version: record.version.clone(),
        build: record.build.clone(),
        tag: record.tag.clone(),
        file: record.file.clone(),
        member: record.member.clone(),
        source: record.source.clone(),
        confirmed: record.confirmed,
        stable: detect::stable_version(&record.sha256).map(str::to_owned),
        stored_at: record.stored_at.saturating_mul(1000),
    }
}

fn file_offer(offer: &FileOffer) -> LoaderFileOffer {
    LoaderFileOffer {
        name: offer.name.clone(),
        url: offer.url.clone(),
        size: offer.size,
        sha256: offer.sha256.clone(),
        sources: offer
            .sources
            .iter()
            .map(|source| match source {
                ChecksumSource::Digest => LoaderChecksumSource::Digest,
                ChecksumSource::Sums => LoaderChecksumSource::Sums,
                ChecksumSource::Pinned => LoaderChecksumSource::Pinned,
            })
            .collect(),
    }
}

fn license(build: &Build) -> LoaderLicense {
    LoaderLicense {
        spdx: build.license.spdx.clone(),
        url: build.license.url.clone(),
        authors: build.license.authors.clone(),
    }
}

fn loader_error(code: ErrorCode, err: &LoaderError) -> ApiError {
    ApiError::phrased(code, &err.phrase)
}

/// The catalogue in force: the one last read from `main` while it still
/// reads and this very program read it (a newer program carries a newer
/// copy), else the compiled-in one.
pub(crate) fn effective(
    store: &LoaderStore,
) -> Result<(Catalog, LoaderCatalogSource, Option<u64>), ApiError> {
    if let Some(stored) = store.catalog()
        && stored.program == crate::VERSION
        && let Ok(catalog) = Catalog::parse(|path| stored.files.get(path).cloned())
    {
        return Ok((catalog, LoaderCatalogSource::Main, Some(stored.stored_at)));
    }
    let catalog = loaders::built_in().map_err(|e| ApiError::internal_error(e.to_string()))?;
    Ok((catalog.clone(), LoaderCatalogSource::BuiltIn, None))
}

fn view(catalog: &Catalog, source: LoaderCatalogSource, stored_at: Option<u64>) -> LoaderCatalog {
    LoaderCatalog {
        schema: catalog.schema,
        builds: catalog.builds.iter().map(build).collect(),
        source,
        fetched_at: stored_at.map(|s| s.saturating_mul(1000)),
    }
}

/// The catalogue in force, for the build cards.
pub(crate) fn catalog(store: &LoaderStore) -> Result<LoaderCatalog, ApiError> {
    let (catalog, source, stored_at) = effective(store)?;
    Ok(view(&catalog, source, stored_at))
}

/// The stored loader ELFs, newest first.
pub(crate) fn files(store: &LoaderStore) -> Vec<LoaderFile> {
    store.elfs().iter().map(file_view).collect()
}

/// The build and tag a request names; the tag must be one of the
/// channel's releases (SPEC §5.10.2).
fn named<'c>(
    catalog: &'c Catalog,
    id: &str,
    tag: Option<&str>,
) -> Result<(&'c Build, String), ApiError> {
    let build = catalog.build(id).ok_or_else(|| {
        ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("loaderUnknownBuild").with("build", id),
        )
    })?;
    let tag = tag.unwrap_or(&build.channel.tag).to_owned();
    if catalog.channel_of(&build.repo, &tag).map(|b| b.id.as_str()) != Some(id) {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderTagNotInChannel")
                .with("tag", &tag)
                .with("build", &build.name),
        ));
    }
    Ok((build, tag))
}

pub(crate) struct ReleaseParams {
    pub build: String,
    pub tag: Option<String>,
    /// A folder with GitHub's layout instead of the network.
    pub mirror: Option<PathBuf>,
}

fn source(mirror: &Option<PathBuf>) -> ReleaseSource {
    match mirror {
        Some(dir) => ReleaseSource::Mirror(dir.clone()),
        None => ReleaseSource::github(),
    }
}

/// The offer for `tag` of `build`, and whether GitHub could not be reached
/// and the last stored answer stood in.
fn fetch_offer(
    net: &Net,
    loader_net: &LoaderNet<'_>,
    build: &Build,
    tag: &str,
) -> Result<(Offer, bool), ApiError> {
    let fetched = net.block_on(loader_net.release(&build.repo, tag))?;
    let (json, offline) = match fetched {
        Ok(release) => (release.json, false),
        Err(err @ NetError::Status { .. }) => return Err(net_error(ErrorCode::NotFound, &err)),
        Err(err) => match loader_net.store.cached_release(&build.repo, tag) {
            Ok(Some((json, _))) if matches!(loader_net.source, ReleaseSource::Remote { .. }) => {
                (json, true)
            }
            _ => return Err(net_error(ErrorCode::Internal, &err)),
        },
    };
    let release = Release::parse(&json).map_err(|e| loader_error(ErrorCode::Internal, &e))?;
    let sums = match build
        .channel
        .checksums
        .sums
        .as_deref()
        .and_then(|name| release.asset(name))
    {
        Some(asset) if !offline => {
            let bytes = net
                .block_on(loader_net.file(&build.repo, &asset.browser_download_url, MAX_TEXT))?
                .map_err(|e| net_error(ErrorCode::Internal, &e))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        _ => None,
    };
    let offer = release::offer(build, &release, sums.as_deref())
        .map_err(|e| loader_error(ErrorCode::BadRequest, &e))?;
    Ok((offer, offline))
}

/// The ELF already stored from this release file, by the file's hash.
fn stored_from(store: &LoaderStore, offer: &Offer) -> Option<ElfRecord> {
    let sha = offer.file.sha256.as_ref()?;
    store
        .elfs()
        .into_iter()
        .find(|r| r.file_sha256.as_ref() == Some(sha))
}

fn offer_view(
    build: &Build,
    offer: &Offer,
    offline: bool,
    stored: Option<&ElfRecord>,
) -> LoaderOffer {
    LoaderOffer {
        build: offer.build.clone(),
        tag: offer.tag.clone(),
        version: offer.version.clone(),
        date: offer.date().map(str::to_owned),
        file: file_offer(&offer.file),
        member: offer.member.clone(),
        languages: offer.languages.as_ref().map(file_offer),
        release_url: offer.release_url.clone(),
        commit_url: offer.commit_url.clone(),
        changelog: offer
            .changelog
            .items
            .iter()
            .map(|item| LoaderChange {
                text: item.text.clone(),
                link: item.link.clone(),
            })
            .collect(),
        changelog_more: offer.changelog.more,
        license: license(build),
        offline,
        stored: stored.map(|r| r.sha256.clone()),
    }
}

/// The body of a `LoaderRelease` job.
pub(crate) fn release_job(net: &Net, params: &ReleaseParams) -> Result<JobResult, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let (build, tag) = named(&catalog, &params.build, params.tag.as_deref())?;
    let service = net.service()?;
    let source = source(&params.mirror);
    let loader_net = LoaderNet {
        http: service.http(),
        source: &source,
        store: &store,
    };
    let (offer, offline) = fetch_offer(net, &loader_net, build, &tag)?;
    let stored = stored_from(&store, &offer);
    Ok(JobResult::LoaderOffer(Box::new(offer_view(
        build,
        &offer,
        offline,
        stored.as_ref(),
    ))))
}

pub(crate) struct DownloadParams {
    pub release: ReleaseParams,
    /// The hash the user saw before confirming (SPEC §5.10.2).
    pub sha256: Option<String>,
}

/// The body of a `DownloadLoader` job: the release file, held to the hash
/// the user saw, its ELF checked and stored.
pub(crate) fn download_job(net: &Net, params: &DownloadParams) -> Result<JobResult, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let (build, tag) = named(
        &catalog,
        &params.release.build,
        params.release.tag.as_deref(),
    )?;
    let service = net.service()?;
    let source = source(&params.release.mirror);
    let loader_net = LoaderNet {
        http: service.http(),
        source: &source,
        store: &store,
    };
    let (offer, _) = fetch_offer(net, &loader_net, build, &tag)?;
    let seen = params.sha256.as_deref().map(str::to_ascii_lowercase);
    if let (Some(seen), Some(now)) = (&seen, &offer.file.sha256)
        && seen != now
    {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderReleaseChanged").with("build", &build.name),
        ));
    }
    if let Some(record) = stored_from(&store, &offer) {
        return Ok(JobResult::LoaderFile(Box::new(file_view(&record))));
    }
    let bytes = net
        .block_on(loader_net.file(&build.repo, &offer.file.url, download::MAX_FILE))?
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    let file_sha = yaoplm_hash::sha256_reader(&bytes[..]).map_err(ApiError::internal_error)?;
    if offer.file.sha256.is_none()
        && let Some(seen) = &seen
        && *seen != file_sha
    {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderDownloadChecksum").with("asset", &offer.file.name),
        ));
    }
    let elf =
        download::check(&offer, &bytes).map_err(|e| loader_error(ErrorCode::BadRequest, &e))?;
    let record = ElfRecord {
        sha256: elf.sha256.clone(),
        size: elf.bytes.len() as u64,
        version: elf.version.clone().or_else(|| Some(offer.version.clone())),
        build: Some(build.id.clone()),
        tag: Some(tag),
        file: offer.file.name.clone(),
        member: offer.member.clone(),
        file_sha256: Some(file_sha),
        source: offer.release_url.clone(),
        confirmed: offer.file.sha256.is_some(),
        stored_at: now_secs(),
    };
    store
        .put_elf(&record, &elf.bytes)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    Ok(JobResult::LoaderFile(Box::new(file_view(&record))))
}

/// The body of an `ImportLoader` job: the user's own ELF or archive, read
/// without the network (LDR-2).
pub(crate) fn import_job(net: &Net, path: &Path) -> Result<JobResult, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let too_large = || {
        ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderFileTooLarge").with("file", &name),
        )
    };
    let len = std::fs::metadata(path).map_err(io_error)?.len();
    if len > download::MAX_FILE {
        return Err(too_large());
    }
    let bytes = std::fs::read(path).map_err(io_error)?;
    let local = local::read(&catalog, &name, &bytes)
        .map_err(|e| loader_error(ErrorCode::BadRequest, &e))?;
    let record = ElfRecord {
        sha256: local.elf.sha256.clone(),
        size: local.elf.bytes.len() as u64,
        version: local
            .elf
            .version
            .clone()
            .or(local.stable.map(str::to_owned)),
        build: local.build.clone(),
        tag: None,
        file: name,
        member: local.member.clone(),
        file_sha256: None,
        source: display_path(path),
        confirmed: local.stable.is_some(),
        stored_at: now_secs(),
    };
    store
        .put_elf(&record, &local.elf.bytes)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    Ok(JobResult::LoaderFile(Box::new(file_view(&record))))
}

/// The body of an `UpdateLoaderCatalog` job: the catalogue on `main` is
/// kept only when it reads with this program's parser and checks.
pub(crate) fn update_catalog_job(
    net: &Net,
    mirror: &Option<PathBuf>,
) -> Result<JobResult, ApiError> {
    let store = net.loader_store();
    let service = net.service()?;
    let source = source(mirror);
    let loader_net = LoaderNet {
        http: service.http(),
        source: &source,
        store: &store,
    };
    let mut files = BTreeMap::new();
    let mut failure = None;
    let parsed = Catalog::parse(|path| match net.block_on(loader_net.catalog_file(path)) {
        Ok(Ok(Some(text))) => {
            files.insert(path.to_owned(), text.clone());
            Some(text)
        }
        Ok(Ok(None)) => None,
        Ok(Err(err)) => {
            failure.get_or_insert(net_error(ErrorCode::Internal, &err));
            None
        }
        Err(err) => {
            failure.get_or_insert(err);
            None
        }
    });
    if let Some(failure) = failure {
        return Err(failure);
    }
    let catalog = parsed.map_err(|e| {
        ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderCatalogInvalid").with("error", e.to_string()),
        )
    })?;
    let stored = store
        .put_catalog(&files, crate::VERSION)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    Ok(JobResult::LoaderCatalog(Box::new(view(
        &catalog,
        LoaderCatalogSource::Main,
        Some(stored),
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reaches_the_front_ends() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = catalog(&LoaderStore::new(dir.path())).unwrap();
        assert_eq!(catalog.source, LoaderCatalogSource::BuiltIn);
        assert_eq!(catalog.fetched_at, None);
        let ids: Vec<&str> = catalog.builds.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["opl-stable", "opl-beta", "yaopl"]);
        let beta = &catalog.builds[1];
        assert!(beta.recommended);
        assert_eq!(beta.family, "opl");
        assert_eq!(beta.profile, LoaderProfile::Opl120Beta);
        assert_eq!(catalog.builds[0].profile, LoaderProfile::Opl110);
        assert_eq!(beta.channel.tag, "latest");
        assert!(beta.channel.floating);
        assert_eq!(beta.summary.keys().collect::<Vec<_>>(), ["en", "ru"]);
        let usb = beta
            .default_presets
            .iter()
            .find(|d| d.device == LoaderTarget::Usb)
            .unwrap();
        assert_eq!(usb.preset, "fmcb");
        let yaopl = &catalog.builds[2];
        assert_eq!(yaopl.publisher, LoaderPublisher::Fork);
        let json = serde_json::to_value(beta).unwrap();
        assert_eq!(json["defaultPresets"][0]["device"], "usb");
        assert_eq!(json["presets"][2]["launchers"][1], "ps2bblMx4sio");
    }

    #[test]
    fn a_stored_catalogue_counts_only_for_the_program_that_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = LoaderStore::new(dir.path());
        let built_in = loaders::built_in().unwrap();
        let mut files = BTreeMap::new();
        files.insert(
            "loaders/index.toml".to_owned(),
            "schema = 1\nbuilds = [\"opl-beta\"]\n".to_owned(),
        );
        let beta = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/loaders/opl-beta.toml"
        ))
        .unwrap();
        files.insert("loaders/opl-beta.toml".to_owned(), beta);
        store.put_catalog(&files, crate::VERSION).unwrap();
        let (catalog, source, at) = effective(&store).unwrap();
        assert_eq!(
            (catalog.builds.len(), source),
            (1, LoaderCatalogSource::Main)
        );
        assert!(at.is_some());
        // Read by another version of YAOPLM: the program's own copy.
        store.put_catalog(&files, "0.0.0-older").unwrap();
        let (catalog, source, _) = effective(&store).unwrap();
        assert_eq!(
            (catalog.builds.len(), source),
            (built_in.builds.len(), LoaderCatalogSource::BuiltIn)
        );
        // A stored copy that no longer reads: the program's own copy too.
        files.insert(
            "loaders/opl-beta.toml".to_owned(),
            "id = \"opl-beta\"\n".to_owned(),
        );
        store.put_catalog(&files, crate::VERSION).unwrap();
        assert_eq!(effective(&store).unwrap().1, LoaderCatalogSource::BuiltIn);
    }
}
