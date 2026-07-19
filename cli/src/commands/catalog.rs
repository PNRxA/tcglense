//! Public catalog commands: games, sets, cards, prices, prints, sealed products,
//! scan, and image download.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use super::{Ctx, push_flag, push_opt};
use crate::models::*;
use crate::output::{
    self, card_detail, cards_table, games_table, prices_table, products_table, sets_table, table,
};

// -- arg types --------------------------------------------------------------

#[derive(Debug, Args)]
pub struct SetsArgs {
    pub game: String,
}

#[derive(Debug, Args)]
pub struct SetArgs {
    pub game: String,
    pub code: String,
    /// List the set's cards instead of its metadata.
    #[arg(long)]
    pub cards: bool,
    /// List the set's Secret Lair drops (requires a drop-grouped set).
    #[arg(long)]
    pub drops: bool,
    /// List the set's cards grouped by sub-type (treatment).
    #[arg(long)]
    pub subtypes: bool,
    /// Scryfall-style filter query.
    #[arg(short = 'q', long)]
    pub query: Option<String>,
    /// Span the set's whole group (root + related tokens/promos/decks).
    #[arg(long)]
    pub related: bool,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long)]
    pub page_size: Option<u32>,
}

#[derive(Debug, Args)]
pub struct CardsArgs {
    pub game: String,
    /// Scryfall-style search query.
    #[arg(short = 'q', long)]
    pub query: Option<String>,
    /// Exact-name filter ("printings of this name").
    #[arg(long)]
    pub name: Option<String>,
    /// Scope to a single set code.
    #[arg(long)]
    pub set: Option<String>,
    /// With --set, span the set's whole group.
    #[arg(long)]
    pub related: bool,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long)]
    pub page_size: Option<u32>,
}

#[derive(Debug, Args)]
pub struct CardArgs {
    pub game: String,
    pub id: String,
}

#[derive(Debug, Args)]
pub struct CardNamesArgs {
    pub game: String,
    pub query: String,
    #[arg(long)]
    pub limit: Option<u32>,
}

#[derive(Debug, Args)]
pub struct PricesArgs {
    pub game: String,
    pub id: String,
    /// Window: 7d | 30d | 1y | 2y | 3y | all (default: full daily series).
    #[arg(long)]
    pub range: Option<String>,
}

#[derive(Debug, Args)]
pub struct PrintsArgs {
    pub game: String,
    pub id: String,
}

#[derive(Debug, Args)]
pub struct SealedArgs {
    pub game: String,
    pub id: String,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    pub game: String,
    /// A file containing the 32-byte fingerprint (raw bytes or hex text).
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// The 32-byte fingerprint as a 64-char hex string.
    #[arg(long)]
    pub hex: Option<String>,
    /// How many candidate matches to return.
    #[arg(long)]
    pub top_k: Option<i64>,
}

#[derive(Debug, Args)]
pub struct ProductsArgs {
    pub game: String,
    #[arg(short = 'q', long)]
    pub query: Option<String>,
    #[arg(long)]
    pub set: Option<String>,
    #[arg(long = "type")]
    pub type_: Option<String>,
    /// Sort key: name | price | released.
    #[arg(long)]
    pub sort: Option<String>,
    /// Direction: asc | desc.
    #[arg(long)]
    pub dir: Option<String>,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long)]
    pub page_size: Option<u32>,
    /// Show the game's product facets (types + sets) instead of a product list.
    #[arg(long)]
    pub facets: bool,
}

#[derive(Debug, Args)]
pub struct ProductArgs {
    pub game: String,
    pub id: String,
    #[command(subcommand)]
    pub command: Option<ProductCommand>,
}

