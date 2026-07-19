//! Deck commands: a container surface (many decks per game) with folders, sections,
//! per-card edits, import/export, and public sharing.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};

use super::Ctx;
use crate::models::*;
use crate::output::{self, decks_table, table};

#[derive(Debug, Args)]
pub struct DecksArgs {
    pub game: String,
    #[command(subcommand)]
    pub command: DecksCommand,
}

#[derive(Debug, Subcommand)]
pub enum DecksCommand {
    /// List your decks.
    List,
    /// Show one deck in full (sections + cards).
    Show { deck_id: i64 },
    /// Create a new deck (seeded with default sections).
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        folder: Option<i64>,
    },
    /// Replace a deck's editable metadata.
    Update {
        deck_id: i64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        format: Option<String>,
    },
    /// Delete a deck.
    Delete { deck_id: i64 },
    /// Import a deck from a provider URL/id or an uploaded file.
    Import {
        #[arg(long, value_enum)]
        provider: DeckProvider,
        /// A public deck URL or id (live import).
        #[arg(long, conflicts_with = "file")]
        source: Option<String>,
        /// A deck-list file to upload.
        #[arg(long, conflicts_with = "source")]
        file: Option<PathBuf>,
        /// Uploaded file format (with --file).
        #[arg(long, value_enum, default_value_t = FileFormat::Csv)]
        file_format: FileFormat,
        /// Name for the new deck.
        #[arg(long)]
        name: Option<String>,
        /// Keep generic Mainboard rows exactly (don't auto-file by type).
        #[arg(long)]
        no_auto_categorize: bool,
    },
    /// Export a deck as a provider-shaped list.
    Export {
        deck_id: i64,
        #[arg(long, value_enum, default_value_t = DeckExportFormat::Archidekt)]
        format: DeckExportFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Manage deck folders.
    Folders {
        #[command(subcommand)]
        command: FolderCommand,
    },
    /// File a deck under a folder (or loosen it with no id).
    MoveToFolder {
        deck_id: i64,
        folder_id: Option<i64>,
    },
    /// Manage a deck's sections.
    Sections {
        deck_id: i64,
        #[command(subcommand)]
        command: SectionCommand,
    },
    /// Edit a card within a deck.
    Card {
        deck_id: i64,
        #[command(subcommand)]
        command: DeckCardCommand,
    },
    /// Enable/disable public sharing of a deck.
    Visibility { deck_id: i64, public: bool },
}

#[derive(Debug, Subcommand)]
pub enum FolderCommand {
    List,
    Create { name: String },
    Rename { folder_id: i64, name: String },
    Delete { folder_id: i64 },
}

#[derive(Debug, Subcommand)]
pub enum SectionCommand {
    /// Add a custom section.
    Add { name: String },
    /// Rename and/or reposition a section.
    Update {
        section_id: i64,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        position: Option<i64>,
    },
    /// Set the full section order.
    Reorder { section_ids: Vec<i64> },
    /// Delete a section (its cards move to the first remaining one).
    Delete { section_id: i64 },
}

