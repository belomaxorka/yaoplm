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
use yaoplm_core::art::{ArtItem, plan_set_art};
use yaoplm_core::convert;
use yaoplm_core::detect;
use yaoplm_core::loaders::impact;
use yaoplm_core::loaders::install::{self, ElfFile, InstallRequest, InstallTarget};
use yaoplm_core::loaders::installed;
use yaoplm_core::loaders::release::{self, ChecksumSource, FileOffer, Offer, Release};
use yaoplm_core::loaders::rollback;
use yaoplm_core::loaders::{
    self, Build, Catalog, DocKind, Launcher, LoaderError, Publisher, Target, boot, download,
    languages, local,
};
use yaoplm_core::ops::GameRef;
use yaoplm_core::plan::Plan;
use yaoplm_core::plan::{Notice as CoreNotice, PlanKind, Skipped};
use yaoplm_core::{LoaderProfile as CoreProfile, ScanOptions};
use yaoplm_device::RelPath;
use yaoplm_device::{DeviceFs, HostSources, SourceFs};
use yaoplm_formats::art::{ArtKind, ArtSubject, recommended_size};
use yaoplm_net::NetError;
use yaoplm_net::cache::now_secs;
use yaoplm_net::loaders::{ElfRecord, LoaderNet, LoaderStore, MAX_TEXT, ReleaseSource, StoredFile};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::art::Net;
use crate::error::net_error;
use crate::jobs::JobResult;
use crate::library::LoaderProfile;
use crate::phrase::Phrase;
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
    /// The release's language pack is kept with it.
    pub languages: bool,
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
        languages: record.languages.is_some(),
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
    /// Also the release's language pack (LDR-3).
    pub languages: bool,
}

/// Downloads the language pack of `offer` for `record` unless it is kept
/// already; held to the size and hash the release gives.
fn download_languages(
    net: &Net,
    loader_net: &LoaderNet<'_>,
    build: &Build,
    offer: &Offer,
    mut record: ElfRecord,
) -> Result<ElfRecord, ApiError> {
    let store = loader_net.store;
    if let Some(kept) = &record.languages
        && store
            .languages(&kept.sha256)
            .map_err(|e| net_error(ErrorCode::Internal, &e))?
            .is_some()
    {
        return Ok(record);
    }
    let Some(pack) = &offer.languages else {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderNoLanguages").with("build", &build.name),
        ));
    };
    if pack.size > download::MAX_FILE {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderFileTooLarge").with("file", &pack.name),
        ));
    }
    let bytes = net
        .block_on(loader_net.file(&build.repo, &pack.url, download::MAX_FILE))?
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    let sha = yaoplm_hash::sha256_reader(&bytes[..]).map_err(ApiError::internal_error)?;
    if bytes.len() as u64 != pack.size || pack.sha256.as_ref().is_some_and(|want| *want != sha) {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderDownloadChecksum").with("asset", &pack.name),
        ));
    }
    store
        .put_languages(&sha, &bytes)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    record.languages = Some(StoredFile {
        file: pack.name.clone(),
        sha256: sha,
    });
    store
        .put_record(&record)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    Ok(record)
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
    if let Some(mut record) = stored_from(&store, &offer) {
        if params.languages {
            record = download_languages(net, &loader_net, build, &offer, record)?;
        }
        return Ok(JobResult::LoaderFile(Box::new(file_view(&record))));
    }
    // A release that says its file is huge is not asked for at all.
    if offer.file.size > download::MAX_FILE {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderFileTooLarge").with("file", &offer.file.name),
        ));
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
        languages: None,
        stored_at: now_secs(),
    };
    store
        .put_elf(&record, &elf.bytes)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    let record = if params.languages {
        download_languages(net, &loader_net, build, &offer, record)?
    } else {
        record
    };
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
    // The same ELF downloaded from its release says more than a file
    // nobody vouches for: that record stays.
    if let Some(downloaded) = store
        .elfs()
        .into_iter()
        .find(|r| r.sha256 == local.elf.sha256 && r.tag.is_some())
    {
        return Ok(JobResult::LoaderFile(Box::new(file_view(&downloaded))));
    }
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
        languages: None,
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

