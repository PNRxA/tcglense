//! Collection commands: the shared holdings engine plus the collection-only
//! import/sync/export, value history, movers, saved source, and visibility.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};

use super::Ctx;
use super::holdings::{self, ProductHoldingCommand, Surface};
use super::push_opt;
use crate::models::*;
use crate::output::table;

#[derive(Debug, Args)]
pub struct CollectionArgs {
    pub game: String,
    #[command(subcommand)]
    pub command: CollectionCommand,
}

#[derive(Debug, Subcommand)]
pub enum CollectionCommand {
    /// List owned cards.
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
    /// Show owned counts for one card.
    Get { card_id: String },
    /// Set the absolute owned counts for a card (both zero removes it).
    Set {
        card_id: String,
        #[arg(long, default_value_t = 0)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Increment the owned counts for a card.
    Add {
        card_id: String,
        #[arg(long, default_value_t = 1)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Remove a card from the collection.
    Remove { card_id: String },
    /// Collection value / copy summary.
    Summary {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        related: bool,
    },
    /// Per-set owned aggregates.
    Sets,
    /// Owned cards in a drop-grouped set, grouped by drop.
    Drops {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Owned cards in a set, grouped by sub-type.
    Subtypes {
        code: String,
        #[arg(short = 'q', long)]
        query: Option<String>,
        #[arg(long)]
        page: Option<u32>,
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Batch owned counts for the given card ids.
    Owned { ids: Vec<String> },
    /// Biggest holding-value gainers/losers.
    Movers {
        /// day | week | month | year | two_year | three_year | all_time.
        #[arg(long)]
        window: Option<String>,
    },
    /// Collection value over time.
    ValueHistory {
        #[arg(long)]
        range: Option<String>,
    },
    /// Import a collection from a provider (async; polls to completion).
    Import {
        #[arg(long, value_enum)]
        provider: Provider,
        /// Collection URL or bare id.
        #[arg(long)]
        source: String,
        #[arg(long, value_enum, default_value_t = Mode::Merge)]
        mode: Mode,
        /// Enqueue only; don't poll for completion.
        #[arg(long)]
        no_wait: bool,
    },
    /// Import from a CSV export file (synchronous).
    ImportCsv {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = Mode::Merge)]
        mode: Mode,
    },
    /// Poll an import/sync job by id.
    Job {
        job_id: i64,
        /// Poll until the job finishes.
        #[arg(long)]
        wait: bool,
    },
    /// Re-sync from the saved source (async; polls to completion).
    Sync {
        #[arg(long)]
        no_wait: bool,
    },
    /// Manage the saved collection source link.
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// Export the whole collection as a provider-shaped CSV.
    Export {
        #[arg(long, value_enum, default_value_t = ExportFormat::Archidekt)]
        format: ExportFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Manage public sharing / display prefs for this collection.
    Visibility {
        #[command(subcommand)]
        command: VisibilityCommand,
    },
    /// Manage held sealed products.
    Products {
        #[command(subcommand)]
        command: ProductHoldingCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// Show the saved source link.
    Show,
    /// Save/replace the source link.
    Set {
        #[arg(long, value_enum)]
        provider: Provider,
        #[arg(long)]
        source: String,
        /// Use smart (incremental) sync for re-syncs.
        #[arg(long)]
        smart: bool,
    },
    /// Forget the saved source link.
    Delete,
}

#[derive(Debug, Subcommand)]
pub enum VisibilityCommand {
    /// Show the current visibility + display prefs.
    Show,
    /// Update sharing / display prefs.
    Set {
        #[arg(long)]
        public: Option<bool>,
        #[arg(long)]
        value_chart: Option<bool>,
        #[arg(long)]
        movers: Option<bool>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Provider {
    Archidekt,
    Moxfield,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Provider::Archidekt => "archidekt",
            Provider::Moxfield => "moxfield",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Mode {
    Overwrite,
    Replace,
    Merge,
    Smart,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Overwrite => "overwrite",
            Mode::Replace => "replace",
            Mode::Merge => "merge",
            Mode::Smart => "smart",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Archidekt,
    Moxfield,
}

impl ExportFormat {
    fn as_str(self) -> &'static str {
        match self {
            ExportFormat::Archidekt => "archidekt",
            ExportFormat::Moxfield => "moxfield",
        }
    }
}

pub async fn run(ctx: &Ctx, args: CollectionArgs) -> Result<()> {
    let s = Surface {
        base: format!("/api/collection/{}", args.game),
        batch_route: "owned",
        product_batch_route: "owned",
        noun: "Owned",
    };
    match args.command {
        CollectionCommand::List {
            query,
            set,
            related,
            sort,
            dir,
            page,
            page_size,
        } => holdings::list(ctx, &s, query, set, related, sort, dir, page, page_size).await,
        CollectionCommand::Get { card_id } => holdings::get(ctx, &s, &card_id).await,
        CollectionCommand::Set { card_id, qty, foil } => {
            holdings::set(ctx, &s, &card_id, qty, foil).await
        }
        CollectionCommand::Add { card_id, qty, foil } => {
            holdings::add(ctx, &s, &card_id, qty, foil).await
        }
        CollectionCommand::Remove { card_id } => holdings::set(ctx, &s, &card_id, 0, 0).await,
        CollectionCommand::Summary { set, related } => {
            holdings::summary(ctx, &s, set, related).await
        }
        CollectionCommand::Sets => holdings::sets(ctx, &s).await,
        CollectionCommand::Drops {
            code,
            query,
            page,
            page_size,
        } => holdings::set_drops(ctx, &s, &code, query, page, page_size).await,
        CollectionCommand::Subtypes {
            code,
            query,
            page,
            page_size,
        } => holdings::set_subtypes(ctx, &s, &code, query, page, page_size).await,
        CollectionCommand::Owned { ids } => holdings::batch_counts(ctx, &s, ids).await,
        CollectionCommand::Movers { window } => movers(ctx, &s, window).await,
        CollectionCommand::ValueHistory { range } => value_history(ctx, &s, range).await,
        CollectionCommand::Import {
            provider,
            source,
            mode,
            no_wait,
        } => import(ctx, &s, provider, source, mode, no_wait).await,
        CollectionCommand::ImportCsv { file, mode } => import_csv(ctx, &s, file, mode).await,
        CollectionCommand::Job { job_id, wait } => {
            if wait {
                let job = wait_for_job(ctx, &s, job_id).await?;
                report_job(ctx, &job);
            } else {
                let job = fetch_job(ctx, &s, job_id).await?;
                report_job(ctx, &job);
            }
            Ok(())
        }
        CollectionCommand::Sync { no_wait } => sync(ctx, &s, no_wait).await,
        CollectionCommand::Source { command } => source(ctx, &s, command).await,
        CollectionCommand::Export { format, output } => export(ctx, &s, format, output).await,
        CollectionCommand::Visibility { command } => visibility(ctx, &s, command).await,
        CollectionCommand::Products { command } => holdings::products(ctx, &s, command).await,
    }
}

async fn value_history(ctx: &Ctx, s: &Surface, range: Option<String>) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "range", &range);
    let path = format!("{}/value-history", s.base);
    let body: DataBody<Vec<CollectionValuePoint>> = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&body.data)?;
    } else {
        let mut t = table(&["Date", "Cards USD", "Sealed USD"]);
        for p in &body.data {
            t.add_row(vec![
                p.date.clone(),
                crate::output::price(&p.value_usd),
                crate::output::price(&p.sealed_value_usd),
            ]);
        }
        println!("{t}");
    }
    Ok(())
}

async fn movers(ctx: &Ctx, s: &Surface, window: Option<String>) -> Result<()> {
    let mut q: Vec<(&str, String)> = Vec::new();
    push_opt(&mut q, "window", &window);
    let path = format!("{}/movers", s.base);
    let movers: CollectionMovers = ctx.client.get_json(&path, &q).await?;
    if ctx.printer.json {
        ctx.printer.json(&movers)?;
        return Ok(());
    }
    println!("as_of: {}", movers.as_of.as_deref().unwrap_or("—"));
    let windows = [
        ("day", &movers.day),
        ("week", &movers.week),
        ("month", &movers.month),
        ("year", &movers.year),
        ("two_year", &movers.two_year),
        ("three_year", &movers.three_year),
        ("all_time", &movers.all_time),
    ];
    for (name, list) in windows {
        if list.gainers.is_empty() && list.losers.is_empty() {
            continue;
        }
        println!("\n[{name}]");
        print_mover_rows("gainers", &list.gainers);
        print_mover_rows("losers", &list.losers);
    }
    Ok(())
}

fn print_mover_rows(label: &str, movers: &[CollectionMover]) {
    if movers.is_empty() {
        return;
    }
    println!("  {label}:");
    for m in movers {
        let pct = m
            .change_pct
            .map(|p| format!("{p:+.1}%"))
            .unwrap_or_else(|| "—".into());
        println!(
            "    {:<30} {} → {}  (Δ {} / {})",
            crate::output::truncate(&m.card.name, 30),
            m.value_prev,
            m.value_now,
            m.change_usd,
            pct
        );
    }
}

async fn import(
    ctx: &Ctx,
    s: &Surface,
    provider: Provider,
    source: String,
    mode: Mode,
    no_wait: bool,
) -> Result<()> {
    let body = serde_json::json!({
        "provider": provider.as_str(),
        "source": source,
        "mode": mode.as_str(),
    });
    let job: ImportJob = ctx
        .client
        .post_json(&format!("{}/import", s.base), body)
        .await?;
    ctx.printer.note(format!(
        "Enqueued import job {} ({}).",
        job.job_id, job.status
    ));
    if no_wait {
        if ctx.printer.json {
            ctx.printer.json(&job)?;
        }
        return Ok(());
    }
    let job = wait_for_job(ctx, s, job.job_id).await?;
    report_job(ctx, &job);
    Ok(())
}

async fn import_csv(ctx: &Ctx, s: &Surface, file: PathBuf, mode: Mode) -> Result<()> {
    let text = std::fs::read_to_string(&file)?;
    let path = format!("{}/import/csv", s.base);
    let summary: ImportSummary = ctx
        .client
        .post_text(
            &path,
            &[("mode", mode.as_str().to_string())],
            text,
            "text/csv",
        )
        .await?;
    print_summary(ctx, &summary);
    Ok(())
}

async fn sync(ctx: &Ctx, s: &Surface, no_wait: bool) -> Result<()> {
    let job: ImportJob = ctx
        .client
        .post_json(&format!("{}/sync", s.base), serde_json::json!({}))
        .await?;
    ctx.printer.note(format!(
        "Enqueued sync job {} ({}).",
        job.job_id, job.status
    ));
    if no_wait {
        if ctx.printer.json {
            ctx.printer.json(&job)?;
        }
        return Ok(());
    }
    let job = wait_for_job(ctx, s, job.job_id).await?;
    report_job(ctx, &job);
    Ok(())
}

async fn fetch_job(ctx: &Ctx, s: &Surface, job_id: i64) -> Result<ImportJob> {
    let path = format!("{}/import/jobs/{}", s.base, job_id);
    ctx.client.get_json(&path, &[]).await
}

async fn wait_for_job(ctx: &Ctx, s: &Surface, job_id: i64) -> Result<ImportJob> {
    loop {
        let job = fetch_job(ctx, s, job_id).await?;
        match job.status.as_str() {
            "complete" | "error" => return Ok(job),
            _ => {
                if !ctx.printer.json {
                    let progress = job
                        .progress
                        .as_ref()
                        .map(|p| match p.total {
                            Some(t) => format!(" {}/{}", p.fetched, t),
                            None => format!(" {} fetched", p.fetched),
                        })
                        .unwrap_or_default();
                    println!("  {}{progress}…", job.status);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn report_job(ctx: &Ctx, job: &ImportJob) {
    if ctx.printer.json {
        let _ = ctx.printer.json(job);
        return;
    }
    match job.status.as_str() {
        "complete" => {
            if let Some(summary) = &job.summary {
                print_summary(ctx, summary);
            } else {
                println!("Job {} complete.", job.job_id);
            }
        }
        "error" => {
            println!(
                "Job {} failed: {}",
                job.job_id,
                job.error.as_deref().unwrap_or("unknown error")
            );
        }
        other => println!("Job {} status: {other}", job.job_id),
    }
}

fn print_summary(ctx: &Ctx, s: &ImportSummary) {
    if ctx.printer.json {
        let _ = ctx.printer.json(s);
        return;
    }
    println!("Import ({} / {}):", s.provider, s.mode);
    println!("  rows fetched   : {}", s.total_rows);
    println!("  distinct cards : {}", s.distinct_cards);
    println!("  matched        : {}", s.matched_cards);
    println!("  unmatched      : {}", s.unmatched_cards);
    println!("  regular copies : {}", s.regular_copies);
    println!("  foil copies    : {}", s.foil_copies);
    if s.removed_cards > 0 {
        println!("  removed        : {}", s.removed_cards);
    }
    if s.stopped_early {
        println!("  stopped early  : yes (smart sync)");
    }
    if !s.unmatched_sample.is_empty() {
        println!("  unmatched e.g. : {}", s.unmatched_sample.join(", "));
    }
}

async fn source(ctx: &Ctx, s: &Surface, cmd: SourceCommand) -> Result<()> {
    let path = format!("{}/source", s.base);
    match cmd {
        SourceCommand::Show => {
            let src: Option<CollectionSource> = ctx.client.get_json(&path, &[]).await?;
            match src {
                None => ctx.printer.note("No saved source."),
                Some(src) => {
                    if ctx.printer.json {
                        ctx.printer.json(&src)?;
                    } else {
                        println!("provider    : {}", src.provider);
                        println!("external_id : {}", src.external_id);
                        println!("url         : {}", src.url);
                        println!("smart       : {}", src.smart);
                        println!(
                            "last synced : {}",
                            src.last_synced_at.as_deref().unwrap_or("never")
                        );
                    }
                }
            }
        }
        SourceCommand::Set {
            provider,
            source,
            smart,
        } => {
            let body = serde_json::json!({
                "provider": provider.as_str(),
                "source": source,
                "smart": smart,
            });
            let src: CollectionSource = ctx.client.put_json(&path, body).await?;
            if ctx.printer.json {
                ctx.printer.json(&src)?;
            } else {
                println!("Saved source: {} ({}).", src.url, src.provider);
            }
        }
        SourceCommand::Delete => {
            ctx.client.delete(&path).await?;
            ctx.printer.note("Deleted saved source.");
        }
    }
    Ok(())
}

async fn export(
    ctx: &Ctx,
    s: &Surface,
    format: ExportFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let path = format!("{}/export", s.base);
    let csv = ctx
        .client
        .get_text(&path, &[("format", format.as_str().to_string())])
        .await?;
    match output {
        Some(p) => {
            std::fs::write(&p, csv.as_bytes())?;
            ctx.printer
                .note(format!("Wrote collection to {}.", p.display()));
        }
        None => print!("{csv}"),
    }
    Ok(())
}

async fn visibility(ctx: &Ctx, s: &Surface, cmd: VisibilityCommand) -> Result<()> {
    let path = format!("{}/visibility", s.base);
    match cmd {
        VisibilityCommand::Show => {
            let v: CollectionVisibility = ctx.client.get_json(&path, &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&v)?;
            } else {
                println!("public          : {}", v.public);
                println!("show_value_chart: {}", v.show_value_chart);
                println!("show_movers     : {}", v.show_movers);
                println!(
                    "handle          : {}",
                    v.handle.as_deref().unwrap_or("(none)")
                );
            }
        }
        VisibilityCommand::Set {
            public,
            value_chart,
            movers,
        } => {
            if public.is_none() && value_chart.is_none() && movers.is_none() {
                bail!("provide at least one of --public / --value-chart / --movers");
            }
            let body = serde_json::json!({
                "public": public,
                "show_value_chart": value_chart,
                "show_movers": movers,
            });
            let v: CollectionVisibility = ctx.client.put_json(&path, body).await?;
            if ctx.printer.json {
                ctx.printer.json(&v)?;
            } else {
                println!(
                    "public: {}  ·  handle: {}",
                    v.public,
                    v.handle.as_deref().unwrap_or("(none)")
                );
            }
        }
    }
    Ok(())
}