#[derive(Debug, Subcommand)]
pub enum DeckCardCommand {
    /// Set a card's absolute counts in a section (both zero removes it there).
    Set {
        card_id: String,
        #[arg(long)]
        section: i64,
        #[arg(long, default_value_t = 0)]
        qty: i64,
        #[arg(long, default_value_t = 0)]
        foil: i64,
    },
    /// Move a card between two sections.
    Move {
        card_id: String,
        #[arg(long)]
        from: i64,
        #[arg(long)]
        to: i64,
    },
    /// Swap a card for another printing in a section.
    Printing {
        card_id: String,
        #[arg(long)]
        to_card: String,
        #[arg(long)]
        section: i64,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeckProvider {
    Archidekt,
    Moxfield,
}

impl DeckProvider {
    fn as_str(self) -> &'static str {
        match self {
            DeckProvider::Archidekt => "archidekt",
            DeckProvider::Moxfield => "moxfield",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FileFormat {
    Csv,
    Text,
}

impl FileFormat {
    fn as_str(self) -> &'static str {
        match self {
            FileFormat::Csv => "csv",
            FileFormat::Text => "text",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeckExportFormat {
    Archidekt,
    Moxfield,
    #[value(name = "moxfield-text")]
    MoxfieldText,
}

impl DeckExportFormat {
    fn as_str(self) -> &'static str {
        match self {
            DeckExportFormat::Archidekt => "archidekt",
            DeckExportFormat::Moxfield => "moxfield",
            DeckExportFormat::MoxfieldText => "moxfield-text",
        }
    }
}

pub async fn run(ctx: &Ctx, args: DecksArgs) -> Result<()> {
    let base = format!("/api/decks/{}", args.game);
    match args.command {
        DecksCommand::List => {
            let body: DataBody<Vec<Deck>> = ctx.client.get_json(&base, &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else if body.data.is_empty() {
                println!("No decks.");
            } else {
                decks_table(&body.data);
            }
        }
        DecksCommand::Show { deck_id } => {
            let deck: DeckDetail = ctx
                .client
                .get_json(&format!("{base}/{deck_id}"), &[])
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&deck)?;
            } else {
                print_deck_detail(&deck);
            }
        }
        DecksCommand::Create {
            name,
            description,
            format,
            folder,
        } => {
            let body = serde_json::json!({
                "name": name,
                "description": description,
                "format": format,
                "folder_id": folder,
            });
            let deck: DeckDetail = ctx.client.post_json(&base, body).await?;
            if ctx.printer.json {
                ctx.printer.json(&deck)?;
            } else {
                println!("Created deck '{}' (id {}).", deck.name, deck.id);
            }
        }
        DecksCommand::Update {
            deck_id,
            name,
            description,
            format,
        } => {
            let body = serde_json::json!({
                "name": name,
                "description": description,
                "format": format,
            });
            let deck: Deck = ctx
                .client
                .put_json(&format!("{base}/{deck_id}"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&deck)?;
            } else {
                println!("Updated deck '{}'.", deck.name);
            }
        }
        DecksCommand::Delete { deck_id } => {
            ctx.client.delete(&format!("{base}/{deck_id}")).await?;
            ctx.printer.note(format!("Deleted deck {deck_id}."));
        }
        DecksCommand::Import {
            provider,
            source,
            file,
            file_format,
            name,
            no_auto_categorize,
        } => {
            let (source_val, contents_val, format_val) = match (source, file) {
                (Some(src), None) => (Some(src), None, None),
                (None, Some(path)) => {
                    let text = std::fs::read_to_string(&path)?;
                    (None, Some(text), Some(file_format.as_str()))
                }
                (Some(_), Some(_)) => bail!("provide only one of --source or --file"),
                (None, None) => bail!("provide --source <url/id> or --file <path>"),
            };
            let body = serde_json::json!({
                "provider": provider.as_str(),
                "source": source_val,
                "contents": contents_val,
                "format": format_val,
                "name": name,
                "auto_categorize": !no_auto_categorize,
            });
            let resp: DeckImportResponse = ctx
                .client
                .post_json(&format!("{base}/import"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&resp)?;
            } else {
                println!(
                    "Imported '{}' (id {}): {} rows, {} matched, {} unmatched.",
                    resp.deck.name,
                    resp.deck.id,
                    resp.total_rows,
                    resp.matched_cards,
                    resp.unmatched_cards
                );
                if !resp.unmatched_sample.is_empty() {
                    println!("  unmatched e.g. {}", resp.unmatched_sample.join(", "));
                }
            }
        }
        DecksCommand::Export {
            deck_id,
            format,
            output,
        } => {
            let csv = ctx
                .client
                .get_text(
                    &format!("{base}/{deck_id}/export"),
                    &[("format", format.as_str().to_string())],
                )
                .await?;
            match output {
                Some(p) => {
                    std::fs::write(&p, csv.as_bytes())?;
                    ctx.printer.note(format!("Wrote deck to {}.", p.display()));
                }
                None => print!("{csv}"),
            }
        }
        DecksCommand::Folders { command } => folders(ctx, &base, command).await?,
        DecksCommand::MoveToFolder { deck_id, folder_id } => {
            let body = serde_json::json!({ "folder_id": folder_id });
            let deck: Deck = ctx
                .client
                .put_json(&format!("{base}/{deck_id}/folder"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&deck)?;
            } else {
                match deck.folder_id {
                    Some(f) => println!("Deck '{}' filed under folder {f}.", deck.name),
                    None => println!("Deck '{}' loosened (no folder).", deck.name),
                }
            }
        }
        DecksCommand::Sections { deck_id, command } => {
            sections(ctx, &base, deck_id, command).await?
        }
        DecksCommand::Card { deck_id, command } => deck_card(ctx, &base, deck_id, command).await?,
        DecksCommand::Visibility { deck_id, public } => {
            let body = serde_json::json!({ "public": public });
            let v: DeckVisibility = ctx
                .client
                .put_json(&format!("{base}/{deck_id}/visibility"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&v)?;
            } else {
                println!(
                    "public: {}  ·  handle: {}",
                    v.public,
                    v.handle.as_deref().unwrap_or("(set a username first)")
                );
            }
        }
    }
    Ok(())
}

async fn folders(ctx: &Ctx, base: &str, cmd: FolderCommand) -> Result<()> {
    match cmd {
        FolderCommand::List => {
            let body: DataBody<Vec<DeckFolder>> =
                ctx.client.get_json(&format!("{base}/folders"), &[]).await?;
            if ctx.printer.json {
                ctx.printer.json(&body.data)?;
            } else {
                let mut t = table(&["ID", "Name", "Decks"]);
                for f in &body.data {
                    t.add_row(vec![
                        f.id.to_string(),
                        f.name.clone(),
                        f.deck_count.to_string(),
                    ]);
                }
                println!("{t}");
            }
        }
        FolderCommand::Create { name } => {
            let body = serde_json::json!({ "name": name });
            let f: DeckFolder = ctx
                .client
                .post_json(&format!("{base}/folders"), body)
                .await?;
            ctx.printer
                .note(format!("Created folder '{}' (id {}).", f.name, f.id));
            if ctx.printer.json {
                ctx.printer.json(&f)?;
            }
        }
        FolderCommand::Rename { folder_id, name } => {
            let body = serde_json::json!({ "name": name });
            let f: DeckFolder = ctx
                .client
                .put_json(&format!("{base}/folders/{folder_id}"), body)
                .await?;
            ctx.printer
                .note(format!("Renamed folder {} to '{}'.", f.id, f.name));
        }
        FolderCommand::Delete { folder_id } => {
            ctx.client
                .delete(&format!("{base}/folders/{folder_id}"))
                .await?;
            ctx.printer.note(format!(
                "Deleted folder {folder_id} (its decks were ungrouped)."
            ));
        }
    }
    Ok(())
}

async fn sections(ctx: &Ctx, base: &str, deck_id: i64, cmd: SectionCommand) -> Result<()> {
    let sbase = format!("{base}/{deck_id}/sections");
    match cmd {
        SectionCommand::Add { name } => {
            let body = serde_json::json!({ "name": name });
            let s: DeckSection = ctx.client.post_json(&sbase, body).await?;
            ctx.printer
                .note(format!("Added section '{}' (id {}).", s.name, s.id));
        }
        SectionCommand::Update {
            section_id,
            name,
            position,
        } => {
            let body = serde_json::json!({ "name": name, "position": position });
            let s: DeckSection = ctx
                .client
                .put_json(&format!("{sbase}/{section_id}"), body)
                .await?;
            ctx.printer.note(format!(
                "Updated section '{}' (position {}).",
                s.name, s.position
            ));
        }
        SectionCommand::Reorder { section_ids } => {
            let body = serde_json::json!({ "section_ids": section_ids });
            let out: DataBody<Vec<DeckSection>> = ctx
                .client
                .put_json(&format!("{sbase}/reorder"), body)
                .await?;
            if ctx.printer.json {
                ctx.printer.json(&out.data)?;
            } else {
                let names: Vec<String> = out.data.iter().map(|s| s.name.clone()).collect();
                println!("New order: {}", names.join(" → "));
            }
        }
        SectionCommand::Delete { section_id } => {
            ctx.client.delete(&format!("{sbase}/{section_id}")).await?;
            ctx.printer.note(format!("Deleted section {section_id}."));
        }
    }
    Ok(())
}

async fn deck_card(ctx: &Ctx, base: &str, deck_id: i64, cmd: DeckCardCommand) -> Result<()> {
    let cbase = format!("{base}/{deck_id}/cards");
    let result: CollectionQuantities = match cmd {
        DeckCardCommand::Set {
            card_id,
            section,
            qty,
            foil,
        } => {
            let body = serde_json::json!({
                "quantity": qty,
                "foil_quantity": foil,
                "section_id": section,
            });
            ctx.client
                .put_json(&format!("{cbase}/{card_id}"), body)
                .await?
        }
        DeckCardCommand::Move { card_id, from, to } => {
            let body = serde_json::json!({ "from_section_id": from, "to_section_id": to });
            ctx.client
                .put_json(&format!("{cbase}/{card_id}/move"), body)
                .await?
        }
        DeckCardCommand::Printing {
            card_id,
            to_card,
            section,
        } => {
            let body = serde_json::json!({ "new_card_id": to_card, "section_id": section });
            ctx.client
                .put_json(&format!("{cbase}/{card_id}/printing"), body)
                .await?
        }
    };
    if ctx.printer.json {
        ctx.printer.json(&result)?;
    } else {
        println!(
            "Card now {} / {} foil in section.",
            result.quantity, result.foil_quantity
        );
    }
    Ok(())
}

fn print_deck_detail(d: &DeckDetail) {
    println!("{}  [id {}]", d.name, d.id);
    println!(
        "  format: {}  ·  cards: {}  ·  value: {}  ·  public: {}",
        d.format.as_deref().unwrap_or("—"),
        d.summary.total_cards,
        output::price(&d.summary.total_value_usd),
        d.is_public
    );
    if let Some(desc) = &d.description
        && !desc.is_empty()
    {
        println!("  {desc}");
    }
    let mut by_section: HashMap<i64, Vec<&DeckCardEntry>> = HashMap::new();
    for c in &d.cards {
        by_section.entry(c.section_id).or_default().push(c);
    }
    for section in &d.sections {
        let cards = by_section.get(&section.id);
        let count: i64 = cards
            .map(|cs| cs.iter().map(|c| c.quantity + c.foil_quantity).sum())
            .unwrap_or(0);
        if count == 0 {
            continue;
        }
        println!("\n== {} ({count}) ==", section.name);
        if let Some(cards) = cards {
            let mut t = table(&["Qty", "Foil", "Name", "Set", "#"]);
            for c in cards {
                t.add_row(vec![
                    c.quantity.to_string(),
                    c.foil_quantity.to_string(),
                    output::truncate(&c.card.name, 34),
                    c.card.set_code.to_uppercase(),
                    c.card.collector_number.clone(),
                ]);
            }
            println!("{t}");
        }
    }
}
