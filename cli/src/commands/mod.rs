//! One-shot command handlers and the dispatcher wiring the clap surface to them.

pub mod auth;
pub mod catalog;
pub mod collection;
pub mod decks;
pub mod holdings;
pub mod misc;
pub mod public;
pub mod wishlist;

use std::path::PathBuf;

use anyhow::Result;

use crate::cli::{Cli, Command};
use crate::client::Client;
use crate::config::{Auth, Config, DEFAULT_BASE_URL};
use crate::output::Printer;

/// Everything a command handler needs: a configured API client, the output
/// printer, and the resolved config path (for commands that mutate config).
pub struct Ctx {
    pub client: Client,
    pub printer: Printer,
    pub config_path: PathBuf,
}

/// Resolve base URL + credential precedence and build the client context.
pub fn build_ctx(cli: &Cli) -> Result<Ctx> {
    let config_path = Config::resolve_path(cli.config.clone())?;
    let config = Config::load(&config_path)?;

    let base_url = cli
        .url
        .clone()
        .or_else(|| config.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    // Credential precedence: --api-key, then --token (both ephemeral, never
    // persisted), else the stored credential (which we persist rotations back to).
    let (auth, persist_to) = if let Some(key) = &cli.api_key {
        (
            Some(Auth::ApiKey {
                key: key.clone(),
                scope: None,
            }),
            None,
        )
    } else if let Some(token) = &cli.token {
        (
            Some(Auth::ApiKey {
                key: token.clone(),
                scope: None,
            }),
            None,
        )
    } else {
        (config.auth.clone(), Some(config_path.clone()))
    };

    let client = Client::new(base_url, auth, persist_to)?;
    Ok(Ctx {
        client,
        printer: Printer::new(cli.json),
        config_path,
    })
}

/// Dispatch a parsed CLI to the matching handler. A missing subcommand (or `tui`)
/// launches the interactive terminal UI.
pub async fn dispatch(cli: Cli) -> Result<()> {
    let ctx = build_ctx(&cli)?;

    let command = match cli.command {
        None | Some(Command::Tui) => return crate::tui::run(ctx).await,
        Some(cmd) => cmd,
    };

    match command {
        Command::Login(a) => auth::login(&ctx, a).await,
        Command::Logout => auth::logout(&ctx).await,
        Command::Register(a) => auth::register(&ctx, a).await,
        Command::CompleteRegistration(a) => auth::complete_registration(&ctx, a).await,
        Command::Whoami => auth::whoami(&ctx).await,
        Command::Status => auth::status(&ctx).await,
        Command::Config(a) => misc::config(&ctx, a),
        Command::Auth(a) => auth::store_key(&ctx, a).await,
        Command::ApiKeys(a) => auth::api_keys(&ctx, a).await,
        Command::Currency(a) => auth::currency(&ctx, a).await,
        Command::Username(a) => auth::username(&ctx, a).await,

        Command::Games => catalog::games(&ctx).await,
        Command::Sets(a) => catalog::sets(&ctx, a).await,
        Command::Set(a) => catalog::set(&ctx, a).await,
        Command::Cards(a) => catalog::cards(&ctx, a).await,
        Command::Card(a) => catalog::card(&ctx, a).await,
        Command::CardNames(a) => catalog::card_names(&ctx, a).await,
        Command::Prices(a) => catalog::prices(&ctx, a).await,
        Command::Prints(a) => catalog::prints(&ctx, a).await,
        Command::Sealed(a) => catalog::sealed(&ctx, a).await,
        Command::Scan(a) => catalog::scan(&ctx, a).await,
        Command::Products(a) => catalog::products(&ctx, a).await,
        Command::Product(a) => catalog::product(&ctx, a).await,
        Command::Ingest(a) => catalog::ingest(&ctx, a).await,
        Command::Image(a) => catalog::image(&ctx, a).await,

        Command::Collection(a) => collection::run(&ctx, a).await,
        Command::Wishlist(a) => wishlist::run(&ctx, a).await,
        Command::Decks(a) => decks::run(&ctx, a).await,
        Command::Public(a) => public::run(&ctx, a).await,

        Command::Health => misc::health(&ctx).await,
        Command::Ready => misc::ready(&ctx).await,
        Command::ServerConfig => misc::server_config(&ctx).await,
        Command::Currencies => misc::currencies(&ctx).await,
        Command::Openapi(a) => misc::openapi(&ctx, a).await,
        Command::Update(a) => misc::update(&ctx, a).await,

        Command::Tui => crate::tui::run(ctx).await,
    }
}

// -- shared query helpers ---------------------------------------------------

/// Push `key=val` onto a query vec when the option is set.
pub fn push_opt<T: ToString>(
    q: &mut Vec<(&'static str, String)>,
    key: &'static str,
    val: &Option<T>,
) {
    if let Some(v) = val {
        q.push((key, v.to_string()));
    }
}

/// Push `key=true` onto a query vec when the flag is set.
pub fn push_flag(q: &mut Vec<(&'static str, String)>, key: &'static str, val: bool) {
    if val {
        q.push((key, "true".to_string()));
    }
}
