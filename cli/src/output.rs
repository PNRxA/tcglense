//! Output rendering: `--json` emits machine-readable JSON; otherwise data is
//! rendered as human-friendly tables (comfy-table) and detail blocks.

use anyhow::Result;
use comfy_table::{Cell, ContentArrangement, Table, presets};
use serde::Serialize;

use crate::models::*;

/// Shared output settings threaded through command handlers.
#[derive(Clone, Copy)]
pub struct Printer {
    pub json: bool,
}

impl Printer {
    pub fn new(json: bool) -> Printer {
        Printer { json }
    }

    /// Emit `value` as pretty JSON (used for every `--json` path).
    pub fn json<T: Serialize>(&self, value: &T) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }

    /// Print a short status line (suppressed in JSON mode).
    pub fn note(&self, msg: impl AsRef<str>) {
        if !self.json {
            println!("{}", msg.as_ref());
        }
    }
}

/// A comfy-table pre-configured for terminal output.
pub fn table(headers: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(presets::UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    if !headers.is_empty() {
        t.set_header(headers.iter().map(Cell::new));
    }
    t
}

pub fn dash(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "—".to_string())
}

pub fn price(v: &Option<String>) -> String {
    match v {
        Some(p) => format!("${p}"),
        None => "—".to_string(),
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let cut: String = chars.into_iter().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

// ---------------------------------------------------------------------------
// Type-specific table renderers (shared by the command handlers).
// ---------------------------------------------------------------------------

pub fn games_table(games: &[Game]) {
    let mut t = table(&["ID", "Name", "Publisher", "Source"]);
    for g in games {
        t.add_row(vec![&g.id, &g.name, &g.publisher, &g.data_source]);
    }
    println!("{t}");
}

pub fn sets_table(sets: &[CardSet]) {
    let mut t = table(&["Code", "Name", "Type", "Released", "Cards"]);
    for s in sets {
        t.add_row(vec![
            s.code.clone(),
            truncate(&s.name, 44),
            dash(&s.set_type),
            dash(&s.released_at),
            s.card_count.to_string(),
        ]);
    }
    println!("{t}");
}

pub fn cards_table(cards: &[Card]) {
    let mut t = table(&["ID", "Name", "Set", "#", "Rarity", "USD", "Foil"]);
    for c in cards {
        t.add_row(vec![
            truncate(&c.id, 12),
            truncate(&c.name, 34),
            c.set_code.to_uppercase(),
            c.collector_number.clone(),
            dash(&c.rarity),
            price(&c.prices.usd),
            price(&c.prices.usd_foil),
        ]);
    }
    println!("{t}");
}

pub fn card_detail(c: &Card) {
    println!("{}  [{}]", c.name, c.id);
    println!(
        "  {} · {} #{} · {}",
        c.set_name,
        c.set_code.to_uppercase(),
        c.collector_number,
        c.rarity.as_deref().unwrap_or("—")
    );
    if let Some(mc) = &c.mana_cost
        && !mc.is_empty()
    {
        println!("  Mana: {mc}   CMC: {}", c.cmc.unwrap_or(0.0));
    }
    if let Some(tl) = &c.type_line {
        println!("  Type: {tl}");
    }
    if let (Some(p), Some(t)) = (&c.power, &c.toughness) {
        println!("  P/T: {p}/{t}");
    }
    if let Some(l) = &c.loyalty {
        println!("  Loyalty: {l}");
    }
    if let Some(ot) = &c.oracle_text
        && !ot.is_empty()
    {
        println!("  ---\n{}", indent(ot));
    }
    for (i, f) in c.faces.iter().enumerate() {
        println!("  --- face {} ---", i + 1);
        if let Some(n) = &f.name {
            println!("  {n}");
        }
        if let Some(tl) = &f.type_line {
            println!("  {tl}");
        }
        if let Some(ot) = &f.oracle_text {
            println!("{}", indent(ot));
        }
    }
    println!(
        "  Prices: USD {} · Foil {} · EUR {} · TIX {}",
        price(&c.prices.usd),
        price(&c.prices.usd_foil),
        dash(&c.prices.eur),
        dash(&c.prices.tix)
    );
    if let Some(drop) = &c.drop_name {
        println!("  Secret Lair drop: {drop}");
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn products_table(products: &[Product]) {
    let mut t = table(&["ID", "Name", "Set", "Type", "USD", "MSRP"]);
    for p in products {
        t.add_row(vec![
            p.id.clone(),
            truncate(&p.name, 40),
            p.set_code.to_uppercase(),
            truncate(&p.product_type, 16),
            price(&p.prices.usd),
            price(&p.msrp),
        ]);
    }
    println!("{t}");
}

pub fn collection_table(entries: &[CollectionEntry], noun: &str) {
    let mut t = table(&["ID", "Name", "Set", "#", noun, "Foil", "USD"]);
    for e in entries {
        t.add_row(vec![
            truncate(&e.card.id, 12),
            truncate(&e.card.name, 32),
            e.card.set_code.to_uppercase(),
            e.card.collector_number.clone(),
            e.quantity.to_string(),
            e.foil_quantity.to_string(),
            price(&e.card.prices.usd),
        ]);
    }
    println!("{t}");
}

pub fn collection_summary(s: &CollectionSummary) {
    println!("Unique cards : {}", s.unique_cards);
    println!("Total copies : {}", s.total_cards);
    println!("Total value  : {}", price(&s.total_value_usd));
    println!("Bulk value   : {}", price(&s.bulk_value_usd));
}

pub fn product_holdings_table(entries: &[ProductHoldingEntry]) {
    let mut t = table(&["ID", "Name", "Set", "Type", "Qty", "Foil", "USD"]);
    for e in entries {
        t.add_row(vec![
            e.product.id.clone(),
            truncate(&e.product.name, 36),
            e.product.set_code.to_uppercase(),
            truncate(&e.product.product_type, 14),
            e.quantity.to_string(),
            e.foil_quantity.to_string(),
            price(&e.product.prices.usd),
        ]);
    }
    println!("{t}");
}

pub fn decks_table(decks: &[Deck]) {
    let mut t = table(&["ID", "Name", "Format", "Cards", "Public", "Updated"]);
    for d in decks {
        t.add_row(vec![
            d.id.to_string(),
            truncate(&d.name, 36),
            dash(&d.format),
            d.card_count.to_string(),
            if d.is_public { "yes" } else { "no" }.to_string(),
            d.updated_at.clone(),
        ]);
    }
    println!("{t}");
}

pub fn apikeys_table(keys: &[ApiKeyInfo]) {
    let mut t = table(&["ID", "Name", "Scope", "Prefix", "Last used", "Expires"]);
    for k in keys {
        t.add_row(vec![
            k.id.to_string(),
            truncate(&k.name, 28),
            k.scope.clone(),
            k.key_prefix.clone(),
            dash(&k.last_used_at),
            dash(&k.expires_at),
        ]);
    }
    println!("{t}");
}

pub fn prices_table(points: &[PricePoint]) {
    let mut t = table(&["Date", "USD", "Foil", "EUR", "TIX"]);
    for p in points {
        t.add_row(vec![
            p.date.clone(),
            price(&p.usd),
            price(&p.usd_foil),
            dash(&p.eur),
            dash(&p.tix),
        ]);
    }
    println!("{t}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_strings() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_clips_with_ellipsis() {
        let out = truncate("abcdefghij", 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_is_char_boundary_safe() {
        // Multi-byte characters must not be split mid-codepoint.
        let out = truncate("héllo wörld ☃☃☃", 6);
        assert!(out.chars().count() <= 6);
    }

    #[test]
    fn price_and_dash_format_optionals() {
        assert_eq!(price(&Some("1.50".into())), "$1.50");
        assert_eq!(price(&None), "—");
        assert_eq!(dash(&None), "—");
    }
}