/// One way to start an installed loader (BOOT-1): the path the launcher
/// uses and the lines the user adds to its config on the console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderBootHint {
    pub launcher: LoaderLauncher,
    /// `mass:/UPDT/OPL.ELF`; `null` for OPL's Apps page.
    pub ps2_path: Option<String>,
    /// The launcher's stock config starts it: nothing to edit.
    pub stock: bool,
    /// The file the lines go into, on the console.
    pub config: Option<String>,
    /// `N` stands for a free item number.
    pub lines: Vec<String>,
    /// The app's name on OPL's Apps page.
    pub title: Option<String>,
}

fn device(target: LoaderTarget) -> Target {
    match target {
        LoaderTarget::Usb => Target::Usb,
        LoaderTarget::Mx4sio => Target::Mx4sio,
    }
}

fn found(catalog: &Catalog, id: &str) -> Result<Build, ApiError> {
    catalog.build(id).cloned().ok_or_else(|| {
        ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("loaderUnknownBuild").with("build", id),
        )
    })
}

/// The places a request names: catalogue presets for this device, and a
/// path of the user's own.
fn targets(
    build: &Build,
    presets: &[String],
    path: Option<&str>,
    target: Target,
) -> Result<Vec<(InstallTarget, Vec<Launcher>)>, ApiError> {
    let mut out = Vec::new();
    for id in presets {
        let preset = build.preset(id).ok_or_else(|| {
            ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("loaderUnknownPreset").with("preset", id),
            )
        })?;
        if !preset.devices.contains(&target) {
            return Err(ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("loaderPresetNotForDevice").with("preset", id),
            ));
        }
        out.push((
            InstallTarget {
                path: preset.path.clone(),
                preset: Some(id.clone()),
                title: preset.title.clone(),
            },
            preset.launchers.clone(),
        ));
    }
    if let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) {
        out.push((
            InstallTarget {
                path: path.replace('\\', "/").trim_start_matches('/').to_owned(),
                preset: None,
                title: None,
            },
            boot::custom_launchers(target),
        ));
    }
    Ok(out)
}

pub(crate) struct InstallParams {
    pub build: String,
    /// The stored ELF.
    pub sha256: String,
    pub presets: Vec<String>,
    pub path: Option<String>,
    pub device: LoaderTarget,
    pub language: Option<String>,
    pub starter_settings: bool,
    pub replace_foreign: bool,
}

/// The build, the stored ELF and the language files an install of
/// `sha256` takes: the ELF must be a build of `build_id`.
fn prepare(
    store: &LoaderStore,
    catalog: &Catalog,
    build_id: &str,
    sha256: &str,
    language: Option<&str>,
) -> Result<PreparedElf, ApiError> {
    let build = found(catalog, build_id)?;
    let sha = sha256.to_ascii_lowercase();
    let not_stored = || {
        ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("loaderNotStored").with("sha256", &sha),
        )
    };
    let record = store.record(&sha).ok_or_else(not_stored)?;
    if record.build.as_deref() != Some(build.id.as_str()) {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("loaderFileOtherBuild").with("build", &build.name),
        ));
    }
    let path = store
        .elf_file(&sha)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?;
    let meta = HostSources.metadata(&path).map_err(|_| not_stored())?;
    let language = match language {
        None => None,
        Some(name) => {
            let pack = record.languages.as_ref().ok_or_else(|| {
                ApiError::phrased(
                    ErrorCode::BadRequest,
                    &Said::error("loaderLanguagesNotStored").with("build", &build.name),
                )
            })?;
            let bytes = store
                .languages(&pack.sha256)
                .map_err(|e| net_error(ErrorCode::Internal, &e))?
                .ok_or_else(not_stored)?;
            let files = languages::files(&pack.file, &bytes, name)
                .map_err(|e| loader_error(ErrorCode::BadRequest, &e))?;
            Some((name.to_owned(), files))
        }
    };
    let elf = ElfFile {
        path,
        sha256: sha.clone(),
        len: meta.len,
        modified: meta.modified,
        version: record.version.clone(),
        tag: record.tag.clone(),
        source: record.source.clone(),
    };
    Ok(PreparedElf {
        build,
        elf,
        language,
    })
}