#[derive(Debug, Subcommand)]
pub enum ProductCommand {
    /// Price history.
    Prices {
        #[arg(long)]
        range: Option<String>,
    },
    /// Structural composition ("what's in the box").
    Contents,
    /// Parent products that contain this one.
    Containers,
    /// Cards the product contains / can yield.
    Cards {
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// The non-empty card display sections + counts.
    Sections,
}

#[derive(Debug, Args)]
pub struct IngestArgs {
    pub game: String,
}

#[derive(Debug, Args)]
pub struct ImageArgs {
    #[command(subcommand)]
    pub command: ImageCommand,
}

#[derive(Debug, Subcommand)]
pub enum ImageCommand {
    /// Download a card image.
    Card {
        game: String,
        id: String,
        /// small | normal | large | png | art_crop.
        #[arg(long)]
        size: Option<String>,
        /// Face index for double-faced cards.
        #[arg(long)]
        face: Option<u32>,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Download a sealed-product image.
    Product {
        game: String,
        id: String,
        /// normal | small.
        #[arg(long)]
        size: Option<String>,
        #[arg(short, long)]
        output: PathBuf,
    },
}

// -- handlers ---------------------------------------------------------------

pub async fn games(ctx: &Ctx) -> Result<()> {
    let body: DataBody<Vec<Game>> = ctx.client.get_json("/api/games", &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else {
        games_table(&body.data);
    }
    Ok(())
}

pub async fn sets(ctx: &Ctx, args: SetsArgs) -> Result<()> {
    let path = format!("/api/games/{}/sets", args.game);
    let body: DataBody<Vec<CardSet>> = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else {
        sets_table(&body.data);
        ctx.printer.note(format!("{} sets.", body.data.len()));
    }
    Ok(())
}

pub async fn set(ctx: &Ctx, args: SetArgs) -> Result<()> {
    let base = format!("/api/games/{}/sets/{}", args.game, args.code);
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &args.query);
    push_opt(&mut q, "page", &args.page);
    push_opt(&mut q, "page_size", &args.page_size);

    if args.drops {
        let page: Page<DropGroup> = ctx.client.get_json(&format!("{base}/drops"), &q).await?;
        if ctx.printer.json {
            ctx.printer.json(&page)?;
        } else {
            for g in &page.data {
                println!(
                    "\n== {} ({} cards, {}) ==",
                    g.title,
                    g.card_count,
                    output::price(&g.cheapest_prints_usd)
                );
                cards_table(&g.cards);
            }
            page_footer(ctx, page.page, page.total, page.has_more, "drops");
        }
    } else if args.subtypes {
        let page: Page<SubtypeGroup> = ctx.client.get_json(&format!("{base}/subtypes"), &q).await?;
        if ctx.printer.json {
            ctx.printer.json(&page)?;
        } else {
            for g in &page.data {
                println!("\n== {} ({} cards) ==", g.title, g.card_count);
                cards_table(&g.cards);
            }
            page_footer(ctx, page.page, page.total, page.has_more, "sub-types");
        }
    } else if args.cards {
        push_flag(&mut q, "include_related", args.related);
        let page: Page<Card> = ctx.client.get_json(&format!("{base}/cards"), &q).await?;
        print_card_page(ctx, page);
    } else {
        let set: CardSet = ctx.client.get_json(&base, &[]).await?;
        if ctx.printer.json {
            ctx.printer.json(&set)?;
        } else {
            sets_table(std::slice::from_ref(&set));
            println!(
                "drops: {}   sub-types: {}   parent: {}",
                set.has_drops,
                set.has_subtypes,
                set.parent_set_code.as_deref().unwrap_or("—")
            );
        }
    }
    Ok(())
}

pub async fn cards(ctx: &Ctx, args: CardsArgs) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &args.query);
    push_opt(&mut q, "page", &args.page);
    push_opt(&mut q, "page_size", &args.page_size);

    let path = if let Some(set) = &args.set {
        push_flag(&mut q, "include_related", args.related);
        format!("/api/games/{}/sets/{}/cards", args.game, set)
    } else {
        push_opt(&mut q, "name", &args.name);
        format!("/api/games/{}/cards", args.game)
    };
    let page: Page<Card> = ctx.client.get_json(&path, &q).await?;
    print_card_page(ctx, page);
    Ok(())
}

pub async fn card(ctx: &Ctx, args: CardArgs) -> Result<()> {
    let path = format!("/api/games/{}/cards/{}", args.game, args.id);
    let card: Card = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&card)?;
    } else {
        card_detail(&card);
    }
    Ok(())
}

