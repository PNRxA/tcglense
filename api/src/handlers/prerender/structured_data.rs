//! Server-side port of `web/src/lib/structuredData.ts` (+ the `productType.ts` label
//! map and `money.ts::formatUsd`): the pure builders for the SEO enrichment on the
//! card and sealed-product pages — keyword-rich meta descriptions and schema.org
//! `Product` / `BreadcrumbList` JSON-LD.
//!
//! This is a **byte-faithful** mirror of the TypeScript so a crawler served the
//! prerendered document (see the module docs) sees exactly what `usePageMeta` would
//! set at runtime. Each function carries a `// TS twin:` marker; keep the two in sync
//! (the `prerender.rs` golden tests fail if a visible string drifts). Two invariants
//! carried over verbatim (see the TS header): **no** `offers`/`price`/`availability`
//! in the structured data (a price *tracker*, not a storefront), and contents are
//! linked with `isRelatedTo`, not `hasPart`.
//!
//! One deliberate simplification vs. the client: the prerenderer resolves a product
//! **without** its `/contents` composition (that would be a second DB read per bot
//! hit), so the "what's in the box" clause and the `isRelatedTo` links are omitted —
//! a valid degraded state (title/description/image/breadcrumbs are unaffected).

use serde_json::{Map, Value, json};

use crate::handlers::catalog::ProductResponse;
use crate::handlers::shared::CardResponse;

/// Fixed call-to-action closing every meta description; always survives the budget.
const TRACKING_TAIL: &str = "Track its price history on TCGLense.";
/// Meta-description length target — Google truncates the snippet near here.
const MAX_DESCRIPTION: usize = 160;
/// JSON-LD `description` cap (a summary, not the SERP snippet).
const MAX_JSON_LD_DESCRIPTION: usize = 500;

// ---------- Small text helpers (TS twins in structuredData.ts / money.ts) ----------

