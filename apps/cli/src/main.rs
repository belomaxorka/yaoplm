//! `yaoplm`: command-line front end. Subcommands go through the same
//! `yaoplm_api::Api::dispatch` as the desktop app and the server.
//!
//! Every write prints its plan first and runs only with `--yes` or after a
//! "y" on an interactive terminal (SPEC §6, ARCHITECTURE.md §5).

mod report;

use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use yaoplm_api::art::{ArtInput, ArtScope, ArtSourceSetting, ArtType};
use yaoplm_api::jobs::{JobInfo, JobLogEntry, JobResult, JobStatus};
use yaoplm_api::library::{Game, GameFormat, Library, LoaderProfile, Media};
use yaoplm_api::plans::PlanView;
use yaoplm_api::verify::{RedumpUpdate, VerifyStatus};
use yaoplm_api::{
    Api, ConvertTarget, GameRef, JobEvent, JobId, Layout, NameStyle, RecoveryAction, Request,
    Response, Transfer,
};

#[derive(Debug, Parser)]
#[command(
    name = "yaoplm",
    version,
    about = "Manage PS2 game libraries for Open PS2 Loader"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Send a raw JSON request, e.g. '{"type":"Ping"}', and print the JSON answer.
    ///
    /// Reads the request from stdin when REQUEST is omitted or "-". Requests
    /// that start a job wait for it and print its final state. Exits with
    /// status 1 and prints the error object when the request fails.
    Rpc {
        request: Option<String>,
        /// Pretty-print the answer.
        #[arg(long)]
        pretty: bool,
    },
    /// List USB and SD volumes with a verdict for the loader (--all: every volume).
    Devices {
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Check one device or folder: file system, cluster, partition, junk files.
    Device {
        root: PathBuf,
        /// OPL itself is started from this device (FreeMcBoot, old uLaunchELF).
        #[arg(long)]
        boot_from_device: bool,
        #[command(flatten)]
        common: Common,
    },
    /// List a device root (or its usb_prefix folder) the way OPL would:
    /// games in CD/, DVD/ and ul.cfg, apps, and what OPL skips.
    Scan {
        root: PathBuf,
        /// Trust IDs from file names and games.bin like OPL does, without
        /// opening those images (faster, misses mismatches).
        #[arg(long)]
        no_verify: bool,
        #[command(flatten)]
        common: Common,
    },
    /// Show what OPL would make of one ISO or ZSO file.
    Info {
        image: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    /// Create the OPL folders (CD DVD CFG CHT VMC ART THM LNG APPS).
    Init {
        root: PathBuf,
        #[command(flatten)]
        apply: Apply,
    },
    /// Copy ISO, ZSO, BIN/CUE or CHD images (also from zip and 7z archives,
    /// or folders of them) to a device.
    ///
    /// On FAT32, games over 4 GiB are split into ul chunks. File names are
    /// ID.Title.iso unless --name-style title is given.
    Add {
        root: PathBuf,
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        /// Delete each source once its game is on the device.
        #[arg(long = "move")]
        move_sources: bool,
        /// Store as ul chunks even where one image would fit.
        #[arg(long)]
        ul: bool,
        /// Store images as ZSO (OPL 1.2.0 beta; not with ul chunks).
        #[arg(long, conflicts_with = "ul")]
        zso: bool,
        /// Skip the dump check: by default every image is hashed while it
        /// is written and compared with the stored Redump DAT.
        #[arg(long)]
        no_verify: bool,
        /// Title the games the bundled database knows by its name there;
        /// the others keep the title of their file name.
        #[arg(long)]
        db_titles: bool,
        /// Look for the covers of the games being added (as `art fetch`
        /// does) and write them together with the games.
        #[arg(long)]
        covers: bool,
        /// With --covers: read a source from a folder with its repository
        /// layout, xlenore=DIR or libretro=DIR. Repeatable.
        #[arg(long = "mirror", value_name = "SOURCE=DIR", requires = "covers")]
        mirrors: Vec<String>,
        /// With --covers: skip a source, xlenore or libretro. Repeatable.
        #[arg(long = "skip-source", value_name = "SOURCE", requires = "covers")]
        skipped: Vec<String>,
        /// Title for the game (one source only).
        #[arg(long)]
        title: Option<String>,
        /// Override the CD/DVD guess for every source.
        #[arg(long, value_enum)]
        media: Option<MediaArg>,
        #[arg(long, value_enum)]
        name_style: Option<StyleArg>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Rename a game. GAME is its ID, title, file name, path, or ul:<index>.
    Rename {
        root: PathBuf,
        game: String,
        title: String,
        /// Correct the game ID too; CFG, art and cheats move with it.
        #[arg(long = "id")]
        startup: Option<String>,
        #[arg(long, value_enum)]
        name_style: Option<StyleArg>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Delete a game. Its CFG, art and cheats stay unless --with-related.
    Rm {
        root: PathBuf,
        game: String,
        /// Also delete CFG, art and cheats when no other game uses the ID.
        #[arg(long)]
        with_related: bool,
        #[command(flatten)]
        apply: Apply,
    },
    /// Convert games between ISO, ZSO and ul chunks. The old form is
    /// deleted once the new one is complete; the game ID, and with it the
    /// CFG, art and cheats, stays.
    Convert {
        root: PathBuf,
        /// ID, title, file name, path or ul:<index>.
        #[arg(required = true)]
        games: Vec<String>,
        #[arg(long, value_enum)]
        to: FormatArg,
        /// How images made from ul chunks are named.
        #[arg(long, value_enum)]
        name_style: Option<StyleArg>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Move a game between CD/ and DVD/ (ul games: change the media byte).
    Mv {
        root: PathBuf,
        game: String,
        #[arg(long, value_enum)]
        to: MediaArg,
        #[command(flatten)]
        apply: Apply,
    },
    /// Finish or undo operations a crash or an unplugged device left half done.
    Recover {
        root: PathBuf,
        /// Redo unfinished work instead of undoing it.
        #[arg(long)]
        resume: bool,
        #[command(flatten)]
        apply: Apply,
    },
    /// Covers: download them, set one from a file, remove one.
    Art {
        #[command(subcommand)]
        command: ArtCommand,
    },
    /// Check games against Redump: CRC-32 and SHA-1 (and MD5 with --md5)
    /// in one read of each image, then a match with the stored DAT.
    ///
    /// Unchanged images are not read again (hash cache). Exits with 1 when
    /// a game is a bad dump or cannot be read.
    Verify {
        root: PathBuf,
        /// Only this game: ID, title, file name, path or ul:<index>. Repeatable.
        #[arg(long = "game", value_name = "GAME")]
        games: Vec<String>,
        /// Also compute MD5.
        #[arg(long)]
        md5: bool,
        /// Read every image even when its hashes are cached: finds damage
        /// that left a file's size and time alone.
        #[arg(long)]
        fresh: bool,
        #[command(flatten)]
        common: Common,
    },
    /// The Redump DAT used by `verify`: show it, download a newer one (at
    /// most once a week), or import a .dat or Redump .zip you have.
    Redump {
        /// Ask Redump for a newer DAT (at most once a week, whatever the
        /// answer).
        #[arg(long)]
        update: bool,
        /// Store this .dat or Redump .zip instead; no network.
        #[arg(long, value_name = "FILE", conflicts_with = "update")]
        import: Option<PathBuf>,
        /// Print the answer as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Credits and licences of the bundled data and the cover sources.
    About {
        /// Print the answer as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ArtCommand {
    /// Find covers (xlenore/ps2-covers, then libretro-thumbnails) for the
    /// games on a device, then write them after confirmation.
    ///
    /// Only games without a cover by default. Downloads are cached under
    /// the app data folder; a local mirror of a source (a clone of its
    /// repository) replaces the network with --mirror.
    Fetch {
        root: PathBuf,
        /// Every game, replacing existing covers (the old files move to
        /// .yaoplm/backup/).
        #[arg(long)]
        all: bool,
        /// Only these game IDs (SLUS_200.62 or SLUS-20062). Repeatable.
        #[arg(long = "game", value_name = "ID")]
        games: Vec<String>,
        /// Read a source from a folder with its repository layout:
        /// xlenore=DIR or libretro=DIR. Repeatable.
        #[arg(long = "mirror", value_name = "SOURCE=DIR")]
        mirrors: Vec<String>,
        /// Skip a source: xlenore or libretro. Repeatable.
        #[arg(long = "skip-source", value_name = "SOURCE")]
        skipped: Vec<String>,
        /// Picture types to look for: COV (default), SCR, SCR2, LGO; ICO,
        /// COV2, LAB and BG have no open source. Repeatable.
        #[arg(long = "kind", value_name = "TYPE")]
        kinds: Vec<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Use an image file as a game's picture (resized and stored as PNG).
    Set {
        root: PathBuf,
        /// ID, title, file name, path or ul:<index> (see `yaoplm scan`).
        game: String,
        image: PathBuf,
        /// Leave an existing picture alone instead of replacing it.
        #[arg(long)]
        keep_existing: bool,
        /// The picture type: COV (default), COV2, LAB, ICO, LGO, SCR, SCR2
        /// or BG.
        #[arg(long, value_name = "TYPE")]
        kind: Option<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Delete a game's picture (a copy stays in .yaoplm/backup/).
    Rm {
        root: PathBuf,
        game: String,
        /// The picture type, COV by default.
        #[arg(long, value_name = "TYPE")]
        kind: Option<String>,
        #[command(flatten)]
        apply: Apply,
    },
    /// Show the art cache, or empty it with --clear.
    Cache {
        #[arg(long)]
        clear: bool,
        /// Print the answer as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct Common {
    #[arg(long, value_enum, default_value_t = Profile::Opl12)]
    profile: Profile,
    /// Print the API answer as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct Apply {
    /// Apply the plan without asking.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Only print the plan.
    #[arg(long, conflicts_with = "yes")]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = Profile::Opl12)]
    profile: Profile,
    /// Print the plan and the result as JSON.
    #[arg(long)]
    json: bool,
}

/// The loader the device is prepared for.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Profile {
    /// OPL 1.2.0 beta (default).
    #[value(name = "opl-1.2")]
    Opl12,
    /// OPL 1.1.0 stable.
    #[value(name = "opl-1.1")]
    Opl11,
    #[value(name = "riptopl")]
    RiptOpl,
}

impl From<Profile> for LoaderProfile {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Opl12 => Self::Opl120Beta,
            Profile::Opl11 => Self::Opl110,
            Profile::RiptOpl => Self::RiptOpl,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MediaArg {
    Cd,
    Dvd,
}

impl From<MediaArg> for Media {
    fn from(media: MediaArg) -> Self {
        match media {
            MediaArg::Cd => Self::CD,
            MediaArg::Dvd => Self::DVD,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FormatArg {
    Iso,
    Zso,
    Ul,
}

impl From<FormatArg> for ConvertTarget {
    fn from(format: FormatArg) -> Self {
        match format {
            FormatArg::Iso => Self::Iso,
            FormatArg::Zso => Self::Zso,
            FormatArg::Ul => Self::Ul,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StyleArg {
    /// SLUS_200.62.Title.iso (default).
    IdTitle,
    /// Title.iso.
    Title,
}

impl From<StyleArg> for NameStyle {
    fn from(style: StyleArg) -> Self {
        match style {
            StyleArg::IdTitle => Self::IdTitle,
            StyleArg::Title => Self::Title,
        }
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let api = Api::new();
    match cli.command {
        Command::Rpc { request, pretty } => rpc(&api, request, pretty),
        Command::Devices { all, common } => {
            let response = api.dispatch(Request::ListDevices {
                profile: Some(common.profile.into()),
            });
            show(response, common.json, |response| match response {
                Response::Devices { devices } => {
                    let shown: Vec<_> = devices
                        .iter()
                        .filter(|d| all || (d.external && !d.system))
                        .cloned()
                        .collect();
                    print!("{}", report::devices(&shown, all));
                }
                other => print_json(other, true),
            })
        }
        Command::Device {
            root,
            boot_from_device,
            common,
        } => {
            let response = api.dispatch(Request::InspectDevice {
                root: text(&root),
                profile: Some(common.profile.into()),
                boot_from_device: Some(boot_from_device),
            });
            show(response, common.json, |response| match response {
                Response::Device(device) => print!("{}", report::device(device)),
                other => print_json(other, true),
            })
        }
        Command::Scan {
            root,
            no_verify,
            common,
        } => {
            let library = match scan(&api, &root, common.profile, !no_verify) {
                Ok(library) => library,
                Err(err) => return fail(&err, common.json),
            };
            if common.json {
                print_json(&JobResult::Library(Box::new(library)), true);
            } else {
                print!("{}", report::library(&library));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Info { image, common } => {
            let response = api.dispatch(Request::ImageInfo {
                path: text(&image),
                profile: Some(common.profile.into()),
            });
            show(response, common.json, |response| match response {
                Response::ImageInfo(info) => print!("{}", report::image(info)),
                other => print_json(other, true),
            })
        }
        Command::Init { root, apply } => {
            plan_and_apply(&api, Request::PlanInit { root: text(&root) }, &apply)
        }
        Command::Add {
            root,
            sources,
            move_sources,
            ul,
            zso,
            no_verify,
            db_titles,
            covers,
            mirrors,
            skipped,
            title,
            media,
            name_style,
            apply,
        } => {
            if title.is_some() && sources.len() != 1 {
                bail!("--title needs exactly one source");
            }
            let overrides: Vec<_> = sources
                .iter()
                .filter(|_| title.is_some() || media.is_some())
                .map(|source| yaoplm_api::ImportOverride {
                    source: absolute(source),
                    title: title.clone(),
                    media: media.map(Into::into),
                })
                .collect();
            let mut request = Request::PlanImport {
                root: text(&root),
                sources: sources.iter().map(|s| absolute(s)).collect(),
                profile: Some(apply.profile.into()),
                name_style: name_style.map(Into::into),
                transfer: Some(if move_sources {
                    Transfer::Move
                } else {
                    Transfer::Copy
                }),
                layout: Some(if ul { Layout::Ul } else { Layout::Auto }),
                overrides: (!overrides.is_empty()).then_some(overrides),
                compress: Some(zso),
                verify: Some(!no_verify),
                database_titles: Some(db_titles),
                covers: None,
            };
            if covers {
                let found = find_import_covers(&api, &request, &root, &mirrors, &skipped, &apply)?;
                if let (Some(id), Request::PlanImport { covers, .. }) = (found, &mut request) {
                    *covers = Some(id);
                }
            }
            plan_and_apply(&api, request, &apply)
        }
        Command::Convert {
            root,
            games,
            to,
            name_style,
            apply,
        } => {
            let refs = find_games(&api, &root, &games, apply.profile)?;
            let request = Request::ConvertGames {
                root: text(&root),
                games: refs,
                target: to.into(),
                profile: Some(apply.profile.into()),
                name_style: name_style.map(Into::into),
            };
            plan_and_apply(&api, request, &apply)
        }
        Command::Rename {
            root,
            game,
            title,
            startup,
            name_style,
            apply,
        } => {
            let game = find_game(&api, &root, &game, apply.profile)?;
            let request = Request::RenameGame {
                root: text(&root),
                game,
                title,
                startup,
                name_style: name_style.map(Into::into),
            };
            plan_and_apply(&api, request, &apply)
        }
        Command::Rm {
            root,
            game,
            with_related,
            apply,
        } => {
            let game = find_game(&api, &root, &game, apply.profile)?;
            let request = Request::DeleteGame {
                root: text(&root),
                game,
                related: Some(with_related),
            };
            plan_and_apply(&api, request, &apply)
        }
        Command::Mv {
            root,
            game,
            to,
            apply,
        } => {
            let game = find_game(&api, &root, &game, apply.profile)?;
            let request = Request::MoveGame {
                root: text(&root),
                game,
                media: to.into(),
            };
            plan_and_apply(&api, request, &apply)
        }
        Command::Recover {
            root,
            resume,
            apply,
        } => {
            let request = Request::PlanRecovery {
                root: text(&root),
                action: Some(if resume {
                    RecoveryAction::Resume
                } else {
                    RecoveryAction::Rollback
                }),
            };
            plan_and_apply(&api, request, &apply)
        }
        Command::Art { command } => art(&api, command),
        Command::Verify {
            root,
            games,
            md5,
            fresh,
            common,
        } => {
            let refs = if games.is_empty() {
                Vec::new()
            } else {
                find_games(&api, &root, &games, common.profile)?
            };
            let request = Request::VerifyGames {
                root: text(&root),
                games: (!refs.is_empty()).then_some(refs),
                profile: Some(common.profile.into()),
                md5: Some(md5),
                fresh: Some(fresh),
            };
            let job = match api.dispatch(request) {
                Ok(Response::Job { job }) => job,
                Ok(other) => bail!("unexpected answer {other:?}"),
                Err(error) => return fail(&error, common.json),
            };
            let info = wait_job(&api, job, io::stderr().is_terminal() && !common.json);
            match info.result {
                Some(JobResult::Verification(report)) => {
                    if common.json {
                        print_json(&report, true);
                    } else {
                        print!("{}", report::verification(&report));
                    }
                    let bad = report
                        .games
                        .iter()
                        .any(|g| matches!(g.status, VerifyStatus::BadDump | VerifyStatus::Failed));
                    Ok(if bad {
                        ExitCode::FAILURE
                    } else {
                        ExitCode::SUCCESS
                    })
                }
                _ => fail(
                    &info
                        .error
                        .unwrap_or_else(|| yaoplm_api::ApiError::internal("verification failed")),
                    common.json,
                ),
            }
        }
        Command::Redump {
            update,
            import,
            json,
        } => {
            if !update && import.is_none() {
                return match api.dispatch(Request::RedumpStatus) {
                    Ok(Response::Redump { dat }) => {
                        if json {
                            print_json(&dat, true);
                        } else {
                            print!("{}", report::redump(dat.as_deref()));
                        }
                        Ok(ExitCode::SUCCESS)
                    }
                    Ok(other) => bail!("unexpected answer {other:?}"),
                    Err(error) => fail(&error, json),
                };
            }
            let job = match api.dispatch(Request::UpdateRedump {
                file: import.as_deref().map(absolute),
            }) {
                Ok(Response::Job { job }) => job,
                Ok(other) => bail!("unexpected answer {other:?}"),
                Err(error) => return fail(&error, json),
            };
            let info = wait_job(&api, job, false);
            match info.result {
                Some(JobResult::Redump(result)) => {
                    if json {
                        print_json(&result, true);
                    } else {
                        let what = match result.update {
                            RedumpUpdate::Downloaded => "Downloaded a new DAT.".to_owned(),
                            RedumpUpdate::NotModified => "Redump has nothing newer.".to_owned(),
                            RedumpUpdate::TooSoon => format!(
                                "Redump was asked less than a week ago; next request after {}.",
                                result
                                    .next_check
                                    .map_or_else(|| "a week".to_owned(), report::date)
                            ),
                            RedumpUpdate::Imported => "Imported.".to_owned(),
                        };
                        println!("{what}");
                        print!("{}", report::redump(result.dat.as_ref()));
                    }
                    Ok(ExitCode::SUCCESS)
                }
                _ => fail(
                    &info
                        .error
                        .unwrap_or_else(|| yaoplm_api::ApiError::internal("the update failed")),
                    json,
                ),
            }
        }
        Command::About { json } => {
            let response = api.dispatch(Request::Attribution);
            show(response, json, |response| match response {
                Response::Attribution(about) => print!("{}", report::about(about)),
                other => print_json(other, true),
            })
        }
    }
}

/// Plans `request` once to learn its games, then searches their covers
/// (ART-8). The id of the cover set, or `None` when there is nothing to
/// search or the search failed: the games are then added without covers.
fn find_import_covers(
    api: &Api,
    request: &Request,
    root: &Path,
    mirrors: &[String],
    skipped: &[String],
    apply: &Apply,
) -> anyhow::Result<Option<String>> {
    let plan = match api.dispatch(request.clone()) {
        Ok(Response::Plan(plan)) => plan,
        // The real planning call reports the problem.
        _ => return Ok(None),
    };
    let games = plan.transactions.iter().any(|t| t.game.is_some());
    let sources = art_sources(mirrors, skipped)?;
    let job = games.then(|| {
        api.dispatch(Request::FetchArt {
            root: text(root),
            profile: Some(apply.profile.into()),
            scope: None,
            games: None,
            sources: (!sources.is_empty()).then_some(sources),
            kind: None,
            kinds: None,
            import: Some(plan.id.clone()),
        })
    });
    let found = match job {
        Some(Ok(Response::Job { job })) => {
            let info = wait_job(api, job, io::stderr().is_terminal() && !apply.json);
            match (info.status, info.result) {
                (JobStatus::Finished, Some(JobResult::Covers(search))) => {
                    if !apply.json {
                        eprintln!(
                            "Covers: {} found, {} not found, {} already on the device",
                            search.found, search.missing, search.present
                        );
                    }
                    Some(search.id)
                }
                _ => {
                    let message = info
                        .error
                        .map_or_else(|| "the cover search failed".to_owned(), |e| e.message);
                    eprintln!("yaoplm: {message}; the games are added without covers");
                    None
                }
            }
        }
        Some(Err(error)) => {
            eprintln!(
                "yaoplm: {}; the games are added without covers",
                error.message
            );
            None
        }
        _ => None,
    };
    let _ = api.dispatch(Request::DiscardPlan { plan: plan.id });
    Ok(found)
}

/// `xlenore=DIR` and `--skip-source` flags as the API wants them.
fn art_sources(mirrors: &[String], skipped: &[String]) -> anyhow::Result<Vec<ArtSourceSetting>> {
    let mut settings = Vec::new();
    for mirror in mirrors {
        let (id, dir) = mirror
            .split_once('=')
            .with_context(|| format!("--mirror takes SOURCE=DIR, got {mirror:?}"))?;
        settings.push(ArtSourceSetting {
            id: id.to_owned(),
            enabled: None,
            mirror: Some(absolute(Path::new(dir))),
        });
    }
    for id in skipped {
        settings.push(ArtSourceSetting {
            id: id.clone(),
            enabled: Some(false),
            mirror: None,
        });
    }
    Ok(settings)
}

fn art(api: &Api, command: ArtCommand) -> anyhow::Result<ExitCode> {
    match command {
        ArtCommand::Fetch {
            root,
            all,
            games,
            mirrors,
            skipped,
            kinds,
            apply,
        } => {
            let sources = art_sources(&mirrors, &skipped)?;
            let kinds = kinds
                .iter()
                .map(|k| ArtType::parse(k).with_context(|| format!("unknown picture type {k:?}")))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let request = Request::FetchArt {
                root: text(&root),
                profile: Some(apply.profile.into()),
                scope: Some(if all {
                    ArtScope::All
                } else {
                    ArtScope::Missing
                }),
                games: (!games.is_empty()).then_some(games),
                sources: (!sources.is_empty()).then_some(sources),
                kind: None,
                kinds: (!kinds.is_empty()).then_some(kinds),
                import: None,
            };
            let job = match api.dispatch(request) {
                Ok(Response::Job { job }) => job,
                Ok(other) => bail!("unexpected answer {other:?}"),
                Err(error) => return fail(&error, apply.json),
            };
            let info = wait_job(api, job, io::stderr().is_terminal() && !apply.json);
            let plan = match (info.status, info.result) {
                (JobStatus::Finished, Some(JobResult::Plan(plan))) => plan,
                (_, _) => {
                    let message = info
                        .error
                        .map(|e| e.message)
                        .unwrap_or_else(|| "the cover search failed".into());
                    if apply.json {
                        print_json(&yaoplm_api::ApiError::internal(message), true);
                    } else {
                        eprintln!("yaoplm: {message}");
                    }
                    return Ok(ExitCode::FAILURE);
                }
            };
            apply_plan(api, plan, &apply)
        }
        ArtCommand::Set {
            root,
            game,
            image,
            keep_existing,
            kind,
            apply,
        } => {
            let startup = find_startup(api, &root, &game, apply.profile)?;
            let request = Request::SetArt {
                root: text(&root),
                startup,
                input: ArtInput::File {
                    path: absolute(&image),
                },
                profile: Some(apply.profile.into()),
                kind,
                overwrite: Some(!keep_existing),
            };
            plan_and_apply(api, request, &apply)
        }
        ArtCommand::Rm {
            root,
            game,
            kind,
            apply,
        } => {
            let startup = find_startup(api, &root, &game, apply.profile)?;
            let request = Request::DeleteArt {
                root: text(&root),
                startup,
                kind,
            };
            plan_and_apply(api, request, &apply)
        }
        ArtCommand::Cache { clear, json } => {
            if clear && let Err(error) = api.dispatch(Request::ClearArtCache) {
                return fail(&error, json);
            }
            let response = api.dispatch(Request::ArtCache);
            show(response, json, |response| match response {
                Response::ArtCache(cache) => println!(
                    "{}: {} objects, {}{}",
                    cache.path,
                    cache.objects,
                    report::size(cache.bytes),
                    if clear { " (cleared)" } else { "" }
                ),
                other => print_json(other, true),
            })
        }
    }
}

/// The game ID behind a `<game>` argument.
fn find_startup(api: &Api, root: &Path, query: &str, profile: Profile) -> anyhow::Result<String> {
    Ok(match find_game(api, root, query, profile)? {
        GameRef::Ul { startup, .. } => startup,
        GameRef::Image { path } => {
            let library =
                scan(api, root, profile, false).map_err(|e| anyhow::anyhow!(e.message))?;
            library
                .games
                .iter()
                .find(|g| g.path == path)
                .map(|g| g.startup.clone())
                .with_context(|| format!("{path} vanished"))?
        }
    })
}

/// Sources are resolved against the current folder before the API sees them.
fn absolute(path: &Path) -> String {
    std::path::absolute(path).map_or_else(|_| text(path), |p| text(&p))
}

fn fail(error: &yaoplm_api::ApiError, json: bool) -> anyhow::Result<ExitCode> {
    if json {
        print_json(error, true);
    } else {
        eprintln!("yaoplm: {}", error.message);
    }
    Ok(ExitCode::FAILURE)
}

/// Prints an answer as JSON or as text.
fn show(
    response: Result<Response, yaoplm_api::ApiError>,
    json: bool,
    text: impl FnOnce(&Response),
) -> anyhow::Result<ExitCode> {
    match response {
        Ok(response) if json => print_json(&response, true),
        Ok(response) => text(&response),
        Err(error) => return fail(&error, json),
    }
    Ok(ExitCode::SUCCESS)
}

/// Starts a job and waits for it; progress goes to stderr on a terminal.
fn wait_job(api: &Api, job: JobId, progress: bool) -> JobInfo {
    let line = Arc::new(Mutex::new(false));
    let shown = Arc::clone(&line);
    let _subscription = progress.then(|| {
        api.subscribe(move |event| match event {
            JobEvent::Progress {
                job: id,
                done,
                total,
                rate,
                item,
                ..
            } if *id == job => {
                let mut stderr = io::stderr().lock();
                let _ = write!(
                    stderr,
                    "\r\x1b[2K{}",
                    report::progress(*done, *total, *rate, item.as_deref())
                );
                let _ = stderr.flush();
                *shown.lock().unwrap_or_else(|e| e.into_inner()) = true;
            }
            JobEvent::Log {
                job: id,
                entry: entry @ JobLogEntry::Art { .. },
            } if *id == job => {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "\r\x1b[2K{}", report::art_line(entry));
                let _ = stderr.flush();
            }
            JobEvent::Log {
                job: id,
                entry: entry @ JobLogEntry::Verify { .. },
            } if *id == job => {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "\r\x1b[2K{}", report::verify_line(entry));
                let _ = stderr.flush();
            }
            _ => {}
        })
    });
    let info = api.jobs().wait(job).unwrap_or_else(|| JobInfo {
        id: job,
        kind: yaoplm_api::jobs::JobKind::ExecutePlan,
        title: String::new(),
        title_phrase: None,
        root: None,
        status: JobStatus::Failed,
        done: 0,
        total: None,
        rate: None,
        item: None,
        item_phrase: None,
        result: None,
        error: Some(yaoplm_api::ApiError::internal("the job vanished")),
        log: Vec::new(),
    });
    if *line.lock().unwrap_or_else(|e| e.into_inner()) {
        eprintln!();
    }
    info
}

fn scan(
    api: &Api,
    root: &Path,
    profile: Profile,
    verify: bool,
) -> Result<Library, yaoplm_api::ApiError> {
    let Response::Job { job } = api.dispatch(Request::ScanLibrary {
        root: text(root),
        profile: Some(profile.into()),
        verify: Some(verify),
    })?
    else {
        return Err(yaoplm_api::ApiError::internal(
            "ScanLibrary did not start a job",
        ));
    };
    let info = wait_job(api, job, false);
    match info.result {
        Some(JobResult::Library(library)) => Ok(*library),
        _ => Err(info
            .error
            .unwrap_or_else(|| yaoplm_api::ApiError::internal("the scan failed"))),
    }
}

fn game_ref(game: &Game) -> GameRef {
    match (game.format, game.ul_index) {
        (GameFormat::UL, Some(index)) => GameRef::Ul {
            index,
            startup: game.startup.clone(),
            name: game.title.clone(),
        },
        _ => GameRef::Image {
            path: game.path.clone(),
        },
    }
}

/// Finds a game by ID, title, file name, path or `ul:<index>`.
fn find_game(api: &Api, root: &Path, query: &str, profile: Profile) -> anyhow::Result<GameRef> {
    let library = scan(api, root, profile, false).map_err(|e| anyhow::anyhow!(e.message))?;
    match_game(&library, query)
}

/// Several games from one scan.
fn find_games(
    api: &Api,
    root: &Path,
    queries: &[String],
    profile: Profile,
) -> anyhow::Result<Vec<GameRef>> {
    let library = scan(api, root, profile, false).map_err(|e| anyhow::anyhow!(e.message))?;
    queries.iter().map(|q| match_game(&library, q)).collect()
}

fn match_game(library: &Library, query: &str) -> anyhow::Result<GameRef> {
    if let Some(index) = query.strip_prefix("ul:") {
        let index: u32 = index.parse().context("ul:<index> takes a number")?;
        let game = library
            .games
            .iter()
            .find(|g| g.ul_index == Some(index))
            .with_context(|| format!("ul.cfg has no record {index}"))?;
        return Ok(game_ref(game));
    }
    let lower = query.to_lowercase();
    let wanted = std::path::absolute(query)
        .ok()
        .map(|p| text(&p).to_lowercase());
    let matches: Vec<&Game> = library
        .games
        .iter()
        .filter(|g| {
            let path = g.path.to_lowercase();
            g.startup.eq_ignore_ascii_case(query)
                || g.title.to_lowercase() == lower
                || (g.format != GameFormat::UL
                    && (Path::new(&g.path)
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase() == lower)
                        || wanted.as_deref() == Some(path.as_str())
                        || path
                            .replace('\\', "/")
                            .ends_with(&format!("/{}", lower.replace('\\', "/")))))
        })
        .collect();
    match matches.as_slice() {
        [game] => Ok(game_ref(game)),
        [] => bail!("no game matches {query:?}; see `yaoplm scan`"),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|g| match g.ul_index {
                    Some(i) => format!("  ul:{i}  {} {}", g.startup, g.title),
                    None => format!("  {}  {} {}", g.path, g.startup, g.title),
                })
                .collect();
            bail!(
                "{} games match {query:?}; name one by path or ul:<index>:\n{}",
                many.len(),
                list.join("\n")
            )
        }
    }
}

fn confirm(plan: &PlanView) -> anyhow::Result<bool> {
    let question = if plan.deletes_user_data {
        format!(
            "This deletes files ({}). Apply the plan? [y/N] ",
            report::size(plan.delete_bytes)
        )
    } else {
        "Apply the plan? [y/N] ".to_owned()
    };
    eprint!("{question}");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// Plans, prints the plan, asks, executes, reports.
fn plan_and_apply(api: &Api, request: Request, apply: &Apply) -> anyhow::Result<ExitCode> {
    let plan = match api.dispatch(request) {
        Ok(Response::Plan(plan)) => plan,
        Ok(other) => bail!("unexpected answer {other:?}"),
        Err(error) => return fail(&error, apply.json),
    };
    apply_plan(api, plan, apply)
}

/// Prints a plan, asks, executes, reports.
fn apply_plan(api: &Api, plan: Box<PlanView>, apply: &Apply) -> anyhow::Result<ExitCode> {
    if apply.json {
        print_json(&plan, true);
    } else {
        print!("{}", report::plan(&plan));
    }
    if !plan.executable {
        if !apply.json {
            eprintln!("yaoplm: nothing to apply");
        }
        let blocked = plan.notices.iter().any(|n| {
            n.level == yaoplm_api::plans::NoticeLevel::Error
                && !matches!(n.code.as_str(), "nothingToRecover" | "foldersPresent")
        });
        return Ok(if blocked {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        });
    }
    if apply.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    let go = if apply.yes {
        true
    } else if io::stdin().is_terminal() && !apply.json {
        confirm(&plan)?
    } else {
        eprintln!("yaoplm: dry run; add --yes to apply the plan");
        return Ok(ExitCode::SUCCESS);
    };
    if !go {
        eprintln!("yaoplm: nothing was changed");
        return Ok(ExitCode::SUCCESS);
    }
    let job = match api.dispatch(Request::ExecutePlan {
        plan: plan.id.clone(),
        // The user confirmed the plan as printed, deletions included.
        confirm_delete: Some(true),
    }) {
        Ok(Response::Job { job }) => job,
        Ok(other) => bail!("unexpected answer {other:?}"),
        Err(error) => return fail(&error, apply.json),
    };
    let info = wait_job(api, job, io::stderr().is_terminal() && !apply.json);
    if apply.json {
        print_json(&info, true);
    }
    match (&info.status, &info.result) {
        (JobStatus::Finished, Some(JobResult::Execution(result))) => {
            if !apply.json {
                print!("{}", report::execution(&plan, result));
            }
            Ok(if result.error.is_none() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        (JobStatus::Cancelled, Some(JobResult::Execution(result))) => {
            if !apply.json {
                print!("{}", report::execution(&plan, result));
            }
            Ok(ExitCode::FAILURE)
        }
        _ => {
            let message = info
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "the job failed".into());
            if !apply.json {
                eprintln!("yaoplm: {message}");
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

fn rpc(api: &Api, request: Option<String>, pretty: bool) -> anyhow::Result<ExitCode> {
    let json = match request {
        Some(json) if json != "-" => json,
        _ => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read the request from stdin")?;
            buf
        }
    };

    // PowerShell pipes text to native programs with a UTF-8 BOM.
    let json = json.strip_prefix('\u{feff}').unwrap_or(&json);

    match api.dispatch_json(json) {
        // The job would die with this process: wait for it.
        Ok(Response::Job { job }) => {
            let info = wait_job(api, job, false);
            let failed = info.status != JobStatus::Finished;
            print_json(&Response::JobState(Box::new(info)), pretty);
            Ok(if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Ok(response) => {
            print_json(&response, pretty);
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            print_json(&error, pretty);
            Ok(ExitCode::FAILURE)
        }
    }
}

fn print_json(value: &impl Serialize, pretty: bool) {
    let text = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    };
    match text {
        Ok(text) => println!("{text}"),
        Err(err) => eprintln!("yaoplm: cannot print JSON: {err}"),
    }
}
