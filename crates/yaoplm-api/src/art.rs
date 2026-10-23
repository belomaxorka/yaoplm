//! Cover art over the API (ART-1 to ART-4) and the attribution screen.
//!
//! `FetchArt` is a job: it scans the device, asks `yaoplm-net` for the
//! cover of every game in scope (the cache answers for most of them), logs
//! one line per game and ends with a [`PlanView`] that writes what it found.
//! Nothing touches the device until the user confirms that plan. `SetArt`
//! transcodes a file or pasted bytes the same way and returns a plan at
//! once; `DeleteArt` plans a removal.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_core::art::{ArtItem, plan_delete_art, plan_set_art};
use yaoplm_core::plan::{Notice, Skipped};
use yaoplm_core::{LoaderProfile, ScanOptions, titles};
use yaoplm_device::{HostFs, HostSources};
use yaoplm_formats::art::{ArtKind, ArtSubject, recommended_size};
use yaoplm_formats::serial;
use yaoplm_hash::RedumpIndex;
use yaoplm_net::sources::{SourceConfig, SourceId};
use yaoplm_net::{ArtService, AttemptResult, CoverRequest};

use crate::jobs::{JobContext, JobLogEntry, JobResult, now_ms};
use crate::library::LoaderProfile as WireProfile;
use crate::paths::{PathPolicy, display_path};
use crate::plans::PlanStore;
use crate::{ApiError, ErrorCode};

/// Which games a bulk run touches (ART-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ArtScope {
    /// Games without a cover; existing files stay.
    #[default]
    Missing,
    /// Every game; existing covers are moved to the backup folder.
    All,
}

/// How one source is used for a request. Sources not listed keep their
/// defaults: enabled, remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ArtSourceSetting {
    /// `xlenore` or `libretro`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub enabled: Option<bool>,
    /// A folder with the repository's layout, read instead of the network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub mirror: Option<String>,
}

/// A cover the user supplies (ART-3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ArtInput {
    /// An image file on the host.
    File { path: String },
    /// Image bytes, base64 (a paste or a browser drop).
    Bytes {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ArtLogState {
    /// Downloaded or read from a mirror.
    Found,
    /// Served from the cache.
    Cached,
    NotFound,
    Failed,
}

/// A data or art source, for the About screen (`data/attribution.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    pub id: String,
    pub name: String,
    pub url: String,
    /// `None`: no licence stated; used on request only, never bundled.
    pub license: Option<String>,
    #[serde(default)]
    pub license_url: Option<String>,
    pub credits: String,
    #[serde(default)]
    pub fields: Option<String>,
    #[serde(rename = "use")]
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TitlesInfo {
    pub built: String,
    pub libretro_version: String,
    pub entries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub name: String,
    pub version: String,
    pub license: String,
    pub repository: String,
    /// Bundled databases.
    pub data: Vec<SourceEntry>,
    /// Cover sources, fetched on request.
    pub art: Vec<SourceEntry>,
    pub titles: TitlesInfo,
}

#[derive(Deserialize)]
struct AttributionFile {
    data: Vec<SourceEntry>,
    art: Vec<SourceEntry>,
}

static ATTRIBUTION: &str = include_str!("../../../data/attribution.json");

pub fn attribution() -> Result<Attribution, ApiError> {
    let file: AttributionFile = serde_json::from_str(ATTRIBUTION)
        .map_err(|e| ApiError::internal(format!("attribution.json: {e}")))?;
    let info = titles::database().info();
    Ok(Attribution {
        name: crate::NAME.to_owned(),
        version: crate::VERSION.to_owned(),
        license: "GPL-3.0-or-later".to_owned(),
        repository: "https://github.com/belomaxorka/yaoplm".to_owned(),
        data: file.data,
        art: file.art,
        titles: TitlesInfo {
            built: info.built.clone(),
            libretro_version: info.libretro_version.clone(),
            entries: u32::try_from(info.entries).unwrap_or(u32::MAX),
        },
    })
}

/// The cache folder and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ArtCacheInfo {
    pub path: String,
    pub objects: u64,
    pub bytes: u64,
}

