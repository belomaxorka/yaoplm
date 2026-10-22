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
use yaoplm_api::jobs::{JobInfo, JobResult, JobStatus};
use yaoplm_api::library::{Game, GameFormat, Library, LoaderProfile, Media};
use yaoplm_api::plans::PlanView;
use yaoplm_api::{
    Api, GameRef, JobEvent, JobId, Layout, NameStyle, RecoveryAction, Request, Response, Transfer,
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
    /// Copy ISO, ZSO or BIN/CUE files (or folders of them) to a device.
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
            let request = Request::PlanImport {
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
    }
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
        api.subscribe(move |event| {
            if let JobEvent::Progress {
                job: id,
                done,
                total,
                rate,
                item,
            } = event
                && *id == job
            {
                let mut stderr = io::stderr().lock();
                let _ = write!(
                    stderr,
                    "\r\x1b[2K{}",
                    report::progress(*done, *total, *rate, item.as_deref())
                );
                let _ = stderr.flush();
                *shown.lock().unwrap_or_else(|e| e.into_inner()) = true;
            }
        })
    });
    let info = api.jobs().wait(job).unwrap_or_else(|| JobInfo {
        id: job,
        kind: yaoplm_api::jobs::JobKind::ExecutePlan,
        title: String::new(),
        status: JobStatus::Failed,
        done: 0,
        total: None,
        rate: None,
        item: None,
        result: None,
        error: Some(yaoplm_api::ApiError::internal("the job vanished")),
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
