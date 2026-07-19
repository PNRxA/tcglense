//! Public, handle-keyed reads: another user's profile, shared collection, and
//! public decks. Unauthenticated (no credential is required or sent).

use anyhow::Result;
use clap::{Args, Subcommand};

use super::Ctx;
use super::holdings::{self, Surface};
use crate::models::*;
use crate::output::{decks_table, table};

#[derive(Debug, Args)]
pub struct PublicArgs {
    /// The owner's public handle, e.g. `alice-0001`.
    pub handle: String,
    #[command(subcommand)]
    pub command: PublicCommand,
}

#[derive(Debug, Subcommand)]
pub enum PublicCommand {
    /// Show the owner's public profile (handle + per-game summaries).
    Profile,
    /// Read the owner's public collection for a game.
    Collection {
        game: String,
        #[command(subcommand)]
        command: PublicCollectionCommand,
    },
    /// List the owner's public decks (across games).
    Decks,
    /// Show one public deck.
    Deck { deck_id: i64 },
}

#[derive(Debug, Subcommand)]
pub enum PublicCollectionCommand {
    /// List owned cards.
    List {
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        related: bool,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Collection summary.
    Summary {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        related: bool,
    },
    /// Per-set owned aggregates.
    Sets,
    /// Owned cards grouped by drop.
    Drops {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Owned cards grouped by sub-type.
    Subtypes {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Which of the given card ids the owner holds.
    Owned { ids: Vec<String> },
}

pub async fn run(ctx: &Ctx, args: PublicArgs) -> Result<()> {
    let handle = &args.handle;
    match args.command {
        PublicCommand::Profile => {
            let p: PublicProfile = ctx
                .client
                .get_json(&format!("/api/u/{handle}"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&p)?;
            } else {
                println!("{} (member since {})", p.handle, p.member_since);
                let mut t = table(&["Game", "Cards", "Copies", "Value"]);
                for g in &p.games {
                    t.add_row(vec![
                        g.game.clone(),
                        g.summary.unique_cards.to_string(),
                        g.summary.total_cards.to_string(),
                        crate::output::price(&g.summary.total_value_usd),
                    ]);
                }
                println!("{t}");
            }
        }
        PublicCommand::Collection { game, command } => {
            let s = Surface {
                base: format!("/api/u/{handle}/{game}"),
                batch_route: "owned",
                product_batch_route: "owned",
                noun: "Owned",
            };
            match command {
                PublicCollectionCommand::List {
                    query,
                    set,
                    related,
                    page,
                    page_size,
                } => {
                    holdings::list(ctx, &s, query, set, related, None, None, page, page_size)
                        .await?
                }
                PublicCollectionCommand::Summary { set, related } => {
                    holdings::summary(ctx, &s, set, related).await?
                }
                PublicCollectionCommand::Sets => holdings::sets(ctx, &s).await?,
                PublicCollectionCommand::Drops {
                    code,
                    query,
                    page,
                    page_size,
                } => holdings::set_drops(ctx, &s, &code, query, page, page_size).await?,
                PublicCollectionCommand::Subtypes {
                    code,
                    query,
                    page,
                    page_size,
                } => holdings::set_subtypes(ctx, &s, &code, query, page, page_size).await?,
                PublicCollectionCommand::Owned { ids } => {
                    holdings::batch_counts(ctx, &s, ids).await?
                }
            }
        }
        PublicCommand::Decks => {
            let body: DataBody<Vec<Deck>> = ctx
                .client
                .get_json(&format!("/api/u/{handle}/decks"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else if body.data.is_empty() {
                println!("No public decks.");
            } else {
                decks_table(&body.data);
            }
        }
        PublicCommand::Deck { deck_id } => {
            let d: DeckDetail = ctx
                .client
                .get_json(&format!("/api/u/{handle}/decks/{deck_id}"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&d)?;
            } else {
                println!("{} by {}", d.name, d.handle.as_deref().unwrap_or("?"));
                println!(
                    "  format: {}  ·  cards: {}",
                    d.format.as_deref().unwrap_or("—"),
                    d.summary.total_cards
                );
                let mut t = table(&["Qty", "Foil", "Name", "Set"]);
                for c in &d.cards {
                    t.add_row(vec![
                        c.quantity.to_string(),
                        c.foil_quantity.to_string(),
                        crate::output::truncate(&c.card.name, 34),
                        c.card.set_code.to_uppercase(),
                    ]);
                }
                println!("{t}");
            }
        }
    }
    Ok(())
}
