//! Authentication + account commands: login/logout/register, API-key management,
//! currency, and username.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde::Deserialize;

use super::Ctx;
use crate::config::{Auth, Config};
use crate::models::{ApiKeyList, CreatedApiKey, User, UsernameAvailability};
use crate::output::apikeys_table;

// -- arg types --------------------------------------------------------------

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Account email (prompted if omitted).
    #[arg(long)]
    pub email: Option<String>,
    /// Account password (prompted securely if omitted).
    #[arg(long)]
    pub password: Option<String>,
    /// Cloudflare Turnstile token (only if the server has CAPTCHA enabled).
    #[arg(long)]
    pub captcha: Option<String>,
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    /// Email to register.
    pub email: String,
    /// Same-origin path to carry through the completion link.
    #[arg(long)]
    pub redirect: Option<String>,
    #[arg(long)]
    pub captcha: Option<String>,
}

#[derive(Debug, Args)]
pub struct CompleteArgs {
    /// The registration-completion token (from the email link or the dev bypass).
    pub token: String,
    /// The password to set (prompted securely if omitted).
    #[arg(long)]
    pub password: Option<String>,
    /// Optionally claim a public username at signup.
    #[arg(long)]
    pub username: Option<String>,
    #[arg(long)]
    pub captcha: Option<String>,
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Store a `tcgl_` API key as the active credential (verified against `/me`).
    Key {
        /// The `tcgl_<hex>` key.
        key: String,
        /// Note the key's scope for display (read | read_write).
        #[arg(long)]
        scope: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct ApiKeysArgs {
    #[command(subcommand)]
    pub command: ApiKeysCommand,
}

#[derive(Debug, Subcommand)]
pub enum ApiKeysCommand {
    /// List your active API keys.
    List,
    /// Mint a new API key (the plaintext is shown once).
    Create {
        /// A label for the key.
        name: String,
        /// Key scope.
        #[arg(long, value_enum, default_value_t = KeyScope::ReadWrite)]
        scope: KeyScope,
        /// Expire the key after N days (default: never).
        #[arg(long)]
        expires_in_days: Option<i64>,
        /// Also store the new key as the active credential.
        #[arg(long)]
        use_key: bool,
    },
    /// Revoke an API key by id.
    Revoke { id: i64 },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum KeyScope {
    Read,
    #[value(name = "read_write")]
    ReadWrite,
}

impl KeyScope {
    fn as_str(self) -> &'static str {
        match self {
            KeyScope::Read => "read",
            KeyScope::ReadWrite => "read_write",
        }
    }
}

#[derive(Debug, Args)]
pub struct CurrencyArgs {
    /// A supported ISO 4217 code (USD/AUD/CAD/EUR/GBP/JPY/NZD) to set; omit to show.
    pub currency: Option<String>,
}

#[derive(Debug, Args)]
pub struct UsernameArgs {
    #[command(subcommand)]
    pub command: UsernameCommand,
}

#[derive(Debug, Subcommand)]
pub enum UsernameCommand {
    /// Claim / change your public username.
    Set { username: String },
    /// Check whether a username is available.
    Check { username: String },
}

#[derive(Deserialize)]
struct MeResponse {
    user: User,
}

// -- handlers ---------------------------------------------------------------

pub async fn login(ctx: &Ctx, args: LoginArgs) -> Result<()> {
    let email = match args.email {
        Some(e) => e,
        None => prompt_line("Email: ")?,
    };
    let password = match args.password {
        Some(p) => p,
        None => rpassword::prompt_password("Password: ").context("reading password")?,
    };
    let user = ctx
        .client
        .login(&email, &password, args.captcha.as_deref())
        .await?;
    if ctx.printer.json {
        ctx.printer.json(&user)?;
    } else {
        println!("Logged in as {} (id {}).", user.email, user.id);
    }
    Ok(())
}

pub async fn logout(ctx: &Ctx) -> Result<()> {
    ctx.client.logout().await?;
    ctx.printer.note("Logged out.");
    Ok(())
}

pub async fn register(ctx: &Ctx, args: RegisterArgs) -> Result<()> {
    let mut body = serde_json::json!({ "email": args.email });
    if let Some(r) = &args.redirect {
        body["redirect"] = serde_json::Value::String(r.clone());
    }
    if let Some(c) = &args.captcha {
        body["captcha_token"] = serde_json::Value::String(c.clone());
    }
    let resp: crate::models::RegisterResponse =
        ctx.client.post_json("/api/auth/register", body).await?;
    if ctx.printer.json {
        ctx.printer.json(&resp)?;
        return Ok(());
    }
    match resp.completion_token {
        Some(token) => {
            println!("Registration started (no email provider — dev bypass).");
            println!("Finish with:\n  tcglense complete-registration {token}");
        }
        None => {
            println!(
                "If {} can register, a completion link has been emailed. Follow it, or run\n  tcglense complete-registration <token>",
                args.email
            );
        }
    }
    Ok(())
}

pub async fn complete_registration(ctx: &Ctx, args: CompleteArgs) -> Result<()> {
    let password = match args.password {
        Some(p) => p,
        None => rpassword::prompt_password("New password: ").context("reading password")?,
    };
    let user = ctx
        .client
        .complete_registration(
            &args.token,
            &password,
            args.username.as_deref(),
            args.captcha.as_deref(),
        )
        .await?;
    if ctx.printer.json {
        ctx.printer.json(&user)?;
    } else {
        println!("Registered and signed in as {}.", user.email);
    }
    Ok(())
}

pub async fn whoami(ctx: &Ctx) -> Result<()> {
    let me: MeResponse = ctx.client.get_json("/api/auth/me", &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&me.user)?;
    } else {
        print_user(&me.user);
    }
    Ok(())
}

pub async fn status(ctx: &Ctx) -> Result<()> {
    let auth = ctx.client.current_auth().await;
    if ctx.printer.json {
        let v = serde_json::json!({
            "base_url": ctx.client.base_url(),
            "config_path": ctx.config_path.display().to_string(),
            "authenticated": auth.is_some(),
            "credential": auth.as_ref().map(|a| a.describe()),
        });
        ctx.printer.json(&v)?;
        return Ok(());
    }
    println!("Base URL   : {}", ctx.client.base_url());
    println!("Config     : {}", ctx.config_path.display());
    match auth {
        Some(a) => println!("Credential : {}", a.describe()),
        None => println!("Credential : (none — run `tcglense login`)"),
    }
    Ok(())
}

pub async fn store_key(ctx: &Ctx, args: AuthArgs) -> Result<()> {
    let AuthCommand::Key { key, scope } = args.command;
    if !key.starts_with("tcgl_") {
        bail!("an API key must start with `tcgl_`");
    }
    ctx.client.set_api_key(key, scope).await?;
    // Verify it works and report the account.
    match ctx.client.get_json::<MeResponse>("/api/auth/me", &[]).await {
        Ok(me) => {
            ctx.printer.note(format!(
                "API key stored; authenticated as {}.",
                me.user.email
            ));
        }
        Err(e) => {
            ctx.printer
                .note(format!("API key stored, but verification failed: {e}"));
        }
    }
    Ok(())
}

pub async fn api_keys(ctx: &Ctx, args: ApiKeysArgs) -> Result<()> {
    match args.command {
        ApiKeysCommand::List => {
            let list: ApiKeyList = ctx.client.get_json("/api/auth/api-keys", &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&list)?;
            } else if list.data.is_empty() {
                println!("No API keys.");
            } else {
                apikeys_table(&list.data);
            }
        }
        ApiKeysCommand::Create {
            name,
            scope,
            expires_in_days,
            use_key,
        } => {
            let body = serde_json::json!({
                "name": name,
                "scope": scope.as_str(),
                "expires_in_days": expires_in_days,
            });
            let created: CreatedApiKey = ctx.client.post_json("/api/auth/api-keys", body).await?;
            if use_key {
                ctx.client
                    .set_api_key(created.key.clone(), Some(created.scope.clone()))
                    .await?;
            }
            if ctx.printer.json {
                ctx.printer.json(&created)?;
            } else {
                println!(
                    "Created API key '{}' (id {}, {}).",
                    created.name, created.id, created.scope
                );
                println!("\n  {}\n", created.key);
                println!("This is the ONLY time the key is shown — copy it now.");
                if use_key {
                    println!("Stored as the active credential.");
                }
            }
        }
        ApiKeysCommand::Revoke { id } => {
            ctx.client
                .delete(&format!("/api/auth/api-keys/{id}"))
                .await?;
            ctx.printer.note(format!("Revoked API key {id}."));
        }
    }
    Ok(())
}

