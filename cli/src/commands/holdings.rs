//! Shared engine for the collection + wish-list surfaces. They are independent
//! tables that share the same wire shapes and route shape (only the base path and
//! the batch-count route name differ), so the card- and product-holding operations
//! live here once, parameterised by a [`Surface`].

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Subcommand;

use super::{Ctx, push_flag, push_opt};
use crate::models::*;
use crate::output::{self, collection_summary, collection_table, product_holdings_table, table};

/// Identifies one holdings surface (collection or wish list) for a game.
pub struct Surface {
    /// e.g. `/api/collection/mtg`.
    pub base: String,
    /// Batch card-count route leaf: `owned` (collection) or `counts` (wish list).
    pub batch_route: &'static str,
    /// Batch product-count route leaf.
    pub product_batch_route: &'static str,
    /// Column label for the primary count: `Owned` or `Wanted`.
    pub noun: &'static str,
}

/// Product-holding subcommands, identical between the two surfaces.
#[derive(Debug, Subcommand)]
pub enum ProductHoldingCommand {
    /// List held sealed products.
    List {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Show the counts for one product.
    Get { product_id: String },
    /// Set absolute counts for one product (both zero removes it).
    Set {
        product_id: String,
        #[arg(long, default_value_t = 0)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Aggregate summary of held products.
    Summary,
    /// Per-set aggregate tiles.
    Sets,
    /// Batch counts for the given product ids.
    Counts { ids: Vec<String> },
}

// -- card holdings ----------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn list(
    ctx: &Ctx,
    s: &Surface,
    query: Option<String>,
    set: Option<String>,
    related: bool,
    sort: Option<String>,
    dir: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &query);
    push_opt(&mut q, "set", &set);
    push_opt(&mut q, "sort", &sort);
    push_opt(&mut q, "dir", &dir);
    push_opt(&mut q, "page", &page);
    push_opt(&mut q, "page_size", &page_size);
    push_flag(&mut q, "include_related", related);
    let page: Page<CollectionEntry> = ctx.client.get_json(&s.base, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&page)?;
    } else {
        collection_table(&page.data, s.noun);
        ctx.printer.note(format!(
            "page {} · {} cards total{}",
            page.page,
            page.total,
            if page.has_more {
                " · more (--page)"
            } else {
                ""
            }
        ));
    }
    Ok(())
}

pub async fn get(ctx: &Ctx, s: &Surface, card_id: &str) -> Result<()> {
    let path = format!("{}/cards/{}", s.base, card_id);
    let q: CollectionQuantities = ctx.client.get_json(&path, &[]).await?;
    print_quantities(ctx, &q);
    Ok(())
}

pub async fn set(ctx: &Ctx, s: &Surface, card_id: &str, qty: i64, foil: i64) -> Result<()> {
    let path = format!("{}/cards/{}", s.base, card_id);
    let body = serde_json::json!({ "quantity": qty, "foil_quantity": foil });
    let q: CollectionQuantities = ctx.client.put_json(&path, body).await?;
    if !ctx.printer.json {
        if q.quantity == 0 && q.foil_quantity == 0 {
            ctx.printer.note("Removed.");
        } else {
            ctx.printer.note(format!(
                "Set to {} regular / {} foil.",
                q.quantity, q.foil_quantity
            ));
        }
    } else {
        ctx.printer.json(&q)?;
    }
    Ok(())
}

pub async fn add(ctx: &Ctx, s: &Surface, card_id: &str, qty: i64, foil: i64) -> Result<()> {
    let path = format!("{}/cards/{}", s.base, card_id);
    let current: CollectionQuantities = ctx.client.get_json(&path, &[]).await?;
    set(
        ctx,
        s,
        card_id,
        current.quantity + qty,
        current.foil_quantity + foil,
    )
    .await
}

pub async fn summary(ctx: &Ctx, s: &Surface, set: Option<String>, related: bool) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "set", &set);
    push_flag(&mut q, "include_related", related);
    let path = format!("{}/summary", s.base);
    let summary: CollectionSummary = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&summary)?;
    } else {
        collection_summary(&summary);
    }
    Ok(())
}

pub async fn sets(ctx: &Ctx, s: &Surface) -> Result<()> {
    let path = format!("{}/sets", s.base);
    let body: DataBody<Vec<CollectionSet>> = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else {
        let mut t = table(&["Code", "Name", "Cards", "Copies", "Value"]);
        for cs in &body.data {
            t.add_row(vec![
                cs.code.to_uppercase(),
                output::truncate(&cs.name, 36),
                cs.owned_cards.to_string(),
                cs.owned_copies.to_string(),
                output::price(&cs.owned_value_usd),
            ]);
        }
        println!("{t}");
    }
    Ok(())
}

pub async fn set_drops(
    ctx: &Ctx,
    s: &Surface,
    code: &str,
    query: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &query);
    push_opt(&mut q, "page", &page);
    push_opt(&mut q, "page_size", &page_size);
    let path = format!("{}/sets/{}/drops", s.base, code);
    let page: Page<CollectionDropGroup> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&page)?;
    } else {
        for g in &page.data {
            println!("\n== {} ({} cards) ==", g.title, g.card_count);
            collection_table(&g.cards, s.noun);
        }
    }
    Ok(())
}

