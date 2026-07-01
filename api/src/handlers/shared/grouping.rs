//! Secret Lair drop grouping shared by the catalog and collection by-drop views:
//! the generic grouping pass (over bare cards or owned holdings) and the by-drop
//! pagination tail both endpoints run once their rows are grouped into buckets.

use super::pagination::Page;
use crate::scryfall::drops::DropTable;

/// A drop's items, before pagination/serialization (so off-page drops never get
/// turned into response DTOs). Generic over the item type `T`: the public catalog
/// groups bare `card::Model`s, while the per-user collection groups owned
/// `(collection_item, card)` pairs — both share this one grouping pass.
pub(crate) struct DropBucket<T> {
    pub(crate) slug: Option<String>,
    pub(crate) title: String,
    pub(crate) cards: Vec<T>,
}

/// Group a set's items — already in collector-number order — into Secret Lair
/// drops, preserving Scryfall's drop order. `collector_number` extracts each item's
/// collector number (the drop-table key), so the same grouping works for bare cards
/// and for owned holdings. Items the snapshot doesn't place in a drop collect into a
/// trailing "Other" bucket. Empty drops never appear: a bucket exists only once an
/// item lands in it (so a search that matches a subset yields only the drops with
/// matches).
pub(crate) fn group_into_drops<T>(
    table: &DropTable,
    rows: Vec<T>,
    collector_number: impl Fn(&T) -> &str,
) -> Vec<DropBucket<T>> {
    use std::collections::BTreeMap;
    // Sentinel order for the "Other" bucket: `BTreeMap` ordering parks it last.
    const OTHER: usize = usize::MAX;

    let mut buckets: BTreeMap<usize, DropBucket<T>> = BTreeMap::new();
    for row in rows {
        let (order, slug, title) = match table.drop_for(collector_number(&row)) {
            Some(drop) => (drop.order, Some(drop.slug.clone()), drop.title.clone()),
            None => (OTHER, None, "Other".to_string()),
        };
        buckets
            .entry(order)
            .or_insert_with(|| DropBucket {
                slug,
                title,
                cards: Vec::new(),
            })
            .cards
            .push(row);
    }
    buckets.into_values().collect()
}

/// Paginate already-grouped drop buckets by *drop* (not by card), mapping each
/// on-page bucket into its response shape with `map`. The by-drop endpoints pull a
/// whole (bounded) set, group it in memory, then page over the drops — this is the
/// shared tail of that: off-page buckets are skipped before `map` runs, so their
/// cards are never turned into DTOs.
pub(crate) fn paginate_buckets<T, R>(
    buckets: Vec<DropBucket<T>>,
    page: u64,
    page_size: u64,
    map: impl Fn(DropBucket<T>) -> R,
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
    use sea_orm::prelude::DateTimeUtc;

    use crate::entities::card;

    /// A minimal card row; only its set code and collector number matter to the
    /// drop grouping.
    fn sld_test_card(
        set_code: &str,
        collector_number: &str,
        number_int: Option<i32>,
    ) -> card::Model {
        let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        card::Model {
            id: 0,
            game: "mtg".into(),
            external_id: format!("ext-{set_code}-{collector_number}"),
            oracle_id: None,
            name: format!("Card {collector_number}"),
            set_code: set_code.into(),
            set_name: set_code.to_uppercase(),
            collector_number: collector_number.into(),
            collector_number_int: number_int,
            rarity: None,
            lang: "en".into(),
            released_at: None,
            mana_cost: None,
            cmc: None,
            type_line: None,
            color_identity: None,
            colors: None,
            layout: None,
            oracle_text: None,
            power: None,
            toughness: None,
            loyalty: None,
            image_small: None,
            image_normal: None,
            image_large: None,
            image_art_crop: None,
            image_png: None,
            card_faces: None,
            price_usd: None,
            price_usd_foil: None,
            price_eur: None,
            price_tix: None,
            digital: false,
            created_at: ts,
            updated_at: ts,
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
}