/// The network edge of the API: a tokio runtime and the art service, both
/// created on first use so `Api::new()` stays cheap.
pub(crate) struct Net {
    data_dir: PathBuf,
    runtime: OnceLock<Result<tokio::runtime::Runtime, String>>,
    service: OnceLock<Result<Arc<ArtService>, String>>,
    /// The parsed Redump DAT and the SHA-256 it was parsed from.
    pub(crate) redump: Mutex<Option<(String, Arc<RedumpIndex>)>>,
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Net")
            .field("data_dir", &self.data_dir)
            .finish()
    }
}

/// `%LOCALAPPDATA%\yaoplm`, `~/.local/share/yaoplm`, `~/Library/Application
/// Support/yaoplm`; `YAOPLM_DATA_DIR` overrides it (Docker, tests).
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("YAOPLM_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("yaoplm")
}

impl Net {
    pub(crate) fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            runtime: OnceLock::new(),
            service: OnceLock::new(),
            redump: Mutex::new(None),
        }
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, ApiError> {
        self.runtime
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("yaoplm-net")
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| ApiError::internal(format!("cannot start the network runtime: {e}")))
    }

    pub(crate) fn service(&self) -> Result<Arc<ArtService>, ApiError> {
        self.service
            .get_or_init(|| {
                ArtService::open(&self.data_dir)
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(|e| ApiError::internal(format!("cannot open the art cache: {e}")))
    }

    /// Runs a future to completion on the network runtime, from a job
    /// thread.
    pub(crate) fn block_on<F: std::future::Future>(
        &self,
        future: F,
    ) -> Result<F::Output, ApiError> {
        Ok(self.runtime()?.block_on(future))
    }

    pub(crate) fn cache_info(&self) -> Result<ArtCacheInfo, ApiError> {
        let service = self.service()?;
        let (objects, bytes) = service
            .cache()
            .stats()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(ArtCacheInfo {
            path: display_path(service.cache().root()),
            objects,
            bytes,
        })
    }

    pub(crate) fn clear_cache(&self) -> Result<(), ApiError> {
        self.service()?
            .cache()
            .clear()
            .map_err(|e| ApiError::internal(e.to_string()))
    }
}

/// Source settings from a request, in priority order, with mirrors checked
/// against the path policy.
pub(crate) fn source_configs(
    policy: &PathPolicy,
    settings: &[ArtSourceSetting],
) -> Result<Vec<SourceConfig>, ApiError> {
    let mut configs = Vec::new();
    for id in SourceId::ALL {
        let setting = settings.iter().find(|s| s.id == id.as_str());
        if setting.is_some_and(|s| s.enabled == Some(false)) {
            continue;
        }
        match setting
            .and_then(|s| s.mirror.as_deref())
            .filter(|m| !m.is_empty())
        {
            Some(mirror) => {
                let dir = policy.resolve(mirror)?;
                if !dir.is_dir() {
                    return Err(ApiError::bad_request(format!("{mirror} is not a folder")));
                }
                // Without the Windows verbatim prefix: the path ends up in
                // the plan and the log.
                configs.push(SourceConfig::mirror(id, PathBuf::from(display_path(&dir))));
            }
            None => configs.push(SourceConfig::remote(id)),
        }
    }
    for setting in settings {
        if SourceId::parse(&setting.id).is_none() {
            return Err(ApiError::bad_request(format!(
                "unknown art source {}",
                setting.id
            )));
        }
    }
    Ok(configs)
}

pub(crate) fn art_kind(kind: Option<&str>) -> Result<ArtKind, ApiError> {
    match kind {
        None => Ok(ArtKind::Cov),
        Some(k) => ArtKind::from_suffix(&k.to_ascii_uppercase())
            .filter(|k| *k == ArtKind::Cov)
            .ok_or_else(|| ApiError::bad_request(format!("art type {k} is not supported yet"))),
    }
}

/// A game as the fetch job needs it.
struct Target {
    startup: String,
    title: String,
    media: yaoplm_core::Media,
    format: String,
    has_cover: bool,
}

fn targets(root: &Path, profile: LoaderProfile, kind: ArtKind) -> Result<Vec<Target>, ApiError> {
    let options = ScanOptions {
        profile,
        verify_images: false,
    };
    let library = yaoplm_core::scan_library(root, &options)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let mut seen = std::collections::HashSet::new();
    Ok(library
        .games
        .iter()
        .filter(|g| seen.insert(g.startup.to_lowercase()))
        .map(|g| Target {
            startup: g.startup.clone(),
            title: g.title.clone(),
            media: g.media,
            format: g.format.to_string(),
            has_cover: g.art.iter().any(|a| {
                a.suffix.eq_ignore_ascii_case(kind.suffix())
                    && a.path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| {
                            profile
                                .art_extensions()
                                .iter()
                                .any(|x| x.eq_ignore_ascii_case(e))
                        })
            }),
        })
        .collect())
}