pub async fn currency(ctx: &Ctx, args: CurrencyArgs) -> Result<()> {
    match args.currency {
        Some(cur) => {
            let body = serde_json::json!({ "currency": cur.to_uppercase() });
            let user: User = ctx.client.put_json("/api/auth/currency", body).await?;
            if ctx.printer.json {
                ctx.printer.json(&user)?;
            } else {
                println!("Display currency set to {}.", user.currency);
            }
        }
        None => {
            let me: MeResponse = ctx.client.get_json("/api/auth/me", &[]).await?;
            if ctx.printer.json {
                ctx.printer
                    .json(&serde_json::json!({ "currency": me.user.currency }))?;
            } else {
                println!("Display currency: {}", me.user.currency);
            }
        }
    }
    Ok(())
}

pub async fn username(ctx: &Ctx, args: UsernameArgs) -> Result<()> {
    match args.command {
        UsernameCommand::Set { username } => {
            let body = serde_json::json!({ "username": username });
            let user: User = ctx.client.put_json("/api/auth/username", body).await?;
            if ctx.printer.json {
                ctx.printer.json(&user)?;
            } else {
                println!(
                    "Username set. Handle: {}",
                    user.handle.as_deref().unwrap_or("(pending)")
                );
            }
        }
        UsernameCommand::Check { username } => {
            let avail: UsernameAvailability = ctx
                .client
                .get_json("/api/auth/username/available", &[("username", username)])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&avail)?;
            } else if avail.valid {
                println!("Available.");
            } else {
                println!(
                    "Unavailable: {}",
                    avail.reason.as_deref().unwrap_or("invalid")
                );
            }
        }
    }
    Ok(())
}

// -- helpers ----------------------------------------------------------------

fn print_user(u: &User) {
    println!("Email      : {}", u.email);
    println!("Id         : {}", u.id);
    println!("Handle     : {}", u.handle.as_deref().unwrap_or("(none)"));
    println!("Currency   : {}", u.currency);
    println!("Member since: {}", u.created_at);
}

fn prompt_line(label: &str) -> Result<String> {
    use std::io::Write;
    print!("{label}");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

/// Load the persisted config (used by config-mutating commands elsewhere).
#[allow(dead_code)]
pub fn reload_config(ctx: &Ctx) -> Result<Config> {
    Config::load(&ctx.config_path)
}

/// Convenience: does the ctx hold a session credential (vs. an API key / none)?
#[allow(dead_code)]
pub async fn is_session(ctx: &Ctx) -> bool {
    matches!(ctx.client.current_auth().await, Some(Auth::Session { .. }))
}