pub async fn card_names(ctx: &Ctx, args: CardNamesArgs) -> Result<()> {
    let mut q: Vec<(&str, String)> = vec![("q", args.query)];
    push_opt(&mut q, "limit", &args.limit);
    let path = format!("/api/games/{}/card-names", args.game);
    let body: DataBody<Vec<String>> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else {
        for n in &body.data {
            println!("{n}");
        }
    }
    Ok(())
}

pub async fn prices(ctx: &Ctx, args: PricesArgs) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "range", &args.range);
    let path = format!("/api/games/{}/cards/{}/prices", args.game, args.id);
    let body: DataBody<Vec<PricePoint>> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else if body.data.is_empty() {
        println!("No price history in range.");
    } else {
        prices_table(&body.data);
    }
    Ok(())
}

pub async fn prints(ctx: &Ctx, args: PrintsArgs) -> Result<()> {
    let path = format!("/api/games/{}/cards/{}/prints", args.game, args.id);
    let body: DataBody<Vec<Card>> = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else if body.data.is_empty() {
        println!("No other printings.");
    } else {
        cards_table(&body.data);
    }
    Ok(())
}

pub async fn sealed(ctx: &Ctx, args: SealedArgs) -> Result<()> {
    let path = format!("/api/games/{}/cards/{}/sealed", args.game, args.id);
    let body: DataBody<Vec<SealedProductRef>> = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else if body.data.is_empty() {
        println!("Not found in any sealed product.");
    } else {
        let mut t = table(&["Membership", "Foil", "Product", "Set", "USD"]);
        for r in &body.data {
            t.add_row(vec![
                r.membership.clone(),
                if r.foil { "yes" } else { "" }.to_string(),
                output::truncate(&r.product.name, 40),
                r.product.set_code.to_uppercase(),
                output::price(&r.product.prices.usd),
            ]);
        }
        println!("{t}");
    }
    Ok(())
}

pub async fn scan(ctx: &Ctx, args: ScanArgs) -> Result<()> {
    let bytes = read_fingerprint(&args)?;
    let numbers: Vec<u16> = bytes.iter().map(|b| *b as u16).collect();
    let body = serde_json::json!({
        "fingerprints": [numbers],
        "top_k": args.top_k,
    });
    let path = format!("/api/games/{}/scan", args.game);
    let resp: ScanResponseLocal = ctx.client.post_json(&path, body).await?;
    if ctx.printer.json {
        ctx.printer.json(&resp.data)?;
    } else if resp.data.is_empty() {
        println!("No match within the confidence radius.");
    } else {
        let mut t = table(&["Distance", "ID", "Name", "Set", "#"]);
        for m in &resp.data {
            t.add_row(vec![
                m.distance.to_string(),
                output::truncate(&m.card.id, 12),
                output::truncate(&m.card.name, 34),
                m.card.set_code.to_uppercase(),
                m.card.collector_number.clone(),
            ]);
        }
        println!("{t}");
    }
    Ok(())
}

pub async fn products(ctx: &Ctx, args: ProductsArgs) -> Result<()> {
    if args.facets {
        let path = format!("/api/games/{}/products/facets", args.game);
        let body: DataBody<ProductFacets> = ctx.client.get_json(&path, &[]).await?;
        if ctx.printer.json {
            ctx.printer.json(&body.data)?;
        } else {
            println!("Types: {}", body.data.types.join(", "));
            let mut t = table(&["Set", "Name", "Products"]);
            for s in &body.data.sets {
                t.add_row(vec![
                    s.code.to_uppercase(),
                    output::dash(&s.name),
                    s.product_count.to_string(),
                ]);
            }
            println!("{t}");
        }
        return Ok(());
    }

    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "q", &args.query);
    push_opt(&mut q, "set", &args.set);
    push_opt(&mut q, "type", &args.type_);
    push_opt(&mut q, "sort", &args.sort);
    push_opt(&mut q, "dir", &args.dir);
    push_opt(&mut q, "page", &args.page);
    push_opt(&mut q, "page_size", &args.page_size);
    let path = format!("/api/games/{}/products", args.game);
    let page: Page<Product> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&page)?;
    } else {
        products_table(&page.data);
        page_footer(ctx, page.page, page.total, page.has_more, "products");
    }
    Ok(())
}

