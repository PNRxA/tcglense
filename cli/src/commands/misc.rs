//! Config management and server/meta commands (health, ready, config, currencies,
//! OpenAPI).

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

use super::Ctx;
use crate::config::Config;
use crate::models::{CurrencyRatesResponse, PublicConfig};
use crate::output::table;

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the stored configuration.
    Show,
    /// Set the API base URL persisted in the config file.
    Url { url: String },
    /// Print the config file path.
    Path,
}

#[derive(Debug, Args)]
pub struct OpenapiArgs {
    /// Write the document to this file instead of stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Only report whether a newer release exists; don't download or install.
    #[arg(long)]
    pub check: bool,
    /// Update without prompting for confirmation.
    #[arg(short = 'y', long)]
    pub yes: bool,
}

pub async fn update(ctx: &Ctx, args: UpdateArgs) -> Result<()> {
    use crate::update::Outcome;
    // Suppress the download progress bar in JSON mode.
    let outcome = crate::update::run(args.check, args.yes, ctx.printer.json).await?;
    match outcome {
        Outcome::UpToDate { current } => {
            if ctx.printer.json {
                ctx.printer
                    .json(&serde_json::json!({ "status": "up_to_date", "current": current }))?;
            } else {
                println!("Already up to date (tcglense {current}).");
            }
        }
        Outcome::Available { current, latest } => {
            if ctx.printer.json {
                ctx.printer.json(
                    &serde_json::json!({ "status": "available", "current": current, "latest": latest }),
                )?;
            } else {
                println!("A newer version is available: {current} → {latest}.");
                println!("Run `tcglense update` to install it.");
            }
        }
        Outcome::Updated { version } => {
            if ctx.printer.json {
                ctx.printer
                    .json(&serde_json::json!({ "status": "updated", "version": version }))?;
            } else {
                println!("Updated to tcglense {version}.");
            }
        }
        Outcome::Cancelled => ctx.printer.note("Update cancelled."),
        Outcome::NoAssetForPlatform { target, latest } => {
            anyhow::bail!(
                "release {latest} has no prebuilt binary for this platform ({target}); build from source with `cargo install`"
            );
        }
    }
    Ok(())
}

pub fn config(ctx: &Ctx, args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommand::Show => {
            let cfg = Config::load(&ctx.config_path)?;
            if ctx.printer.json {
                ctx.printer.json(&cfg)?;
            } else {
                println!(
                    "base_url   : {}",
                    cfg.base_url
                        .as_deref()
                        .unwrap_or(crate::config::DEFAULT_BASE_URL)
                );
                println!(
                    "credential : {}",
                    cfg.auth
                        .as_ref()
                        .map(|a| a.describe())
                        .unwrap_or_else(|| "(none)".into())
                );
                println!("path       : {}", ctx.config_path.display());
            }
        }
        ConfigCommand::Url { url } => {
            let mut cfg = Config::load(&ctx.config_path)?;
            cfg.base_url = Some(url.trim_end_matches('/').to_string());
            cfg.save(&ctx.config_path)?;
            ctx.printer
                .note(format!("Base URL set to {}.", cfg.base_url.unwrap()));
        }
        ConfigCommand::Path => {
            println!("{}", ctx.config_path.display());
        }
    }
    Ok(())
}

pub async fn health(ctx: &Ctx) -> Result<()> {
    let (status, body) = ctx.client.probe("/api/health").await?;
    print_probe(ctx, status, &body);
    Ok(())
}

pub async fn ready(ctx: &Ctx) -> Result<()> {
    let (status, body) = ctx.client.probe("/api/ready").await?;
    print_probe(ctx, status, &body);
    Ok(())
}

fn print_probe(ctx: &Ctx, status: u16, body: &str) {
    if ctx.printer.json {
        let value = serde_json::from_str::<serde_json::Value>(body)
            .unwrap_or(serde_json::Value::String(body.to_string()));
        let _ = ctx
            .printer
            .json(&serde_json::json!({ "status_code": status, "body": value }));
    } else {
        println!("HTTP {status}: {}", body.trim());
    }
}

pub async fn server_config(ctx: &Ctx) -> Result<()> {
    let cfg: PublicConfig = ctx.client.get_json("/api/config", &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&cfg)?;
    } else {
        println!("maintenance_mode : {}", cfg.maintenance_mode);
        println!("signups_enabled  : {}", cfg.signups_enabled);
        println!(
            "turnstile        : {}",
            if cfg.turnstile_site_key.is_some() {
                "enabled"
            } else {
                "disabled"
            }
        );
        if let Some(m) = cfg.signups_disabled_message {
            println!("signups_message  : {m}");
        }
    }
    Ok(())
}

pub async fn currencies(ctx: &Ctx) -> Result<()> {
    let rates: CurrencyRatesResponse = ctx.client.get_json("/api/currencies", &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&rates)?;
    } else {
        println!("Base {} as of {}", rates.base, rates.as_of);
        let mut t = table(&["Currency", "Rate (per USD)"]);
        for (code, rate) in &rates.rates {
            t.add_row(vec![code.clone(), format!("{rate}")]);
        }
        println!("{t}");
    }
    Ok(())
}

pub async fn openapi(ctx: &Ctx, args: OpenapiArgs) -> Result<()> {
    let body = ctx.client.get_text("/api/openapi.json", &[]).await?;
    match args.output {
        Some(path) => {
            std::fs::write(&path, body.as_bytes())?;
            ctx.printer
                .note(format!("Wrote OpenAPI document to {}.", path.display()));
        }
        None => println!("{body}"),
    }
    Ok(())
}
