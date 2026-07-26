//! Tagger art-tag filter: `art:` / `arttag:` / `atag:` (issue #140) — matches cards
//! whose artwork carries a community Tagger tag, from the ingest-populated
//! `card_art_tags` table (see `crate::scryfall::art_tags`).

use sea_orm::Condition;
use sea_orm::Value;

use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;
use super::common::raw_vals;
use crate::db::Dialect;

/// Normalize user input to Tagger's slug shape: lowercase, whitespace runs collapsed to
/// single hyphens (`art:"Rashida Scalebane"` → `rashida-scalebane`). Anything else binds
/// as-is — an unknown slug just matches nothing.
fn normalize_slug(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// `art:` / `arttag:` / `atag:` — the card's artwork carries the given art tag.
///
/// A correlated, game-scoped `EXISTS` probe against the mapping's `(game, tag_slug,
/// illustration_id)` unique index. Hierarchy resolution happened at ingest (a parent
/// tag's rows already include every descendant's artworks), so no tree walk here. A
/// card without an `illustration_id` never matches (the NULL comparison is false), so
/// the leaf stays total (0/1) and `-`/`not:`/`!=` negate cleanly — matching Scryfall,
/// where `-art:x` includes cards with untagged or absent artwork. An unrecognised tag
/// simply matches no rows (also Scryfall's behaviour).
pub(super) fn art_tag(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let slug = normalize_slug(value);
    if slug.is_empty() {
        return Err(invalid("art", value, "expected a tag name"));
    }
    let select = "SELECT 1 FROM card_art_tags WHERE card_art_tags.game = ? \
         AND card_art_tags.tag_slug = ? \
         AND card_art_tags.illustration_id = cards.illustration_id";
    let bind = || {
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(slug.clone()),
        ]
    };
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(dialect, format!("EXISTS ({select})"), bind())),
        Op::Ne => Ok(raw_vals(dialect, format!("NOT EXISTS ({select})"), bind())),
        _ => Err(unsupported_op("art", op)),
    }
}
