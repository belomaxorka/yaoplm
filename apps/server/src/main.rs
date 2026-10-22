use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;
use yaoplm_api::Api;
use yaoplm_server::{AppState, Config};

#[derive(Debug, Parser)]
#[command(
    name = "yaoplm-server",
    version,
    about = "Serve the YAOPLM web UI and API"
)]
struct Args {
    /// Address to listen on. A non-loopback address requires --token.
    #[arg(long, env = "YAOPLM_BIND", default_value = "127.0.0.1:7878")]
    bind: SocketAddr,

    /// Token for /api, sent as `Authorization: Bearer <token>`. Open the UI
    /// once as http://<host>/?token=<token> to store it in the browser.
    #[arg(long, env = "YAOPLM_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Library root the UI may open. Repeat for several roots.
    #[arg(long = "root", value_name = "DIR")]
    roots: Vec<PathBuf>,

    /// Folder for the art cache and other app data (ARCHITECTURE.md §6).
    /// Defaults to the platform's app data folder.
    #[arg(long, env = "YAOPLM_DATA_DIR", value_name = "DIR")]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let token = args.token.filter(|token| !token.is_empty());
    let loopback_only = args.bind.ip().is_loopback();
    if !loopback_only && token.is_none() {
        bail!(
            "--token is required when listening on {} (only loopback is open without a token)",
            args.bind
        );
    }

    let mut roots = Vec::with_capacity(args.roots.len());
    for root in &args.roots {
        let canonical = std::fs::canonicalize(root)
            .with_context(|| format!("library root {} is not accessible", root.display()))?;
        if !canonical.is_dir() {
            bail!("library root {} is not a directory", root.display());
        }
        tracing::info!(root = %canonical.display(), "library root");
        roots.push(canonical);
    }

    let mut api = Api::with_roots(roots.clone());
    if let Some(dir) = args.data_dir {
        api = api.with_data_dir(dir);
    }
    tracing::info!(dir = %api.data_dir().display(), "app data folder");
    let state = AppState::new(
        Arc::new(api),
        Config {
            token,
            roots,
            loopback_only,
        },
    );
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("failed to listen on {}", args.bind))?;
    tracing::info!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, yaoplm_server::app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
