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
use yaoplm_core::art::{ArtItem, kind_name, plan_delete_art, plan_set_art};
use yaoplm_core::plan::{Notice, Skipped};
use yaoplm_core::{LoaderProfile, Phrase as Said, ScanOptions, titles};
use yaoplm_device::{HostFs, HostSources};
use yaoplm_formats::art::{ArtKind, ArtSubject, recommended_size};
use yaoplm_formats::serial;
use yaoplm_hash::RedumpIndex;
use yaoplm_net::sources::{SourceConfig, SourceId};
use yaoplm_net::{ArtService, AttemptResult, CoverRequest};

use crate::jobs::{JobContext, JobLogEntry, JobResult, now_ms};
use crate::library::LoaderProfile as WireProfile;
use crate::paths::{PathPolicy, display_path};
use crate::phrase::Phrase;
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

/// What a cover search for an import plan found (ART-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CoverSearch {
    /// Pass to `PlanImport` as `covers`.
    pub id: String,
    /// Pictures found, of every type asked for.
    pub found: u32,
    /// Pictures no source had.
    pub missing: u32,
    /// Pictures the device has already.
    pub present: u32,
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
    /// The picture types a cover source has (ART-2); empty for data.
    #[serde(default)]
    pub kinds: Vec<ArtType>,
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
        .map_err(|e| ApiError::internal_error(format!("attribution.json: {e}")))?;
    let info = titles::database().info();
    Ok(Attribution {
        name: crate::NAME.to_owned(),
        version: crate::VERSION.to_owned(),
        license: "GPL-3.0-or-later".to_owned(),
        repository: "https://github.com/belomaxorka/yaoplm".to_owned(),
        data: file.data,
        art: file
            .art
            .into_iter()
            .map(|mut entry| {
                entry.kinds = SourceId::parse(&entry.id)
                    .map(|s| s.kinds().iter().copied().map(ArtType::from).collect())
                    .unwrap_or_default();
                entry
            })
            .collect(),
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
    /// Kept once opened. A cache that could not be opened (another process
    /// held it too long) is tried again on the next call, not for the life
    /// of the process.
    service: Mutex<Option<Arc<ArtService>>>,
    /// The parsed Redump DAT and the SHA-256 it was parsed from.
    pub(crate) redump: Mutex<Option<(String, Arc<RedumpIndex>)>>,
    /// One DAT download or import at a time: two at once would both pass
    /// the weekly check and both ask Redump.
    pub(crate) redump_update: Mutex<()>,
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
            service: Mutex::new(None),
            redump: Mutex::new(None),
            redump_update: Mutex::new(()),
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
            .map_err(|e| ApiError::internal_error(format!("cannot start the network runtime: {e}")))
    }

    pub(crate) fn service(&self) -> Result<Arc<ArtService>, ApiError> {
        let mut service = self
            .service
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(service) = service.as_ref() {
            return Ok(Arc::clone(service));
        }
        let opened = ArtService::open(&self.data_dir)
            .map(Arc::new)
            .map_err(|e| ApiError::internal_error(format!("cannot open the art cache: {e}")))?;
        *service = Some(Arc::clone(&opened));
        Ok(opened)
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
        let (objects, bytes) = service.cache().stats().map_err(ApiError::internal_error)?;
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
            .map_err(ApiError::internal_error)
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
                    return Err(ApiError::phrased(
                        ErrorCode::BadRequest,
                        &Said::error("notAFolder").with("path", mirror),
                    ));
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
            return Err(ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("unknownArtSource").with("id", &setting.id),
            ));
        }
    }
    Ok(configs)
}

/// A picture type on the wire, as its file suffix (ART-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "UPPERCASE")]
pub enum ArtType {
    Cov,
    Cov2,
    Lab,
    Ico,
    Lgo,
    Scr,
    Scr2,
    Bg,
}

impl ArtType {
    /// `cov`, `COV2`...: a suffix in either case.
    pub fn parse(suffix: &str) -> Option<Self> {
        ArtKind::from_suffix(&suffix.to_ascii_uppercase()).map(Into::into)
    }
}

