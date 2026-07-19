//! Wish-list commands: the collection's "want" twin. Rides the same shared holdings
//! engine, minus import/sync/export/movers/value-history/visibility (a wish list has
//! nothing to import and no value chart).

use anyhow::Result;
use clap::{Args, Subcommand};

use super::Ctx;
use super::holdings::{self, ProductHoldingCommand, Surface};

#[derive(Debug, Args)]
pub struct WishlistArgs {
    pub game: String,
    #[command(subcommand)]
    pub command: WishlistCommand,
}

#[derive(Debug, Subcommand)]
pub enum WishlistCommand {
    /// List wanted cards.
    List {
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        related: bool,
        #[arg(long)]
        sort: Option<String>,
        #[arg(long)]
        dir: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Show wanted counts for one card.
    Get { card_id: String },
    /// Set the absolute wanted counts for a card (both zero removes it).
    Set {
        card_id: String,
        #[arg(long, default_value_t = 0)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Increment the wanted counts for a card.
    Add {
        card_id: String,
        #[arg(long, default_value_t = 1)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Remove a card from the wish list.
    Remove { card_id: String },
    /// Wish-list value / copy summary.
    Summary {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        related: bool,
    },
    /// Per-set wanted aggregates.
    Sets,
    /// Wanted cards in a drop-grouped set, grouped by drop.
    Drops {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Wanted cards in a set, grouped by sub-type.
    Subtypes {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Batch wanted counts for the given card ids.
    Counts { ids: Vec<String> },
    /// Manage wanted sealed products.
    Products {
        #[command(subcommand)]
        command: ProductHoldingCommand,
    },
}

pub async fn run(ctx: &Ctx, args: WishlistArgs) -> Result<()> {
    let s = Surface {
        base: format!("/api/wishlist/{}", args.game),
        batch_route: "counts",
        product_batch_route: "counts",
        noun: "Wanted",
    };
    match args.command {
        WishlistCommand::List {
            query,
            set,
            related,
            sort,
            dir,
            page,
            page_size,
        } => holdings::list(ctx, &s, query, set, related, sort, dir, page, page_size).await,
        WishlistCommand::Get { card_id } => holdings::get(ctx, &s, &card_id).await,
        WishlistCommand::Set { card_id, qty, foil } => {
            holdings::set(ctx, &s, &card_id, qty, foil).await
        }
        WishlistCommand::Add { card_id, qty, foil } => {
            holdings::add(ctx, &s, &card_id, qty, foil).await
        }
        WishlistCommand::Remove { card_id } => holdings::set(ctx, &s, &card_id, 0, 0).await,
        WishlistCommand::Summary { set, related } => holdings::summary(ctx, &s, set, related).await,
        WishlistCommand::Sets => holdings::sets(ctx, &s).await,
        WishlistCommand::Drops {
            code,
            query,
            page,
            page_size,
        } => holdings::set_drops(ctx, &s, &code, query, page, page_size).await,
        WishlistCommand::Subtypes {
            code,
            query,
            page,
            page_size,
        } => holdings::set_subtypes(ctx, &s, &code, query, page, page_size).await,
        WishlistCommand::Counts { ids } => holdings::batch_counts(ctx, &s, ids).await,
        WishlistCommand::Products { command } => holdings::products(ctx, &s, command).await,
    }
}