pub(crate) struct FetchParams {
    pub root: PathBuf,
    pub profile: WireProfile,
    pub scope: ArtScope,
    /// Only these IDs (either spelling); `None` means every game.
    pub games: Option<Vec<String>>,
    pub sources: Vec<SourceConfig>,
    pub kind: ArtKind,
}

/// The body of a `FetchArt` job.
pub(crate) fn fetch_job(
    ctx: &JobContext,
    net: &Net,
    plans: &PlanStore,
    params: FetchParams,
) -> Result<JobResult, ApiError> {
    let profile: LoaderProfile = params.profile.into();
    let service = net.service()?;
    let wanted: Option<Vec<String>> = params
        .games
        .as_ref()
        .map(|ids| ids.iter().map(|id| serial::lookup_key(id)).collect());
    let all = targets(&params.root, profile, params.kind)?;
    let mut already = 0usize;
    let targets: Vec<Target> = all
        .into_iter()
        .filter(|t| {
            wanted
                .as_ref()
                .is_none_or(|ids| ids.contains(&serial::lookup_key(&t.startup)))
        })
        .filter(|t| {
            if params.scope == ArtScope::Missing && t.has_cover {
                already += 1;
                false
            } else {
                true
            }
        })
        .collect();
    let total = targets.len() as u64;
    ctx.progress(0, Some(total), None);

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        if ctx.is_cancelled() {
            return Err(ApiError::new(ErrorCode::Cancelled, "cancelled"));
        }
        ctx.progress(index as u64, Some(total), Some(&target.title));
        let serial_id = serial::opl_to_serial(&target.startup);
        let entry = titles::database().lookup(&target.startup);
        let names: Vec<String> = entry
            .map(|e| {
                e.redump_names_by_region()
                    .iter()
                    .map(|n| n.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let size = recommended_size(
            profile,
            ArtSubject::Game {
                startup: &target.startup,
            },
            params.kind,
        );
        let request = CoverRequest {
            serial: &serial_id,
            redump_names: &names,
            size,
            sources: &params.sources,
            revalidate: params.scope == ArtScope::All,
        };
        let outcome = net.block_on(service.fetch_cover(&request))?;
        let (state, source, detail) = match &outcome.cover {
            Some(cover) => (
                if cover.from_cache {
                    ArtLogState::Cached
                } else {
                    ArtLogState::Found
                },
                Some(cover.source.clone()),
                Some(cover.origin.clone()),
            ),
            None => {
                let errors: Vec<String> = outcome
                    .attempts
                    .iter()
                    .filter_map(|a| match &a.result {
                        AttemptResult::Error(e) => Some(format!("{}: {e}", a.source)),
                        _ => None,
                    })
                    .collect();
                if errors.is_empty() {
                    let tried: Vec<&str> =
                        outcome.attempts.iter().map(|a| a.source.as_str()).collect();
                    let mut tried = tried;
                    tried.dedup();
                    (
                        ArtLogState::NotFound,
                        None,
                        Some(if tried.is_empty() {
                            "no source to ask".to_owned()
                        } else {
                            format!("not in {}", tried.join(", "))
                        }),
                    )
                } else {
                    (ArtLogState::Failed, None, Some(errors.join("; ")))
                }
            }
        };
        ctx.log(JobLogEntry::Art {
            at: now_ms(),
            startup: target.startup.clone(),
            title: target.title.clone(),
            art: params.kind.suffix().to_owned(),
            state,
            source: source.clone(),
            detail: detail.clone(),
        });
        match outcome.cover {
            Some(cover) => items.push(ArtItem {
                startup: target.startup.clone(),
                title: target.title.clone(),
                media: target.media,
                format: target.format.clone(),
                kind: params.kind,
                file: cover.png,
                origin: format!("{} ({})", cover.source, cover.origin),
            }),
            None => skipped.push(Skipped {
                path: PathBuf::from(format!("{} {}", target.startup, target.title)),
                reason: if state == ArtLogState::Failed {
                    Notice::warning("coverFailed", detail.unwrap_or_default())
                } else {
                    Notice::info("coverNotFound", detail.unwrap_or_default())
                },
            }),
        }
    }
    ctx.progress(total, Some(total), None);

    let fs = HostFs::new(&params.root).map_err(|e| ApiError::internal(e.to_string()))?;
    let mut plan = plan_set_art(&fs, &HostSources, &items, params.scope == ArtScope::All)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    plan.skipped.extend(skipped);
    if already > 0 {
        plan.notices.push(Notice::info(
            "coversPresent",
            format!("{already} games already have a cover and were left alone"),
        ));
    }
    if targets.is_empty() {
        plan.notices.retain(|n| n.code != "nothingToWrite");
        plan.notices.push(Notice::error(
            "nothingToFetch",
            if already > 0 {
                "every game already has a cover"
            } else {
                "no game to fetch a cover for"
            },
        ));
    }
    Ok(JobResult::Plan(Box::new(plans.insert(params.root, plan))))
}

/// Decodes `input` into image bytes and a label for the log.
pub(crate) fn input_bytes(
    policy: &PathPolicy,
    input: &ArtInput,
) -> Result<(Vec<u8>, String), ApiError> {
    const MAX_INPUT: u64 = 64 << 20;
    match input {
        ArtInput::File { path } => {
            let path = policy.resolve(path)?;
            let meta = std::fs::metadata(&path)
                .map_err(|e| ApiError::bad_request(format!("{}: {e}", display_path(&path))))?;
            if !meta.is_file() {
                return Err(ApiError::bad_request(format!(
                    "{} is not a file",
                    display_path(&path)
                )));
            }
            if meta.len() > MAX_INPUT {
                return Err(ApiError::bad_request(format!(
                    "{} is too large for a cover",
                    display_path(&path)
                )));
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| ApiError::bad_request(format!("{}: {e}", display_path(&path))))?;
            Ok((bytes, display_path(&path)))
        }
        ArtInput::Bytes { data, name } => {
            if data.len() as u64 > MAX_INPUT * 4 / 3 {
                return Err(ApiError::bad_request("the image is too large for a cover"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| ApiError::bad_request(format!("image data is not base64: {e}")))?;
            Ok((
                bytes,
                name.clone().unwrap_or_else(|| "clipboard".to_owned()),
            ))
        }
    }
}

/// What `SetArt` asks for.
pub(crate) struct SetParams {
    pub root: PathBuf,
    pub startup: String,
    pub kind: ArtKind,
    pub profile: WireProfile,
    pub input: ArtInput,
    pub overwrite: bool,
}

/// Plans writing a cover the user supplied (ART-3).
pub(crate) fn set_art(
    net: &Net,
    policy: &PathPolicy,
    params: &SetParams,
) -> Result<yaoplm_core::plan::Plan, ApiError> {
    let profile: LoaderProfile = params.profile.into();
    let (bytes, label) = input_bytes(policy, &params.input)?;
    let service = net.service()?;
    let size = recommended_size(
        profile,
        ArtSubject::Game {
            startup: &params.startup,
        },
        params.kind,
    );
    let cover = service
        .import_local(&bytes, size, &label)
        .map_err(|e| ApiError::bad_request(format!("{label}: {e}")))?;
    let game = targets(&params.root, profile, params.kind)?
        .into_iter()
        .find(|t| t.startup.eq_ignore_ascii_case(&params.startup))
        .ok_or_else(|| {
            ApiError::not_found(format!("no game with ID {} on this device", params.startup))
        })?;
    let item = ArtItem {
        startup: game.startup,
        title: game.title,
        media: game.media,
        format: game.format,
        kind: params.kind,
        file: cover.png,
        origin: label,
    };
    let fs = HostFs::new(&params.root).map_err(|e| ApiError::internal(e.to_string()))?;
    plan_set_art(&fs, &HostSources, &[item], params.overwrite)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub(crate) fn delete_art(
    root: &Path,
    startup: &str,
    kind: ArtKind,
) -> Result<yaoplm_core::plan::Plan, ApiError> {
    let fs = HostFs::new(root).map_err(|e| ApiError::internal(e.to_string()))?;
    plan_delete_art(&fs, startup, kind).map_err(crate::op_error)
}