impl From<ArtType> for ArtKind {
    fn from(kind: ArtType) -> Self {
        match kind {
            ArtType::Cov => Self::Cov,
            ArtType::Cov2 => Self::Cov2,
            ArtType::Lab => Self::Lab,
            ArtType::Ico => Self::Ico,
            ArtType::Lgo => Self::Lgo,
            ArtType::Scr => Self::Scr,
            ArtType::Scr2 => Self::Scr2,
            ArtType::Bg => Self::Bg,
        }
    }
}

impl From<ArtKind> for ArtType {
    fn from(kind: ArtKind) -> Self {
        match kind {
            ArtKind::Cov => Self::Cov,
            ArtKind::Cov2 => Self::Cov2,
            ArtKind::Lab => Self::Lab,
            ArtKind::Ico => Self::Ico,
            ArtKind::Lgo => Self::Lgo,
            ArtKind::Scr => Self::Scr,
            ArtKind::Scr2 => Self::Scr2,
            ArtKind::Bg => Self::Bg,
        }
    }
}

/// A picture type named by its suffix, COV when none is given.
pub(crate) fn art_kind(kind: Option<&str>) -> Result<ArtKind, ApiError> {
    match kind {
        None => Ok(ArtKind::Cov),
        Some(k) => ArtKind::from_suffix(&k.to_ascii_uppercase()).ok_or_else(|| {
            ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("artKindUnsupported").with("kind", k),
            )
        }),
    }
}

/// A game as the fetch job needs it.
#[derive(Clone)]
pub(crate) struct Target {
    pub startup: String,
    pub title: String,
    pub media: yaoplm_core::Media,
    pub format: String,
    /// Picture types the device has for it, in a format the profile loads.
    pub present: Vec<ArtKind>,
}

/// The games on the device, once per ID.
pub(crate) fn targets(root: &Path, profile: LoaderProfile) -> Result<Vec<Target>, ApiError> {
    let options = ScanOptions {
        profile,
        verify_images: false,
    };
    let library = yaoplm_core::scan_library(root, &options).map_err(|e| crate::scan_error(&e))?;
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
            present: g
                .art
                .iter()
                .filter(|a| {
                    a.path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| {
                            profile
                                .art_extensions()
                                .iter()
                                .any(|x| x.eq_ignore_ascii_case(e))
                        })
                })
                .filter_map(|a| ArtKind::from_suffix(&a.suffix.to_ascii_uppercase()))
                .collect(),
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
    /// The picture types to look for, at least one.
    pub kinds: Vec<ArtKind>,
    /// The games of an import plan, and how many have the picture already
    /// (ART-8): the job then ends with a cover set, not a plan.
    pub import: Option<(Vec<Target>, usize)>,
}

/// The games an import plan adds, once per ID, each with the `kinds` the
/// device has for it already (ART-8, scope `missing`), and how many
/// pictures that is. Games with every kind present are left out.
pub(crate) fn import_targets(
    plans: &PlanStore,
    plan_id: &str,
    root: &Path,
    kinds: &[ArtKind],
) -> Result<(Vec<Target>, usize), ApiError> {
    let (plan, plan_root) = plans.peek(plan_id).ok_or_else(|| {
        ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("planExpired").with("id", plan_id),
        )
    })?;
    if plan_root != root || plan.kind != yaoplm_core::plan::PlanKind::Import {
        return Err(ApiError::phrased(
            ErrorCode::BadRequest,
            &Said::error("notAnImportPlan").with("id", plan_id),
        ));
    }
    let fs = HostFs::new(root).map_err(ApiError::internal_error)?;
    let present = kinds
        .iter()
        .map(|&kind| Ok((kind, yaoplm_core::art::present_art(&fs, kind)?)))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(ApiError::internal_error)?;
    let mut seen = std::collections::HashSet::new();
    let mut already = 0;
    let mut targets = Vec::new();
    for game in plan.transactions.iter().filter_map(|t| t.game.as_ref()) {
        if !seen.insert(serial::lookup_key(&game.startup)) {
            continue;
        }
        let key = game.startup.to_lowercase();
        let have: Vec<ArtKind> = present
            .iter()
            .filter(|(_, ids)| ids.contains(&key))
            .map(|(kind, _)| *kind)
            .collect();
        already += have.len();
        if have.len() == kinds.len() {
            continue;
        }
        targets.push(Target {
            startup: game.startup.clone(),
            title: game.title.clone(),
            media: game.media,
            format: game.format.clone(),
            present: have,
        });
    }
    Ok((targets, already))
}

