//! Set-card grouping shared by the catalog, collection, and wish-list views: the generic
//! bucketing pass (over bare cards or owned holdings) plus the pagination tail every
//! grouped endpoint runs once its rows are in buckets. Two groupings ride on it — Secret
//! Lair **drops** (curated, keyed by collector number; see [`crate::scryfall::drops`]) and
//! set **sub-types** (card treatments derived from print attributes; see
//! [`crate::scryfall::subtypes`]).

use std::collections::BTreeMap;

use super::pagination::Page;
use crate::error::AppError;
use crate::scryfall::drops::DropTable;
use crate::scryfall::subtypes::{self, Subtype};

/// Resolve a game's set to its Secret Lair drop table, `404`ing an set that isn't
/// drop-grouped. This is the single definition of "drop-grouped" the by-drop endpoints
/// gate on, so it must agree with [`crate::scryfall::drops::has_drops`] — the same
/// non-empty-table predicate the SPA uses to decide whether to offer the by-drop view.
pub(crate) fn require_drop_table(
    game: &str,
    set_code: &str,
) -> Result<&'static DropTable, AppError> {
    crate::scryfall::drops::table(game, set_code)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::NotFound(format!("set '{set_code}' has no drops")))
}

/// One group's items, before pagination/serialization (so off-page groups never get
/// turned into response DTOs). Generic over the item type `T`: the public catalog groups
/// bare `card::Model`s, while the per-user collection/wish list groups owned `(item, card)`
/// pairs — every grouping shares this one bucket type.
pub(crate) struct Bucket<T> {
    /// Stable slug for anchors/links; `None` only for the drops' catch-all "Other" group.
    pub(crate) slug: Option<String>,
    pub(crate) title: String,
    pub(crate) cards: Vec<T>,
}

/// Bucket `rows` by a key function mapping each row to `(order, slug, title)`, keeping the
/// buckets in ascending `order`. A bucket exists only once a row lands in it (empty groups
/// never appear), and the first row to land names it. The shared core of the by-drop and
/// by-sub-type passes.
fn group_by<T>(
    rows: Vec<T>,
    key: impl Fn(&T) -> (usize, Option<String>, String),
) -> Vec<Bucket<T>> {
    let mut buckets: BTreeMap<usize, Bucket<T>> = BTreeMap::new();
    for row in rows {
        let (order, slug, title) = key(&row);
        buckets
            .entry(order)
            .or_insert_with(|| Bucket {
                slug,
                title,
                cards: Vec::new(),
            })
            .cards
            .push(row);
    }
    buckets.into_values().collect()
}

/// Group a set's items — already in collector-number order — into Secret Lair drops,
/// preserving Scryfall's drop order. `collector_number` extracts each item's collector
/// number (the drop-table key), so the same grouping works for bare cards and for owned
/// holdings. Items the snapshot doesn't place in a drop collect into a trailing "Other"
/// bucket. Empty drops never appear: a bucket exists only once an item lands in it (so a
/// search that matches a subset yields only the drops with matches).
pub(crate) fn group_into_drops<T>(
    table: &DropTable,
    rows: Vec<T>,
    collector_number: impl Fn(&T) -> &str,
) -> Vec<Bucket<T>> {
    // Sentinel order for the "Other" bucket: `BTreeMap` ordering parks it last.
    const OTHER: usize = usize::MAX;
    group_by(rows, |row| match table.drop_for(collector_number(row)) {
        Some(drop) => (drop.order, Some(drop.slug.clone()), drop.title.clone()),
        None => (OTHER, None, "Other".to_string()),
    })
}

/// Group a set's items into their derived sub-types (card treatments), Normal first then
/// the treatments in sub-type order. `card` extracts each item's joined `card::Model` (so
/// this works for bare cards and owned holdings alike); classification is
/// [`subtypes::classify`]. Sub-types no card matches never appear.
pub(crate) fn group_into_subtypes<T>(
    rows: Vec<T>,
    card: impl Fn(&T) -> &crate::entities::card::Model,
) -> Vec<Bucket<T>> {
    group_by(rows, |row| {
        let subtype: &Subtype = subtypes::classify(card(row));
        (
            subtype.order,
            Some(subtype.slug.to_string()),
            subtype.title.to_string(),
        )
    })
}

/// Narrow already-grouped drop buckets to those whose curated title contains `needle`
/// (a case-insensitive substring match). Powers the by-drop view's "filter drops by
/// name" box — applied after grouping and *before* pagination, so the filter spans the
/// whole set's drops rather than only the page on screen. A blank `needle` matches every
/// drop (`contains("")` is always true), so callers skip the call for an absent filter.
pub(crate) fn filter_drops_by_title<T>(buckets: Vec<Bucket<T>>, needle: &str) -> Vec<Bucket<T>> {
    let needle = needle.to_lowercase();
    buckets
        .into_iter()
        .filter(|bucket| bucket.title.to_lowercase().contains(&needle))
        .collect()
}

