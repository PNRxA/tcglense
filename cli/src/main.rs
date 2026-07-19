//! `tcglense` — command-line client for the TCGLense API.

mod cli;
mod client;
mod commands;
mod config;
mod models;
mod output;
mod tui;
mod update;

use clap::Parser;

use crate::cli::{Cli, Command};

#[tokio::main]
async fn main() {
    // reqwest's rustls backend uses the process-default crypto provider; install
    // ring (matching the CLI's rustls feature) once at startup.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    // After an ordinary one-shot command, run a throttled, best-effort check for a
    // newer release (skipped for `update` itself and the interactive TUI).
    let notify_after =
        matches!(&cli.command, Some(c) if !matches!(c, Command::Update(_) | Command::Tui));

    if let Err(err) = commands::dispatch(cli).await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
    if notify_after {
        update::maybe_notify().await;
    }
}