/// The Redump names of `startup`, region first: libretro names its files
/// after them.
fn redump_names(startup: &str) -> Vec<String> {
    titles::database()
        .lookup(startup)
        .map(|e| {
            e.redump_names_by_region()
                .iter()
                .map(|n| n.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Why nothing was found for a picture, for the log and the plan: no
/// source has the type, every source is off, the game has no Redump name
/// to look up, nothing at the sources asked, or errors.
fn why_missing(
    kind: ArtKind,
    sources: &[SourceConfig],
    redump_names: &[String],
    attempts: &[yaoplm_net::Attempt],
) -> (ArtLogState, Said) {
    let errors: Vec<String> = attempts
        .iter()
        .filter_map(|a| match &a.result {
            AttemptResult::Error(e) => Some(format!("{}: {e}", a.source)),
            _ => None,
        })
        .collect();
    if !errors.is_empty() {
        return (
            ArtLogState::Failed,
            Said::notice("artFailed").with("errors", errors.join("; ")),
        );
    }
    let mut tried: Vec<&str> = attempts.iter().map(|a| a.source.as_str()).collect();
    tried.dedup();
    let phrase = if !tried.is_empty() {
        Said::notice("artNotFound").with("sources", tried.join(", "))
    } else if !SourceId::ALL.iter().any(|s| s.kinds().contains(&kind)) {
        Said::notice("noSourceForKind").with("kind", kind_name(kind))
    } else if sources.is_empty() {
        Said::notice("noArtSource")
    } else if redump_names.is_empty() && sources.iter().any(|s| s.id.kinds().contains(&kind)) {
        // A source that is on has the type but had nowhere to look: it
        // finds pictures by the Redump name the bundled database gives.
        Said::notice("notInTitleDatabase")
    } else {
        // The sources that are on do not have this type.
        Said::notice("noSourceForKind").with("kind", kind_name(kind))
    };
    (ArtLogState::NotFound, phrase)
}

/// The body of a `FetchArt` job.
pub(crate) fn fetch_job(
    ctx: &JobContext,
    net: &Net,
    plans: &PlanStore,
    mut params: FetchParams,
) -> Result<JobResult, ApiError> {
    let profile: LoaderProfile = params.profile.into();
    let service = net.service()?;
    let for_import = params.import.is_some();
    let only_covers = params.kinds == [ArtKind::Cov];
    let (pairs, already) = match params.import.take() {
        Some((targets, already)) => (
            targets
                .into_iter()
                .flat_map(|t| {
                    params
                        .kinds
                        .iter()
                        .filter(|kind| !t.present.contains(kind))
                        .map(|&kind| (t.clone(), kind))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            already,
        ),
        None => {
            let wanted: Option<Vec<String>> = params
                .games
                .as_ref()
                .map(|ids| ids.iter().map(|id| serial::lookup_key(id)).collect());
            let mut already = 0usize;
            let mut pairs = Vec::new();
            for target in targets(&params.root, profile)? {
                if wanted
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(&serial::lookup_key(&target.startup)))
                {
                    continue;
                }
                let kinds: Vec<ArtKind> = params
                    .kinds
                    .iter()
                    .copied()
                    .filter(|kind| {
                        let present =
                            params.scope == ArtScope::Missing && target.present.contains(kind);
                        already += usize::from(present);
                        !present
                    })
                    .collect();
                for kind in kinds {
                    pairs.push((target.clone(), kind));
                }
            }
            (pairs, already)
        }
    };
    let total = pairs.len() as u64;
    ctx.progress(0, Some(total), None);

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut missing = Vec::new();
    let mut not_found = 0usize;
    for (index, (target, kind)) in pairs.iter().enumerate() {
        if ctx.is_cancelled() {
            return Err(ApiError::cancelled());
        }
        ctx.progress(index as u64, Some(total), Some(&target.title));
        let serial_id = serial::opl_to_serial(&target.startup);
        let names = redump_names(&target.startup);
        let size = recommended_size(
            profile,
            ArtSubject::Game {
                startup: &target.startup,
            },
            *kind,
        );
        let request = CoverRequest {
            serial: &serial_id,
            redump_names: &names,
            kind: *kind,
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
                None,
            ),
            None => {
                let (state, phrase) =
                    why_missing(*kind, &params.sources, &names, &outcome.attempts);
                (state, None, Some(phrase))
            }
        };
        ctx.log(JobLogEntry::Art {
            at: now_ms(),
            startup: target.startup.clone(),
            title: target.title.clone(),
            art: kind.suffix().to_owned(),
            state,
            source: source.clone(),
            // Where it came from, or why it is missing.
            detail: outcome
                .cover
                .as_ref()
                .map(|cover| cover.origin.clone())
                .or_else(|| detail.as_ref().map(Said::english)),
            detail_phrase: detail.as_ref().map(Phrase::from),
        });
        match outcome.cover {
            Some(cover) => items.push(ArtItem {
                startup: target.startup.clone(),
                title: target.title.clone(),
                media: target.media,
                format: target.format.clone(),
                kind: *kind,
                file: cover.png,
                origin: format!("{} ({})", cover.source, cover.origin),
            }),
            None => {
                let label = if *kind == ArtKind::Cov {
                    format!("{} {}", target.startup, target.title)
                } else {
                    format!("{} {} ({kind})", target.startup, target.title)
                };
                let skip = Skipped {
                    path: PathBuf::from(label),
                    reason: match (state, detail) {
                        (ArtLogState::Failed, Some(phrase)) => Notice::warning(phrase),
                        (_, Some(phrase)) => Notice::info(phrase),
                        (_, None) => Notice::info(Said::notice("noArtSource")),
                    },
                };
                not_found += 1;
                // An import plan lists the games left without a cover; the
                // other types are optional extras, counted in the result.
                if *kind == ArtKind::Cov {
                    missing.push((target.startup.clone(), skip.clone()));
                }
                skipped.push(skip);
            }
        }
    }
    ctx.progress(total, Some(total), None);

    if for_import {
        let found = u32::try_from(items.len()).unwrap_or(u32::MAX);
        let id = plans.insert_covers(params.root, items, missing);
        return Ok(JobResult::Covers(CoverSearch {
            id,
            found,
            missing: u32::try_from(not_found).unwrap_or(u32::MAX),
            present: u32::try_from(already).unwrap_or(u32::MAX),
        }));
    }

    let fs = HostFs::new(&params.root).map_err(ApiError::internal_error)?;
    let mut plan = plan_set_art(&fs, &HostSources, &items, params.scope == ArtScope::All)
        .map_err(ApiError::internal_error)?;
    plan.skipped.extend(skipped);
    if already > 0 {
        plan.notices.push(Notice::info(
            Said::notice(if only_covers {
                "coversPresent"
            } else {
                "artPresent"
            })
            .count(already),
        ));
    }
    if pairs.is_empty() {
        plan.notices.retain(|n| n.code != "nothingToWrite");
        plan.notices.push(Notice::error(Said::notice(match () {
            () if already == 0 => "nothingToFetch",
            () if only_covers => "allHaveCovers",
            () => "allHaveArt",
        })));
    }
    Ok(JobResult::Plan(Box::new(plans.insert(params.root, plan))))
}

/// A picture a source has for one game, for the user to pick (SPEC §5.4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ArtVariant {
    pub source: String,
    /// The URL or mirror file it came from.
    pub origin: String,
    pub width: u32,
    pub height: u32,
    /// The PNG as it would be written, base64: pass it back to `SetArt`.
    pub png: String,
}

/// What `FindArt` found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ArtVariants {
    pub startup: String,
    pub kind: ArtType,
    /// In source order: the first is what "Download" would write.
    pub variants: Vec<ArtVariant>,
}

pub(crate) struct FindParams {
    pub root: PathBuf,
    pub profile: WireProfile,
    pub startup: String,
    pub kind: ArtKind,
    pub sources: Vec<SourceConfig>,
}

/// The body of a `FindArt` job: every picture of one type the sources
/// have for one game, resized as it would be written.
pub(crate) fn find_job(
    ctx: &JobContext,
    net: &Net,
    params: FindParams,
) -> Result<JobResult, ApiError> {
    let profile: LoaderProfile = params.profile.into();
    let service = net.service()?;
    let game = targets(&params.root, profile)?
        .into_iter()
        .find(|t| t.startup.eq_ignore_ascii_case(&params.startup))
        .ok_or_else(|| {
            ApiError::phrased(
                ErrorCode::NotFound,
                &Said::error("noGameWithId").with("startup", &params.startup),
            )
        })?;
    ctx.progress(0, Some(1), Some(&game.title));
    let serial_id = serial::opl_to_serial(&game.startup);
    let names = redump_names(&game.startup);
    let request = CoverRequest {
        serial: &serial_id,
        redump_names: &names,
        kind: params.kind,
        size: recommended_size(
            profile,
            ArtSubject::Game {
                startup: &game.startup,
            },
            params.kind,
        ),
        sources: &params.sources,
        revalidate: false,
    };
    let (covers, attempts) = net.block_on(service.find_variants(&request))?;
    let mut variants = Vec::new();
    for cover in &covers {
        let Ok(bytes) = std::fs::read(&cover.png) else {
            continue;
        };
        ctx.log(JobLogEntry::Art {
            at: now_ms(),
            startup: game.startup.clone(),
            title: game.title.clone(),
            art: params.kind.suffix().to_owned(),
            state: if cover.from_cache {
                ArtLogState::Cached
            } else {
                ArtLogState::Found
            },
            source: Some(cover.source.clone()),
            detail: Some(cover.origin.clone()),
            detail_phrase: None,
        });
        variants.push(ArtVariant {
            source: cover.source.clone(),
            origin: cover.origin.clone(),
            width: cover.width,
            height: cover.height,
            png: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    if variants.is_empty() {
        let (state, phrase) = why_missing(params.kind, &params.sources, &names, &attempts);
        ctx.log(JobLogEntry::Art {
            at: now_ms(),
            startup: game.startup.clone(),
            title: game.title.clone(),
            art: params.kind.suffix().to_owned(),
            state,
            source: None,
            detail: Some(phrase.english()),
            detail_phrase: Some(Phrase::from(&phrase)),
        });
    }
    ctx.progress(1, Some(1), None);
    Ok(JobResult::ArtVariants(Box::new(ArtVariants {
        startup: game.startup,
        kind: params.kind.into(),
        variants,
    })))
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
            let meta = std::fs::metadata(&path).map_err(|e| path_error(&path, &e))?;
            if !meta.is_file() {
                return Err(ApiError::phrased(
                    ErrorCode::BadRequest,
                    &Said::error("notAFile").with("path", display_path(&path)),
                ));
            }
            if meta.len() > MAX_INPUT {
                return Err(ApiError::phrased(
                    ErrorCode::BadRequest,
                    &Said::error("imageTooLarge").with("path", display_path(&path)),
                ));
            }
            let bytes = std::fs::read(&path).map_err(|e| path_error(&path, &e))?;
            Ok((bytes, display_path(&path)))
        }
        ArtInput::Bytes { data, name } => {
            if data.len() as u64 > MAX_INPUT * 4 / 3 {
                return Err(ApiError::phrased(
                    ErrorCode::BadRequest,
                    &Said::error("pastedImageTooLarge"),
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| {
                    ApiError::phrased(
                        ErrorCode::BadRequest,
                        &Said::error("badImageData").with("error", e.to_string()),
                    )
                })?;
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
        .import_local(&bytes, params.kind, size, &label)
        .map_err(|e| {
            ApiError::phrased(
                ErrorCode::BadRequest,
                &Said::error("pathError")
                    .with("path", &label)
                    .with("error", e.to_string()),
            )
        })?;
    let game = targets(&params.root, profile)?
        .into_iter()
        .find(|t| t.startup.eq_ignore_ascii_case(&params.startup))
        .ok_or_else(|| {
            ApiError::phrased(
                ErrorCode::NotFound,
                &Said::error("noGameWithId").with("startup", &params.startup),
            )
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
    let fs = HostFs::new(&params.root).map_err(ApiError::internal_error)?;
    plan_set_art(&fs, &HostSources, &[item], params.overwrite).map_err(ApiError::internal_error)
}

/// A file the user picked that cannot be read.
fn path_error(path: &Path, err: &std::io::Error) -> ApiError {
    ApiError::phrased(
        ErrorCode::BadRequest,
        &Said::error("pathError")
            .with("path", display_path(path))
            .with("error", err.to_string()),
    )
}

pub(crate) fn delete_art(
    root: &Path,
    startup: &str,
    kind: ArtKind,
) -> Result<yaoplm_core::plan::Plan, ApiError> {
    let fs = HostFs::new(root).map_err(ApiError::internal_error)?;
    plan_delete_art(&fs, startup, kind).map_err(crate::op_error)
}