/// Paginate already-grouped buckets by *group* (not by card), mapping each on-page bucket
/// into its response shape with `map`. The grouped endpoints pull a whole (bounded) set,
/// group it in memory, then page over the groups — this is the shared tail of that:
/// off-page buckets are skipped before `map` runs, so their cards are never turned into
/// DTOs.
pub(crate) fn paginate_buckets<T, R>(
    buckets: Vec<Bucket<T>>,
    page: u64,
    page_size: u64,
    map: impl Fn(Bucket<T>) -> R,
) -> Page<R> {
    let total = buckets.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let data: Vec<R> = buckets
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(map)
        .collect();
    Page::new(data, page, page_size, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entities::card;
    use crate::test_support::card_model;

    /// A minimal card row; only its set code and collector number matter to the
    /// drop grouping.
    fn sld_test_card(
        set_code: &str,
        collector_number: &str,
        number_int: Option<i32>,
    ) -> card::Model {
        card::Model {
            external_id: format!("ext-{set_code}-{collector_number}"),
            name: format!("Card {collector_number}"),
            set_code: set_code.into(),
            set_name: set_code.to_uppercase(),
            collector_number: collector_number.into(),
            collector_number_int: number_int,
            ..card_model(0)
        }
    }

    #[test]
    fn group_into_drops_orders_named_drops_then_other() {
        let table = crate::scryfall::drops::table("mtg", "sld").unwrap();
        // 2658 -> "Wild in Bloom" (drop order 0); 168 -> "Inked"; an unknown
        // collector number falls into the trailing "Other" bucket.
        let rows = vec![
            sld_test_card("sld", "168", Some(168)),
            sld_test_card("sld", "no-such-number", None),
            sld_test_card("sld", "2658", Some(2658)),
        ];
        let buckets = group_into_drops(table, rows, |c| c.collector_number.as_str());
        let titles: Vec<&str> = buckets.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Wild in Bloom", "Inked", "Other"]);
        assert_eq!(buckets[0].slug.as_deref(), Some("wild-in-bloom"));
        assert!(buckets.last().unwrap().slug.is_none());
        assert!(buckets.iter().all(|b| b.cards.len() == 1));
    }

    #[test]
    fn filter_drops_by_title_matches_case_insensitively() {
        let table = crate::scryfall::drops::table("mtg", "sld").unwrap();
        // 2658 -> "Wild in Bloom"; 168 -> "Inked".
        let rows = vec![
            sld_test_card("sld", "2658", Some(2658)),
            sld_test_card("sld", "168", Some(168)),
        ];
        let buckets = group_into_drops(table, rows, |c| c.collector_number.as_str());

        // A case-insensitive substring keeps only the matching drop.
        let matched = filter_drops_by_title(buckets, "BLOOM");
        let titles: Vec<&str> = matched.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Wild in Bloom"]);

        // A non-matching filter drops every bucket; a blank one keeps them all.
        let rows = vec![sld_test_card("sld", "2658", Some(2658))];
        let buckets = group_into_drops(table, rows, |c| c.collector_number.as_str());
        assert!(filter_drops_by_title(buckets, "no-such-drop").is_empty());
        let rows = vec![sld_test_card("sld", "2658", Some(2658))];
        let buckets = group_into_drops(table, rows, |c| c.collector_number.as_str());
        assert_eq!(filter_drops_by_title(buckets, "").len(), 1);
    }

    #[test]
    fn group_into_drops_preserves_card_order_within_a_drop() {
        let table = crate::scryfall::drops::table("mtg", "sld").unwrap();
        // Two cards from the same drop (Wild in Bloom spans 2658..2662) stay in
        // the order they were fetched (the query's collector-number order).
        let rows = vec![
            sld_test_card("sld", "2659", Some(2659)),
            sld_test_card("sld", "2658", Some(2658)),
        ];
        let buckets = group_into_drops(table, rows, |c| c.collector_number.as_str());
        assert_eq!(buckets.len(), 1);
        let cns: Vec<&str> = buckets[0]
            .cards
            .iter()
            .map(|c| c.collector_number.as_str())
            .collect();
        assert_eq!(cns, vec!["2659", "2658"]);
    }

    #[test]
    fn group_into_subtypes_orders_normal_then_treatments() {
        let borderless = card::Model {
            border_color: Some("borderless".into()),
            ..card_model(1)
        };
        let showcase = card::Model {
            frame_effects: Some("showcase".into()),
            ..card_model(2)
        };
        let normal = card_model(3);
        // Input order is scrambled; output is Normal (order 0) first, then the treatments
        // in sub-type order (Borderless=1, Showcase=2). Sub-types no card matches (Extended
        // Art, Full Art) never appear.
        let rows = vec![showcase, borderless, normal];
        let buckets = group_into_subtypes(rows, |c| c);
        let titles: Vec<&str> = buckets.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Normal", "Borderless", "Showcase"]);
        assert_eq!(buckets[0].slug.as_deref(), Some("normal"));
        assert!(buckets.iter().all(|b| b.cards.len() == 1));
    }
}
