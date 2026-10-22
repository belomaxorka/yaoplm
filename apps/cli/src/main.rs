//! `yaoplm`: command-line front end. Subcommands go through the same
//! `yaoplm_api::Api::dispatch` as the desktop app and the server.

use std::io::{self, Read};
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;
use yaoplm_api::Api;

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
}

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Rpc { request, pretty } => rpc(request, pretty),
    }
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

    let api = Api::new();
    match api.dispatch_json(&json) {
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