/// TS twin: `capitalize`. `Some("rare")` → `"Rare"`; `None`/empty → `""`.
pub(super) fn capitalize(s: Option<&str>) -> String {
    match s {
        Some(s) if !s.is_empty() => {
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// TS twin: `manaCostPlain`. `"{2}{W}{U}"` → `"2WU"`, `"{W/U}"` → `"W/U"`; `None`/empty
/// → `None`. Strips the `{…}` braces Scryfall wraps each symbol in. (Dropping every
/// brace is equivalent to the TS `\{([^}]+)\}` replace for all well-formed Scryfall
/// costs/oracle text, which never contain a lone brace.)
fn mana_cost_plain(cost: Option<&str>) -> Option<String> {
    let cost = cost?;
    if cost.is_empty() {
        return None;
    }
    let stripped: String = cost.chars().filter(|c| *c != '{' && *c != '}').collect();
    (!stripped.is_empty()).then_some(stripped)
}

/// TS twin: the same `\{([^}]+)\}` → inner strip, applied to oracle text.
fn strip_braces(text: &str) -> String {
    text.chars().filter(|c| *c != '{' && *c != '}').collect()
}

/// TS twin: `colorNames`. `["W","U"]` → `"White/Blue"`; `[]` → `None`; unknown letters
/// pass through as-is.
fn color_names(letters: &[String]) -> Option<String> {
    if letters.is_empty() {
        return None;
    }
    let named = letters
        .iter()
        .map(|l| match l.as_str() {
            "W" => "White",
            "U" => "Blue",
            "B" => "Black",
            "R" => "Red",
            "G" => "Green",
            "C" => "Colorless",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("/");
    Some(named)
}

/// TS twin: `money.ts::formatUsd`. `Some("1234.5")` → `Some("$1,234.50")`; a non-finite
/// value → `Some("$<raw>")`; `None`/empty → `None`. Pins en-US grouping (what the
/// browser's `toLocaleString(undefined, …)` yields for the site's audience).
pub(super) fn format_usd(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<f64>() {
        Ok(n) if n.is_finite() => Some(format!("${}", group_thousands(n))),
        _ => Some(format!("${raw}")),
    }
}

/// Format a finite number with thousands separators and exactly two decimals
/// (`1234.5` → `1,234.50`). Rust std has no locale grouping, so group manually. Rounds to
/// cents half-away-from-zero (`f64::round`), matching JS `toLocaleString`'s `halfExpand`,
/// not `format!("{:.2}")`'s half-to-even — so an exact 3rd-decimal tie (`0.125`) formats
/// like the SPA (`0.13`, not `0.12`).
fn group_thousands(n: f64) -> String {
    let negative = n.is_sign_negative() && n != 0.0;
    let cents = (n.abs() * 100.0).round() as i128;
    let int_part = (cents / 100).to_string();
    let frac = cents % 100;
    let bytes = int_part.as_bytes();
    let len = bytes.len();
    let mut grouped = String::with_capacity(len + len / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    format!("{}{}.{:02}", if negative { "-" } else { "" }, grouped, frac)
}

/// TS twin: `productType.ts::productTypeLabel`. A readable label for a sealed-product
/// type slug, or a humanised fallback (`play_pack` → `Play Pack`) for an unknown one.
pub(super) fn product_type_label(slug: &str) -> String {
    let mapped = match slug {
        "collector_display" => "Collector Booster Box",
        "collector_pack" => "Collector Booster Pack",
        "play_display" => "Play Booster Box",
        "play_pack" => "Play Booster Pack",
        "set_display" => "Set Booster Box",
        "set_pack" => "Set Booster Pack",
        "draft_display" => "Draft Booster Box",
        "draft_pack" => "Draft Booster Pack",
        "prerelease" => "Prerelease Pack",
        "commander_deck" => "Commander Deck",
        "secret_lair" => "Secret Lair",
        "bundle" => "Bundle",
        "case" => "Case",
        "starter" => "Starter",
        "display" => "Booster Box",
        "pack" => "Booster Pack",
        "other" => "Other",
        _ => return humanise(slug),
    };
    mapped.to_string()
}

/// TS twin: `productType.ts::humanise`. `play_pack` → `Play Pack`; blank in, blank out.
fn humanise(slug: &str) -> String {
    slug.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| capitalize(Some(w)))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- Meta-description assembly (TS twin: assembleMetaDescription) ----------

/// TS twin: `assembleMetaDescription`. The `lead` (always kept) plus the longest prefix
/// of `clauses`, in priority order, that fits within `MAX_DESCRIPTION` once the fixed
/// tail is appended — so the tail always survives. Assembly STOPS at the first clause
/// that doesn't fit (a lower-priority clause can't take a dropped one's place); empty
/// clauses are skipped without stopping. Whitespace is collapsed.
fn assemble_meta_description(lead: &str, clauses: &[Option<String>]) -> String {
    let mut out = lead.to_string();
    for clause in clauses.iter().flatten() {
        if clause.is_empty() {
            continue;
        }
        // `${out} ${clause} ${tail}`.length — char count matches JS UTF-16 length for
        // the BMP text these carry.
        let candidate = out.chars().count() + 1 + clause.chars().count() + 1 + TRACKING_TAIL.chars().count();
        if candidate > MAX_DESCRIPTION {
            break;
        }
        out.push(' ');
        out.push_str(clause);
    }
    collapse_whitespace(&format!("{out} {TRACKING_TAIL}"))
        .trim()
        .to_string()
}

/// TS twin: `.replace(/\s+/g, ' ')` — collapse every whitespace run to one space.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

/// Join the present, non-empty parts with `sep` (TS twin: `[…].filter(Boolean).join`).
fn join_present(parts: &[Option<String>], sep: &str) -> String {
    parts
        .iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(sep)
}

/// Take the first `max` characters (a char-safe stand-in for JS `.slice(0, max)` on the
/// Latin text these fields carry).
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The `YYYY-MM-DD` prefix if `released_at` starts with an ISO date, else `None`
/// (TS twin: the `/^\d{4}-\d{2}-\d{2}/` test + `.slice(0, 10)`).
fn release_date(released_at: Option<&str>) -> Option<String> {
    let raw = released_at?;
    let bytes = raw.as_bytes();
    let is_iso = bytes.len() >= 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    is_iso.then(|| raw[0..10].to_string())
}

// ---------- schema.org node helpers ----------

/// Append one `PropertyValue`, dropping an absent or empty-string value (TS twin: `prop`
/// — but a numeric `0`, e.g. a card's mana value, is kept).
fn push_prop(props: &mut Vec<Value>, name: &str, value: Option<Value>) {
    let Some(value) = value else { return };
    if let Value::String(s) = &value
        && s.is_empty()
    {
        return;
    }
    props.push(json!({ "@type": "PropertyValue", "name": name, "value": value }));
}

/// A JSON number that renders a whole `f64` as an integer (`3.0` → `3`, matching JS).
fn number_value(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// Resolve a root-relative path (or an already-absolute URL) against `base`
/// (TS twin: `absoluteUrl`, but against `config.public_site_url`, not `window.origin`).
fn absolute(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('/') {
        format!("{base}/{rest}")
    } else {
        format!("{base}/{path}")
    }
}

// ---------- Card ----------

/// TS twin: `cardMetaDescription`. `name — rarity/type · set · #number.` + latest price.
pub(super) fn card_meta_description(c: &CardResponse) -> String {
    let rarity = c
        .rarity
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|r| capitalize(Some(r)));
    let descriptor = join_present(&[rarity, c.type_line.clone()], " ");
    let provenance = join_present(
        &[Some(c.set_name.clone()), Some(format!("#{}", c.collector_number))],
        " · ",
    );
    let inner = join_present(&[non_empty(descriptor), Some(provenance)], " · ");
    let lead = format!("{} — {}.", c.name, inner);
    let usd = format_usd(c.prices.usd.as_deref()).map(|u| format!("Latest price {u}."));
    assemble_meta_description(&lead, &[usd])
}

/// TS twin: `cardJsonLdDescription`. Factual lead + oracle text (both faces joined), capped.
fn card_json_ld_description(c: &CardResponse) -> String {
    let descriptor = join_present(
        &[
            c.rarity
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|r| capitalize(Some(r))),
            c.type_line.clone(),
            non_empty(c.set_name.clone()).map(|s| format!("from {s}")),
        ],
        " ",
    );
    let oracle = if c.faces.is_empty() {
        c.oracle_text.clone().unwrap_or_default()
    } else {
        c.faces
            .iter()
            .filter_map(|f| f.oracle_text.as_deref())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" // ")
    };
    let body = if oracle.is_empty() {
        String::new()
    } else {
        strip_braces(&oracle)
    };
    let head = if descriptor.is_empty() {
        format!("{}.", c.name)
    } else {
        format!("{} — {descriptor}.", c.name)
    };
    truncate_chars(&join_present(&[Some(head), non_empty(body)], " "), MAX_JSON_LD_DESCRIPTION)
}

/// TS twin: `cardProductNode`. schema.org `Product` for a card — deliberately no `offers`.
pub(super) fn card_product_node(c: &CardResponse, image: Option<&str>) -> Value {
    let mut props = Vec::new();
    push_prop(&mut props, "Set", non_empty(c.set_name.clone()).map(Value::String));
    push_prop(&mut props, "Set code", Some(Value::String(c.set_code.to_uppercase())));
    push_prop(&mut props, "Collector number", Some(Value::String(c.collector_number.clone())));
    push_prop(
        &mut props,
        "Rarity",
        c.rarity
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|r| Value::String(capitalize(Some(r)))),
    );
    push_prop(&mut props, "Mana cost", mana_cost_plain(c.mana_cost.as_deref()).map(Value::String));
    push_prop(&mut props, "Mana value", c.cmc.map(number_value));
    push_prop(&mut props, "Color identity", color_names(&c.color_identity).map(Value::String));
    push_prop(&mut props, "Power", c.power.clone().map(Value::String));
    push_prop(&mut props, "Toughness", c.toughness.clone().map(Value::String));
    push_prop(&mut props, "Loyalty", c.loyalty.clone().map(Value::String));
    if c.lang != "en" && !c.lang.is_empty() {
        push_prop(&mut props, "Language", Some(Value::String(c.lang.clone())));
    }

    let mut node = Map::new();
    node.insert("@type".into(), json!("Product"));
    node.insert("name".into(), json!(c.name));
    node.insert("brand".into(), json!({ "@type": "Brand", "name": c.set_name }));
    node.insert("description".into(), json!(card_json_ld_description(c)));
    node.insert(
        "sku".into(),
        json!(format!("{}-{}", c.set_code.to_uppercase(), c.collector_number)),
    );
    node.insert("additionalProperty".into(), Value::Array(props));
    if let Some(img) = image {
        node.insert("image".into(), json!(img));
    }
    if let Some(tl) = c.type_line.as_deref().filter(|s| !s.is_empty()) {
        node.insert("category".into(), json!(tl));
    }
    if let Some(date) = release_date(c.released_at.as_deref()) {
        node.insert("releaseDate".into(), json!(date));
    }
    Value::Object(node)
}

// ---------- Sealed product ----------

/// TS twin: `productMetaDescription` (with no `/contents` — the price is the only clause).
pub(super) fn product_meta_description(p: &ProductResponse, type_label: &str, set_name: &str) -> String {
    let name_lc = p.name.to_lowercase();
    let context = [type_label, set_name]
        .into_iter()
        .filter(|t| !t.is_empty() && !name_lc.contains(&t.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" · ");
    let lead = if context.is_empty() {
        format!("{}.", p.name)
    } else {
        format!("{} — {context}.", p.name)
    };
    let usd = format_usd(p.prices.usd.as_deref()).map(|u| format!("Latest price {u}."));
    assemble_meta_description(&lead, &[usd])
}

/// TS twin: `productJsonLdDescription` (no components → the factual lead alone).
fn product_json_ld_description(p: &ProductResponse, type_label: &str, set_name: &str) -> String {
    let from = if set_name.is_empty() {
        String::new()
    } else {
        format!(" from {set_name}")
    };
    truncate_chars(&format!("{} is a {type_label}{from}.", p.name), MAX_JSON_LD_DESCRIPTION)
}

/// TS twin: `sealedProductNode`. schema.org `Product` for a sealed product — no `offers`,
/// and (no `/contents` resolved here) no `isRelatedTo`.
pub(super) fn sealed_product_node(
    p: &ProductResponse,
    type_label: &str,
    set_name: &str,
    image: Option<&str>,
) -> Value {
    let mut props = Vec::new();
    push_prop(&mut props, "Set", non_empty(set_name.to_string()).map(Value::String));
    push_prop(&mut props, "Set code", Some(Value::String(p.set_code.to_uppercase())));
    push_prop(&mut props, "Product type", non_empty(type_label.to_string()).map(Value::String));

    let mut node = Map::new();
    node.insert("@type".into(), json!("Product"));
    node.insert("name".into(), json!(p.name));
    node.insert("description".into(), json!(product_json_ld_description(p, type_label, set_name)));
    node.insert("sku".into(), json!(p.id));
    node.insert("additionalProperty".into(), Value::Array(props));
    if !type_label.is_empty() {
        node.insert("category".into(), json!(type_label));
    }
    if !set_name.is_empty() {
        node.insert("brand".into(), json!({ "@type": "Brand", "name": set_name }));
    }
    if let Some(img) = image {
        node.insert("image".into(), json!(img));
    }
    if let Some(date) = release_date(p.released_at.as_deref()) {
        node.insert("releaseDate".into(), json!(date));
    }
    Value::Object(node)
}

// ---------- Breadcrumbs + graph ----------

/// One breadcrumb step: a label and an optional root-relative path (the terminal
/// current-page crumb has `None`, and omits `item` — valid per schema.org).
pub(super) type Crumb = (String, Option<String>);

/// TS twin: `breadcrumbList`. Each `item` is made absolute against `base`.
pub(super) fn breadcrumb_list(base: &str, crumbs: &[Crumb]) -> Value {
    let items: Vec<Value> = crumbs
        .iter()
        .enumerate()
        .map(|(i, (label, to))| {
            let mut el = Map::new();
            el.insert("@type".into(), json!("ListItem"));
            el.insert("position".into(), json!(i + 1));
            el.insert("name".into(), json!(label));
            if let Some(path) = to {
                el.insert("item".into(), json!(absolute(base, path)));
            }
            Value::Object(el)
        })
        .collect();
    json!({ "@type": "BreadcrumbList", "itemListElement": items })
}

/// TS twin: `graph`. Wrap the (already-present) nodes in one `@graph`.
pub(super) fn graph(nodes: Vec<Value>) -> Value {
    json!({ "@context": "https://schema.org", "@graph": nodes })
}

/// TS twin: `cardCrumbs` — Home › Cards › {Set} › {Card}.
pub(super) fn card_crumbs(game: &str, c: &CardResponse) -> Vec<Crumb> {
    vec![
        ("Home".into(), Some("/".into())),
        ("Cards".into(), Some(format!("/cards/{game}/cards"))),
        (c.set_name.clone(), Some(format!("/cards/{game}/sets/{}", c.set_code))),
        (c.name.clone(), None),
    ]
}

/// TS twin: `sealedCrumbs` — Home › Sealed › {Product}.
pub(super) fn sealed_crumbs(game: &str, p: &ProductResponse) -> Vec<Crumb> {
    vec![
        ("Home".into(), Some("/".into())),
        ("Sealed".into(), Some(format!("/sealed/{game}"))),
        (p.name.clone(), None),
    ]
}

/// `Some(s)` when `s` is non-empty, else `None` (JS `s || null`).
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_usd_matches_the_ts() {
        assert_eq!(format_usd(Some("1234.5")).as_deref(), Some("$1,234.50"));
        assert_eq!(format_usd(Some("0")).as_deref(), Some("$0.00"));
        assert_eq!(format_usd(Some("9.999")).as_deref(), Some("$10.00"));
        assert_eq!(format_usd(Some("1234567")).as_deref(), Some("$1,234,567.00"));
        // Half-away-from-zero on an exact tie, matching JS toLocaleString.
        assert_eq!(format_usd(Some("0.125")).as_deref(), Some("$0.13"));
        assert_eq!(format_usd(Some("0.625")).as_deref(), Some("$0.63"));
        assert_eq!(format_usd(None), None);
        assert_eq!(format_usd(Some("")), None);
        assert_eq!(format_usd(Some("abc")).as_deref(), Some("$abc"));
    }

    #[test]
    fn capitalize_and_humanise() {
        assert_eq!(capitalize(Some("rare")), "Rare");
        assert_eq!(capitalize(Some("")), "");
        assert_eq!(capitalize(None), "");
        assert_eq!(humanise("play_pack"), "Play Pack");
        assert_eq!(humanise(""), "");
    }

    #[test]
    fn product_type_label_maps_and_falls_back() {
        assert_eq!(product_type_label("collector_display"), "Collector Booster Box");
        assert_eq!(product_type_label("secret_lair"), "Secret Lair");
        assert_eq!(product_type_label("mystery_thing"), "Mystery Thing");
        assert_eq!(product_type_label(""), "");
    }

    #[test]
    fn mana_and_colors() {
        assert_eq!(mana_cost_plain(Some("{2}{W}{U}")).as_deref(), Some("2WU"));
        assert_eq!(mana_cost_plain(Some("{W/U}")).as_deref(), Some("W/U"));
        assert_eq!(mana_cost_plain(Some("")), None);
        assert_eq!(mana_cost_plain(None), None);
        assert_eq!(color_names(&["W".into(), "U".into()]).as_deref(), Some("White/Blue"));
        assert_eq!(color_names(&[]), None);
    }

    #[test]
    fn release_date_only_accepts_iso_prefix() {
        assert_eq!(release_date(Some("2024-01-02T00:00:00")).as_deref(), Some("2024-01-02"));
        assert_eq!(release_date(Some("2024-01-02")).as_deref(), Some("2024-01-02"));
        assert_eq!(release_date(Some("not-a-date")), None);
        assert_eq!(release_date(None), None);
    }

    #[test]
    fn assemble_keeps_lead_and_tail_over_budget() {
        // A lead longer than the budget still survives, tail appended.
        let long_lead = "x".repeat(200);
        let out = assemble_meta_description(&long_lead, &[Some("clause".into())]);
        assert!(out.starts_with(&long_lead));
        assert!(out.ends_with(TRACKING_TAIL));
        // A fitting clause is included; whitespace collapsed.
        let out = assemble_meta_description("Lead.", &[Some("Extra.".into())]);
        assert_eq!(out, "Lead. Extra. Track its price history on TCGLense.");
    }

    #[test]
    fn number_value_renders_whole_as_integer() {
        assert_eq!(number_value(3.0), Value::from(3));
        assert_eq!(number_value(0.0), Value::from(0));
        assert_eq!(number_value(3.5), Value::from(3.5));
    }
}