struct PreparedElf {
    build: Build,
    elf: ElfFile,
    language: Option<(String, Vec<languages::LanguageFile>)>,
}

fn now() -> i64 {
    i64::try_from(now_secs()).unwrap_or_default()
}

/// Plans installing a stored ELF on `fs` (LDR-3).
pub(crate) fn plan_install(
    net: &Net,
    fs: &dyn DeviceFs,
    params: &InstallParams,
) -> Result<Plan, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let PreparedElf {
        build,
        elf,
        language,
    } = prepare(
        &store,
        &catalog,
        &params.build,
        &params.sha256,
        params.language.as_deref(),
    )?;
    let request = InstallRequest {
        build: &build,
        elf,
        targets: targets(
            &build,
            &params.presets,
            params.path.as_deref(),
            device(params.device),
        )?
        .into_iter()
        .map(|(target, _)| target)
        .collect(),
        device: device(params.device),
        language,
        starter_settings: params.starter_settings,
        replace_foreign: params.replace_foreign,
        update: false,
        retire: Vec::new(),
        now: now(),
    };
    install::plan_install(fs, &request).map_err(crate::io_error)
}

/// The loaders on `fs` as the catalogue in force knows them (LDR-4).
fn installed_on(
    fs: &dyn DeviceFs,
    catalog: &Catalog,
) -> Result<Vec<installed::Installed>, ApiError> {
    let detection = detect::detect_profile(fs).map_err(io_error)?;
    installed::installed(fs, &detection, catalog).map_err(io_error)
}

fn not_installed(path: &str) -> ApiError {
    ApiError::phrased(
        ErrorCode::NotFound,
        &Said::error("loaderNotInstalled").with("path", path),
    )
}

/// Plans updating the build at `path` to the stored ELF `sha256`, a newer
/// build of the same channel (LDR-4): only the ELF, its language pack and
/// the manifest change, the old ELF is kept as a backup.
pub(crate) fn plan_update(
    net: &Net,
    fs: &dyn DeviceFs,
    path: &str,
    sha256: &str,
) -> Result<Plan, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let found = installed_on(fs, &catalog)?;
    let current = found
        .iter()
        .find(|l| l.path.eq_ignore_ascii_case(path.trim_start_matches('/')))
        .ok_or_else(|| not_installed(path))?;
    let build_id = current
        .build
        .as_deref()
        .ok_or_else(|| not_installed(path))?;
    let entry = current.entry.as_ref();
    let PreparedElf {
        build,
        elf,
        language,
    } = prepare(
        &store,
        &catalog,
        build_id,
        sha256,
        entry.and_then(|e| e.language.as_deref()),
    )?;
    let device = match entry.map(|e| e.device.as_str()) {
        Some("mx4sio") => Target::Mx4sio,
        _ => Target::Usb,
    };
    let request = InstallRequest {
        build: &build,
        elf,
        targets: vec![InstallTarget {
            path: current.path.clone(),
            preset: entry.and_then(|e| e.preset.clone()),
            title: None,
        }],
        device,
        language,
        starter_settings: false,
        replace_foreign: false,
        update: true,
        retire: Vec::new(),
        now: now(),
    };
    install::plan_install(fs, &request).map_err(crate::io_error)
}

/// Plans putting back the newest backup of the loader at `path` (LDR-4).
pub(crate) fn plan_rollback(net: &Net, fs: &dyn DeviceFs, path: &str) -> Result<Plan, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    rollback::plan_rollback(fs, path.trim_start_matches('/'), &catalog, now()).map_err(io_error)
}