pub async fn product(ctx: &Ctx, args: ProductArgs) -> Result<()> {
    let base = format!("/api/games/{}/products/{}", args.game, args.id);
    match args.command {
        None => {
            let p: Product = ctx.client.get_json(&base, &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&p)?;
            } else {
                product_detail(&p);
            }
        }
        Some(ProductCommand::Prices { range }) => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "range", &range);
            let body: DataBody<Vec<ProductPricePoint>> =
                ctx.client.get_json(&format!("{base}/prices"), &q).await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["Date", "USD", "Foil"]);
                for p in &body.data {
                    t.add_row(vec![
                        p.date.clone(),
                        output::price(&p.usd),
                        output::price(&p.usd_foil),
                    ]);
                }
                println!("{t}");
            }
        }
        Some(ProductCommand::Contents) => {
            let body: DataBody<Vec<ProductComponent>> = ctx
                .client
                .get_json(&format!("{base}/contents"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["Kind", "Qty", "Name", "Links to"]);
                for c in &body.data {
                    let link = c
                        .product
                        .as_ref()
                        .map(|p| format!("product {}", p.id))
                        .or_else(|| c.card.as_ref().map(|cd| format!("card {}", cd.id)))
                        .unwrap_or_default();
                    t.add_row(vec![
                        c.kind.clone(),
                        c.quantity.to_string(),
                        output::truncate(&c.name, 40),
                        link,
                    ]);
                }
                println!("{t}");
            }
        }
        Some(ProductCommand::Containers) => {
            let body: DataBody<Vec<ProductContainer>> = ctx
                .client
                .get_json(&format!("{base}/containers"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["Qty", "Parent product", "ID", "USD"]);
                for c in &body.data {
                    t.add_row(vec![
                        c.quantity.to_string(),
                        output::truncate(&c.product.name, 40),
                        c.product.id.clone(),
                        output::price(&c.product.prices.usd),
                    ]);
                }
                println!("{t}");
            }
        }
        Some(ProductCommand::Cards {
            section,
            page,
            page_size,
        }) => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "section", &section);
            push_opt(&mut q, "page", &page);
            push_opt(&mut q, "page_size", &page_size);
            let page: Page<ProductCardEntry> =
                ctx.client.get_json(&format!("{base}/cards"), &q).await?;
            if ctx.printer.json {
                ctx.printer.json(&page)?;
            } else {
                let mut t = table(&["Membership", "Excl", "Foil", "Name", "Set", "#"]);
                for e in &page.data {
                    t.add_row(vec![
                        e.membership.clone(),
                        if e.exclusive { "yes" } else { "" }.to_string(),
                        if e.foil { "yes" } else { "" }.to_string(),
                        output::truncate(&e.card.name, 32),
                        e.card.set_code.to_uppercase(),
                        e.card.collector_number.clone(),
                    ]);
                }
                println!("{t}");
                page_footer(ctx, page.page, page.total, page.has_more, "cards");
            }
        }
        Some(ProductCommand::Sections) => {
            let body: DataBody<Vec<ProductCardSection>> = ctx
                .client
                .get_json(&format!("{base}/cards/sections"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["Section", "Cards", "Booster family"]);
                for s in &body.data {
                    t.add_row(vec![
                        s.key.clone(),
                        s.total.to_string(),
                        output::dash(&s.booster_family),
                    ]);
                }
                println!("{t}");
            }
        }
    }
    Ok(())
}

pub async fn ingest(ctx: &Ctx, args: IngestArgs) -> Result<()> {
    let path = format!("/api/games/{}/status", args.game);
    let s: IngestStatus = ctx.client.get_json(&path, &[]).await?;
    if ctx.printer.json {
        ctx.printer.json(&s)?;
    } else {
        println!("status         : {}", s.status);
        if let Some(d) = &s.detail {
            println!("detail         : {d}");
        }
        println!("sets_imported  : {}", s.sets_imported);
        println!("cards_imported : {}", s.cards_imported);
        println!(
            "source_updated : {}",
            s.source_updated_at.as_deref().unwrap_or("—")
        );
        println!(
            "finished_at    : {}",
            s.finished_at.as_deref().unwrap_or("—")
        );
    }
    Ok(())
}