pub async fn set_subtypes(
    ctx: &Ctx,
    s: &Surface,
    code: &str,
    query: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &query);
    push_opt(&mut q, "page", &page);
    push_opt(&mut q, "page_size", &page_size);
    let path = format!("{}/sets/{}/subtypes", s.base, code);
    let page: Page<CollectionSubtypeGroup> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&page)?;
    } else {
        for g in &page.data {
            println!("\n== {} ({} cards) ==", g.title, g.card_count);
            collection_table(&g.cards, s.noun);
        }
    }
    Ok(())
}

pub async fn batch_counts(ctx: &Ctx, s: &Surface, ids: Vec<String>) -> Result<()> {
    let path = format!("{}/{}", s.base, s.batch_route);
    let body = serde_json::json!({ "ids": ids });
    let resp: DataBody<BTreeMap<String, CollectionQuantities>> =
        ctx.client.post_json(&path, body).await?;
    print_counts_map(ctx, &resp.data);
    Ok(())
}

// -- product holdings -------------------------------------------------------

pub async fn products(ctx: &Ctx, s: &Surface, cmd: ProductHoldingCommand) -> Result<()> {
    let base = format!("{}/products", s.base);
    match cmd {
        ProductHoldingCommand::List {
            set,
            page,
            page_size,
        } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "set", &set);
            push_opt(&mut q, "page", &page);
            push_opt(&mut q, "page_size", &page_size);
            let page: Page<ProductHoldingEntry> = ctx.client.get_json(&base, &q).await?;
            if ctx.printer.json {
                ctx.printer.json(&page)?;
            } else {
                product_holdings_table(&page.data);
                ctx.printer.note(format!(
                    "page {} · {} products total{}",
                    page.page,
                    page.total,
                    if page.has_more {
                        " · more (--page)"
                    } else {
                        ""
                    }
                ));
            }
        }
        ProductHoldingCommand::Get { product_id } => {
            let q: CollectionQuantities = ctx
                .client
                .get_json(&format!("{base}/{product_id}"), &[])
                .await?;
            print_quantities(ctx, &q);
        }
        ProductHoldingCommand::Set {
            product_id,
            qty,
            foil,
        } => {
            let body = serde_json::json!({ "quantity": qty, "foil_quantity": foil });
            let q: CollectionQuantities = ctx
                .client
                .put_json(&format!("{base}/{product_id}"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&q)?;
            } else if q.quantity == 0 && q.foil_quantity == 0 {
                ctx.printer.note("Removed.");
            } else {
                ctx.printer
                    .note(format!("Set to {} / {} foil.", q.quantity, q.foil_quantity));
            }
        }
        ProductHoldingCommand::Summary => {
            let s: ProductHoldingSummary =
                ctx.client.get_json(&format!("{base}/summary"), &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&s)?;
            } else {
                println!("Unique products : {}", s.unique_products);
                println!("Total products  : {}", s.total_products);
                println!("Total value     : {}", output::price(&s.total_value_usd));
            }
        }
        ProductHoldingCommand::Sets => {
            let body: DataBody<Vec<ProductHoldingSet>> =
                ctx.client.get_json(&format!("{base}/sets"), &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["Set", "Name", "Unique", "Total", "Value"]);
                for ps in &body.data {
                    t.add_row(vec![
                        ps.code.to_uppercase(),
                        output::dash(&ps.name),
                        ps.unique_products.to_string(),
                        ps.total_products.to_string(),
                        output::price(&ps.total_value_usd),
                    ]);
                }
                println!("{t}");
            }
        }
        ProductHoldingCommand::Counts { ids } => {
            let body = serde_json::json!({ "ids": ids });
            let resp: DataBody<BTreeMap<String, CollectionQuantities>> = ctx
                .client
                .post_json(&format!("{base}/{}", s.product_batch_route), body)
                .await?;
            print_counts_map(ctx, &resp.data);
        }
    }
    Ok(())
}

// -- shared printing --------------------------------------------------------

fn print_quantities(ctx: &Ctx, q: &CollectionQuantities) {
    if ctx.printer.json {
        let _ = ctx.printer.json(q);
    } else {
        println!("regular: {}   foil: {}", q.quantity, q.foil_quantity);
    }
}

fn print_counts_map(ctx: &Ctx, map: &BTreeMap<String, CollectionQuantities>) {
    if ctx.printer.json {
        let _ = ctx.printer.json(map);
    } else if map.is_empty() {
        println!("(none held)");
    } else {
        let mut t = table(&["ID", "Regular", "Foil"]);
        for (id, q) in map {
            t.add_row(vec![
                id.clone(),
                q.quantity.to_string(),
                q.foil_quantity.to_string(),
            ]);
        }
        println!("{t}");
    }
}