/// The newest release of a build's channel YAOPLM read, from the store
/// alone, and when it was read (Unix seconds).
fn cached_offer(store: &LoaderStore, build: &Build) -> Option<(Offer, Option<u64>)> {
    let (json, _) = store
        .cached_release(&build.repo, &build.channel.tag)
        .ok()??;
    let release = Release::parse(&json).ok()?;
    let offer = release::offer(build, &release, None).ok()?;
    Some((
        offer,
        store.release_checked_at(&build.repo, &build.channel.tag),
    ))
}

/// What the "Update available" window shows, without the network (LDR-4,
/// SPEC §5.10.1): the release as it was last read.
pub(crate) fn cached_release(
    store: &LoaderStore,
    id: &str,
) -> Result<Option<LoaderOffer>, ApiError> {
    let (catalog, _, _) = effective(store)?;
    let build = found(&catalog, id)?;
    Ok(cached_offer(store, &build).map(|(offer, _)| {
        let stored = stored_from(store, &offer);
        offer_view(&build, &offer, false, stored.as_ref())
    }))
}

/// The newest version of a channel YAOPLM read, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    /// Unix seconds.
    pub checked_at: Option<u64>,
}

/// What the device cards need about loaders, read once per device list:
/// the catalogue in force and the newest version of each channel from the
/// store. Nothing here goes to the network (SPEC §5.10.1).
#[derive(Debug, Clone)]
pub struct LoaderContext {
    pub catalog: Catalog,
    pub latest: BTreeMap<String, Latest>,
}

pub(crate) fn context(store: &LoaderStore) -> Option<LoaderContext> {
    let (catalog, _, _) = effective(store).ok()?;
    let latest = catalog
        .builds
        .iter()
        .filter_map(|build| {
            let (offer, checked_at) = cached_offer(store, build)?;
            Some((
                build.id.clone(),
                Latest {
                    version: offer.version,
                    checked_at,
                },
            ))
        })
        .collect();
    Some(LoaderContext { catalog, latest })
}

/// How the console starts `build` at the places a request names (BOOT-1).
pub(crate) fn boot_hints(
    store: &LoaderStore,
    id: &str,
    presets: &[String],
    path: Option<&str>,
    target: LoaderTarget,
) -> Result<Vec<LoaderBootHint>, ApiError> {
    let (catalog, _, _) = effective(store)?;
    let build = found(&catalog, id)?;
    let mut hints = Vec::new();
    for (place, launchers) in targets(&build, presets, path, device(target))? {
        let title = place.title.clone().unwrap_or_else(|| build.name.clone());
        hints.extend(
            boot::hints(&place.path, device(target), &launchers, &title)
                .into_iter()
                .map(|hint| LoaderBootHint {
                    launcher: launcher(hint.launcher),
                    ps2_path: hint.ps2_path,
                    stock: hint.stock,
                    config: hint.config.map(str::to_owned),
                    lines: hint.lines,
                    title: hint.title,
                }),
        );
    }
    Ok(hints)
}

/// The languages of the pack kept with a stored ELF.
pub(crate) fn language_names(store: &LoaderStore, sha256: &str) -> Result<Vec<String>, ApiError> {
    let sha = sha256.to_ascii_lowercase();
    let record = store.record(&sha).ok_or_else(|| {
        ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("loaderNotStored").with("sha256", &sha),
        )
    })?;
    let Some(pack) = record.languages else {
        return Ok(Vec::new());
    };
    let bytes = store
        .languages(&pack.sha256)
        .map_err(|e| net_error(ErrorCode::Internal, &e))?
        .unwrap_or_default();
    languages::names(&pack.file, &bytes).map_err(|e| loader_error(ErrorCode::BadRequest, &e))
}

