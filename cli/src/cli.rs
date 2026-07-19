//! Command-line surface (clap). The top-level parser plus the `Command` enum; each
//! domain's subcommands + arguments are defined alongside their handlers in
//! `commands/<domain>.rs`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::{auth, catalog, collection, decks, misc, public, wishlist};

/// Command-line client for TCGLense — browse the card catalog, manage your
/// collection, wish list and decks, and mint API keys, via one-shot commands or an
/// interactive TUI.
///
/// Run with no subcommand (or `tcglense tui`) to launch the interactive browser.
#[derive(Debug, Parser)]
#[command(name = "tcglense", version, about, long_about = None, propagate_version = true)]
pub struct Cli {
    /// API base URL (overrides the stored config; default http://localhost:8080).
    #[arg(long, global = true, env = "TCGLENSE_URL")]
    pub url: Option<String>,

    /// Authenticate with a `tcgl_` API key for this invocation (not persisted).
    #[arg(long, global = true, env = "TCGLENSE_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// Authenticate with a raw bearer token for this invocation (not persisted).
    #[arg(long, global = true, env = "TCGLENSE_TOKEN", hide_env_values = true)]
    pub token: Option<String>,

    /// Path to the config file (default: $TCGLENSE_CONFIG or <config-dir>/tcglense/config.json).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Emit JSON instead of human-readable tables.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // -- authentication & account --
    /// Log in with email + password (web auth); stores a refreshable session.
    Login(auth::LoginArgs),
    /// Clear the stored credential (and revoke the session server-side).
    Logout,
    /// Start email-first registration (emails a completion link, or returns the
    /// completion token in the no-email dev posture).
    Register(auth::RegisterArgs),
    /// Finish registration with the completion token (sets the first password).
    #[command(name = "complete-registration", alias = "complete")]
    CompleteRegistration(auth::CompleteArgs),
    /// Show the authenticated account (`GET /api/auth/me`).
    Whoami,
    /// Show the current config: base URL and stored credential.
    Status,
    /// Get or set config values (the API base URL).
    Config(misc::ConfigArgs),
    /// Store a `tcgl_` API key credential (verified against the server).
    Auth(auth::AuthArgs),
    /// Manage `tcgl_` API keys (session auth only).
    #[command(name = "api-keys", alias = "apikey")]
    ApiKeys(auth::ApiKeysArgs),
    /// Show or set your preferred display currency.
    Currency(auth::CurrencyArgs),
    /// Choose or check a public username/handle.
    Username(auth::UsernameArgs),

    // -- catalog --
    /// List supported games.
    Games,
    /// List a game's sets.
    Sets(catalog::SetsArgs),
    /// Show one set (optionally its cards / drops / sub-types).
    Set(catalog::SetArgs),
    /// Search a game's cards (Scryfall-style `-q` query).
    Cards(catalog::CardsArgs),
    /// Show one card's full detail.
    Card(catalog::CardArgs),
    /// Autocomplete distinct card names.
    #[command(name = "card-names")]
    CardNames(catalog::CardNamesArgs),
    /// Show a card's price history.
    Prices(catalog::PricesArgs),
    /// Show a card's other printings.
    Prints(catalog::PrintsArgs),
    /// Show the sealed products a card is found in / can be pulled from.
    Sealed(catalog::SealedArgs),
    /// Identify a card from a 256-bit perceptual-hash fingerprint (auth required).
    Scan(catalog::ScanArgs),
    /// Browse sealed products.
    Products(catalog::ProductsArgs),
    /// Inspect one sealed product (detail / prices / contents / containers / cards).
    Product(catalog::ProductArgs),
    /// Show a game's card-data import status.
    Ingest(catalog::IngestArgs),
    /// Download a card or product image to a file.
    Image(catalog::ImageArgs),

    // -- per-user surfaces --
    /// Manage your card + sealed-product collection for a game.
    Collection(collection::CollectionArgs),
    /// Manage your wish list for a game.
    Wishlist(wishlist::WishlistArgs),
    /// Build and organise decks for a game.
    Decks(decks::DecksArgs),
    /// Read another user's public collection, profile and decks.
    Public(public::PublicArgs),

    // -- server / meta --
    /// Liveness probe (`GET /api/health`).
    Health,
    /// Readiness probe (`GET /api/ready`).
    Ready,
    /// Show the server's public runtime config (`GET /api/config`).
    #[command(name = "server-config")]
    ServerConfig,
    /// Show daily display-currency rates (`GET /api/currencies`).
    Currencies,
    /// Fetch the OpenAPI document (`GET /api/openapi.json`).
    Openapi(misc::OpenapiArgs),

    /// Launch the interactive TUI (default when no subcommand is given).
    Tui,
}
