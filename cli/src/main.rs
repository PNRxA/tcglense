//! `tcglense` — command-line client for the TCGLense API.

mod cli;
mod client;
mod commands;
mod config;
mod models;
mod output;
mod tui;

use clap::Parser;

use crate::cli::Cli;

#[tokio::main]
async fn main() {
    // reqwest's rustls backend uses the process-default crypto provider; install
    // ring (matching the CLI's rustls feature) once at startup.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    if let Err(err) = commands::dispatch(cli).await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