/// One release of a build's channel (LDR-6): "Another version…".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderReleaseItem {
    pub tag: String,
    pub version: Option<String>,
    /// `YYYY-MM-DD` it was published.
    pub date: Option<String>,
    pub prerelease: bool,
    /// The channel's own tag, installed by default.
    pub default: bool,
}

/// The releases of a build's channel the catalogue allows, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderReleaseList {
    pub build: String,
    pub releases: Vec<LoaderReleaseItem>,
    /// GitHub could not be reached: the last stored list is shown.
    pub offline: bool,
}

/// The body of a `LoaderReleases` job: GitHub's list of the repository's
/// releases, kept to the tags `channel.releases` names (SPEC §5.10.2).
pub(crate) fn releases_job(net: &Net, params: &ReleaseParams) -> Result<JobResult, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let build = found(&catalog, &params.build)?;
    let service = net.service()?;
    let source = source(&params.mirror);
    let loader_net = LoaderNet {
        http: service.http(),
        source: &source,
        store: &store,
    };
    let (json, offline) = match net.block_on(loader_net.releases(&build.repo))? {
        Ok(list) => (list.json, false),
        Err(err @ NetError::Status { .. }) => return Err(net_error(ErrorCode::NotFound, &err)),
        Err(err) => match store.cached_list(&build.repo) {
            Ok(Some((json, _))) if matches!(source, ReleaseSource::Remote { .. }) => (json, true),
            _ => return Err(net_error(ErrorCode::Internal, &err)),
        },
    };
    let releases = Release::parse_list(&json).map_err(|e| loader_error(ErrorCode::Internal, &e))?;
    let items = releases
        .iter()
        .filter(|r| !r.draft)
        .filter(|r| {
            build
                .channel
                .releases
                .iter()
                .any(|pattern| loaders::catalog::glob_match(pattern, &r.tag_name, false))
        })
        .map(|r| LoaderReleaseItem {
            tag: r.tag_name.clone(),
            version: release::version(&build, r).map(|v| v.to_string()),
            date: r
                .published_at
                .as_deref()
                .and_then(|d| d.get(..10))
                .map(str::to_owned),
            prerelease: r.prerelease,
            default: r.tag_name == build.channel.tag,
        })
        .collect();
    Ok(JobResult::LoaderReleases(Box::new(LoaderReleaseList {
        build: build.id.clone(),
        releases: items,
        offline,
    })))
}

/// A game the new build would not show (LDR-7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImpactGame {
    pub startup: String,
    pub title: String,
    /// The image, relative to the root, `/`-separated; `null` for `ul`.
    pub path: Option<String>,
    pub size: u64,
    /// Pieces of its files, when known.
    pub fragments: Option<u32>,
}

/// A picture the new build would not load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImpactArt {
    pub startup: String,
    pub title: String,
    pub suffix: String,
    pub path: String,
    pub size: u64,
}

/// What replacing the loader by `build` changes for the games (LDR-7,
/// SPEC §5.10.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderImpact {
    pub build: String,
    pub from: LoaderProfile,
    pub to: LoaderProfile,
    /// The new build cannot use the device: no replacement.
    pub blocked: Option<Phrase>,
    /// Converted to ISO in the replace plan unless left out.
    pub zso: Vec<ImpactGame>,
    /// In several pieces: a list and the README's advice.
    pub fragmented: Vec<ImpactGame>,
    /// The OS cannot say how many pieces.
    pub unknown_fragments: Vec<ImpactGame>,
    /// Re-encoded to PNG in the replace plan unless left out.
    pub art: Vec<ImpactArt>,
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn impact_game(game: &impact::ImpactGame) -> ImpactGame {
    ImpactGame {
        startup: game.startup.clone(),
        title: game.title.clone(),
        path: game.path.as_deref().map(slash),
        size: game.size,
        fragments: game.fragments,
    }
}

