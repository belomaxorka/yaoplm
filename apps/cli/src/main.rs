//! `yaoplm`: command-line front end. Subcommands go through the same
//! `yaoplm_api::Api::dispatch` as the desktop app and the server.

mod report;

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use yaoplm_api::library::LoaderProfile;
use yaoplm_api::{Api, Request, Response};

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
    /// Reads the request from stdin when REQUEST is omitted or "-". Exits with
    /// status 1 and prints the error object when the request fails.
    Rpc {
        request: Option<String>,
        /// Pretty-print the answer.
        #[arg(long)]
        pretty: bool,
    },
    /// List a device root (or its usb_prefix folder) the way OPL would:
    /// games in CD/, DVD/ and ul.cfg, apps, and what OPL skips.
    Scan {
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = Profile::Opl12)]
        profile: Profile,
        /// Trust IDs from file names and games.bin like OPL does, without
        /// opening those images (faster, misses mismatches).
        #[arg(long)]
        no_verify: bool,
        /// Print the API answer as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show what OPL would make of one ISO or ZSO file.
    Info {
        image: PathBuf,
        #[arg(long, value_enum, default_value_t = Profile::Opl12)]
        profile: Profile,
        /// Print the API answer as JSON.
        #[arg(long)]
        json: bool,
    },
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

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Rpc { request, pretty } => rpc(request, pretty),
        Command::Scan {
            root,
            profile,
            no_verify,
            json,
        } => run(
            Request::ScanLibrary {
                root: root.to_string_lossy().into_owned(),
                profile: Some(profile.into()),
                verify: Some(!no_verify),
            },
            json,
        ),
        Command::Info {
            image,
            profile,
            json,
        } => run(
            Request::ImageInfo {
                path: image.to_string_lossy().into_owned(),
                profile: Some(profile.into()),
            },
            json,
        ),
    }
}

/// Dispatches `request` and prints the answer as JSON or as text.
fn run(request: Request, json: bool) -> anyhow::Result<ExitCode> {
    let response = match Api::new().dispatch(request) {
        Ok(response) => response,
        Err(error) if json => return print_json(&error, true).map(|()| ExitCode::FAILURE),
        Err(error) => {
            eprintln!("yaoplm: {}", error.message);
            return Ok(ExitCode::FAILURE);
        }
    };
    if json {
        print_json(&response, true)?;
        return Ok(ExitCode::SUCCESS);
    }
    match &response {
        Response::Library(library) => print!("{}", report::library(library)),
        Response::ImageInfo(info) => print!("{}", report::image(info)),
        other => print_json(other, true)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn rpc(request: Option<String>, pretty: bool) -> anyhow::Result<ExitCode> {
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

    let api = Api::new();
    match api.dispatch_json(json) {
        Ok(response) => print_json(&response, pretty).map(|()| ExitCode::SUCCESS),
        Err(error) => print_json(&error, pretty).map(|()| ExitCode::FAILURE),
    }
}

fn print_json(value: &impl Serialize, pretty: bool) -> anyhow::Result<()> {
    let text = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    println!("{text}");
    Ok(())
}