pub async fn image(ctx: &Ctx, args: ImageArgs) -> Result<()> {
    let (path, mut q, output): (String, Vec<(&str, String)>, PathBuf) = match args.command {
        ImageCommand::Card {
            game,
            id,
            size,
            face,
            output,
        } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "size", &size);
            push_opt(&mut q, "face", &face);
            (format!("/api/games/{game}/cards/{id}/image"), q, output)
        }
        ImageCommand::Product {
            game,
            id,
            size,
            output,
        } => {
            let mut q: Vec<(&str, String)> = Vec::new();
            push_opt(&mut q, "size", &size);
            (format!("/api/games/{game}/products/{id}/image"), q, output)
        }
    };
    let bytes = ctx.client.get_bytes(&path, &q).await?;
    q.clear();
    std::fs::write(&output, &bytes)?;
    ctx.printer.note(format!(
        "Wrote {} bytes to {}.",
        bytes.len(),
        output.display()
    ));
    Ok(())
}

// -- helpers ----------------------------------------------------------------

fn print_card_page(ctx: &Ctx, page: Page<Card>) {
    if ctx.printer.json {
        let _ = ctx.printer.json(&page);
    } else {
        cards_table(&page.data);
        page_footer(ctx, page.page, page.total, page.has_more, "cards");
    }
}

fn page_footer(ctx: &Ctx, page: i64, total: i64, has_more: bool, noun: &str) {
    ctx.printer.note(format!(
        "page {page} · {total} {noun} total{}",
        if has_more {
            " · more available (--page)"
        } else {
            ""
        }
    ));
}

fn product_detail(p: &Product) {
    println!("{}  [{}]", p.name, p.id);
    println!(
        "  {} · {} · {}",
        p.set_code.to_uppercase(),
        p.set_name.as_deref().unwrap_or("—"),
        p.product_type
    );
    println!(
        "  USD {} · Foil {} · MSRP {}",
        output::price(&p.prices.usd),
        output::price(&p.prices.usd_foil),
        output::price(&p.msrp)
    );
    if let Some(r) = &p.released_at {
        println!("  Released: {r}");
    }
    if let Some(u) = &p.url {
        println!("  {u}");
    }
}

fn read_fingerprint(args: &ScanArgs) -> Result<Vec<u8>> {
    let bytes = if let Some(hex) = &args.hex {
        decode_hex(hex)?
    } else if let Some(file) = &args.file {
        let raw = std::fs::read(file)?;
        if raw.len() == 32 {
            raw
        } else {
            // Treat as hex/whitespace text.
            let text = String::from_utf8_lossy(&raw);
            decode_hex(&text)?
        }
    } else {
        bail!("provide the fingerprint with --file <path> or --hex <64-char hex>");
    };
    if bytes.len() != 32 {
        bail!(
            "a fingerprint must be exactly 32 bytes (got {})",
            bytes.len()
        );
    }
    Ok(bytes)
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !cleaned.len().is_multiple_of(2) {
        bail!("hex fingerprint must have an even number of digits");
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("invalid hex: {e}"))
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct ScanResponseLocal {
    data: Vec<ScanMatch>,
}

#[cfg(test)]
mod tests {
    use super::decode_hex;

    #[test]
    fn decode_hex_parses_and_ignores_whitespace() {
        let out = decode_hex("00 ff 10\n2a").unwrap();
        assert_eq!(out, vec![0x00, 0xff, 0x10, 0x2a]);
    }

    #[test]
    fn decode_hex_rejects_odd_length() {
        assert!(decode_hex("abc").is_err());
    }

    #[test]
    fn decode_hex_rejects_non_hex() {
        assert!(decode_hex("zz").is_err());
    }
}