/// The library under `to`'s rules, and what changes against `from`.
fn analyse(
    root: &Path,
    from: CoreProfile,
    to: CoreProfile,
) -> Result<(yaoplm_core::Library, impact::Impact), ApiError> {
    let options = ScanOptions {
        profile: to,
        verify_images: false,
    };
    let library = yaoplm_core::scan_library(root, &options).map_err(|e| crate::scan_error(&e))?;
    let kind = yaoplm_device::volume_for_path(root)
        .map(|v| v.fs)
        .unwrap_or(yaoplm_device::FsKind::Other);
    let impact = impact::impact(&library, kind, from, to);
    Ok((library, impact))
}

/// The body of a `LoaderImpact` job: the library scanned with `build`'s
/// profile against the device's current one.
pub(crate) fn impact_job(
    net: &Net,
    root: &Path,
    id: &str,
    from: LoaderProfile,
) -> Result<JobResult, ApiError> {
    let (catalog, _, _) = effective(&net.loader_store())?;
    let build = found(&catalog, id)?;
    let (_, found) = analyse(root, from.into(), build.profile)?;
    Ok(JobResult::LoaderImpact(Box::new(LoaderImpact {
        build: build.id.clone(),
        from,
        to: build.profile.into(),
        blocked: found.blocked.as_ref().map(Phrase::from),
        zso: found.zso.iter().map(impact_game).collect(),
        fragmented: found.fragmented.iter().map(impact_game).collect(),
        unknown_fragments: found.unknown_fragments.iter().map(impact_game).collect(),
        art: found
            .art
            .iter()
            .map(|a| ImpactArt {
                startup: a.startup.clone(),
                title: a.title.clone(),
                suffix: a.suffix.clone(),
                path: slash(&a.path),
                size: a.size,
            })
            .collect(),
    })))
}

pub(crate) struct ReplaceParams {
    pub install: InstallParams,
    /// The device's profile now: the rules the library follows today.
    pub from: LoaderProfile,
    /// ZSO images to convert to ISO first (`ul` above FAT32's limit).
    pub convert: Vec<String>,
    /// Re-encode JPG and BMP pictures the new build cannot load.
    pub reencode_art: bool,
    /// Loaders left at other paths, moved into the backup folder.
    pub retire: Vec<String>,
}

/// Plans replacing the device's loader by another build (LDR-7, SPEC
/// §5.10.3): the conversions the user kept, the pictures, then the build's
/// folders, the ELF (the old one kept as a backup), the language pack, the
/// manifest. The loader goes last: if writing stops on the way, the old
/// build still starts.
pub(crate) fn plan_replace(
    net: &Net,
    root: &Path,
    fs: &dyn DeviceFs,
    params: &ReplaceParams,
) -> Result<Plan, ApiError> {
    let store = net.loader_store();
    let (catalog, _, _) = effective(&store)?;
    let install = &params.install;
    let PreparedElf {
        build,
        elf,
        language,
    } = prepare(
        &store,
        &catalog,
        &install.build,
        &install.sha256,
        install.language.as_deref(),
    )?;
    let targets: Vec<InstallTarget> = targets(
        &build,
        &install.presets,
        install.path.as_deref(),
        device(install.device),
    )?
    .into_iter()
    .map(|(target, _)| target)
    .collect();
    let previous = installed_on(fs, &catalog)?;
    let from_name = previous
        .iter()
        .find(|l| {
            targets.iter().any(|t| t.path.eq_ignore_ascii_case(&l.path))
                || params
                    .retire
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(&l.path))
        })
        .or_else(|| previous.first())
        .map(|l| {
            l.build
                .as_deref()
                .and_then(|b| catalog.build(b))
                .map_or_else(|| l.family.name().to_owned(), |b| b.name.clone())
        })
        .unwrap_or_else(|| "OPL".to_owned());

    let loader = install::plan_install(
        fs,
        &InstallRequest {
            build: &build,
            elf,
            targets,
            device: device(install.device),
            language,
            starter_settings: install.starter_settings,
            replace_foreign: install.replace_foreign,
            update: false,
            retire: params.retire.clone(),
            now: now(),
        },
    )
    .map_err(crate::io_error)?;
    let mut plan = Plan::titled(
        PlanKind::ReplaceLoader,
        Said::title("replaceLoader")
            .with("from", &from_name)
            .with("to", &build.name),
        loader.fingerprint.clone(),
        loader.free_bytes,
    );
    plan.profile = Some(build.profile);

    let (library, found) = analyse(root, params.from.into(), build.profile)?;
    if let Some(why) = &found.blocked {
        plan.notices.push(CoreNotice::error(why.clone()));
    }
    // 1. Conversions: only ZSO images this build cannot show.
    let wanted: Vec<RelPath> = found
        .zso
        .iter()
        .filter_map(|g| g.path.as_deref())
        .map(slash)
        .filter(|p| {
            params
                .convert
                .iter()
                .any(|c| c.replace('\\', "/").eq_ignore_ascii_case(p))
        })
        .filter_map(|p| RelPath::new(&p).ok())
        .collect();
    if !wanted.is_empty() {
        let options = convert::ConvertOptions {
            profile: build.profile,
            name_style: None,
        };
        let games: Vec<GameRef> = wanted
            .iter()
            .map(|path| GameRef::Image { path: path.clone() })
            .collect();
        let mut to_iso = convert::plan_convert(fs, &games, convert::Target::Iso, &options)
            .map_err(crate::op_error)?;
        // Above FAT32's limit an ISO cannot be one file: those go to `ul`.
        let (too_large, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut to_iso.skipped)
            .into_iter()
            .partition(|s| s.reason.code == "tooLargeUseUl");
        to_iso.skipped = kept;
        plan.absorb(to_iso);
        if !too_large.is_empty() {
            let games: Vec<GameRef> = wanted
                .iter()
                .filter(|path| too_large.iter().any(|s| s.path == Path::new(path.as_str())))
                .map(|path| GameRef::Image { path: path.clone() })
                .collect();
            let to_ul = convert::plan_convert(fs, &games, convert::Target::Ul, &options)
                .map_err(crate::op_error)?;
            plan.absorb(to_ul);
        }
    }
    // 2. Pictures the build cannot load, re-encoded with a backup.
    if params.reencode_art && !found.art.is_empty() {
        let service = net.service()?;
        let mut items = Vec::new();
        for art in &found.art {
            let Some(kind) = ArtKind::from_suffix(&art.suffix.to_ascii_uppercase()) else {
                continue;
            };
            let Some(game) = library.games.iter().find(|g| g.startup == art.startup) else {
                continue;
            };
            let label = slash(&art.path);
            let size = recommended_size(
                build.profile,
                ArtSubject::Game {
                    startup: &art.startup,
                },
                kind,
            );
            let cover = std::fs::read(root.join(&art.path))
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    service
                        .import_local(&bytes, kind, size, &label)
                        .map_err(|e| e.to_string())
                });
            match cover {
                Ok(cover) => items.push(ArtItem {
                    startup: game.startup.clone(),
                    title: game.title.clone(),
                    media: game.media,
                    format: game.format.to_string(),
                    kind,
                    file: cover.png,
                    origin: label,
                }),
                Err(_) => plan.skipped.push(Skipped {
                    path: label.into(),
                    reason: CoreNotice::warning(Said::notice("artUnreadable")),
                }),
            }
        }
        if !items.is_empty() {
            let art = plan_set_art(fs, &HostSources, &items, true).map_err(crate::io_error)?;
            plan.absorb(art);
        }
    }
    // 3. The loader, last.
    plan.absorb(loader);
    let needed = plan.write_bytes();
    if needed > plan.free_bytes && plan.notices.iter().all(|n| n.code != "noSpace") {
        plan.notices.push(CoreNotice::error(
            Said::notice("noSpace")
                .with("needed", needed)
                .with("free", plan.free_bytes),
        ));
    }
    Ok(plan)
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
