use super::*;
use super::import::parse_reconcile_mode;
use super::read::{collection_query, summary};

use crate::catalog;
use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::{card, card_set};
use crate::handlers::shared::{
    DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, SortDir, SortField, build_collection_sets, dedupe_ids,
    group_into_drops, search_condition, validate_quantity,
};
use sea_orm::{ActiveModelTrait, Condition, Set};

/// Build a `ListParams` with only paging set — the search/sort tests override
/// `q`/`sort`/`dir` via struct-update on top of this.
fn params(page: Option<u64>, page_size: Option<u64>) -> ListParams {
    ListParams {
        page,
        page_size,
        q: None,
        sort: None,
        dir: None,
        set: None,
        include_related: None,
    }
}

#[test]
fn page_and_size_defaults_and_clamps() {
    assert_eq!(params(None, None).page_and_size(), (1, DEFAULT_PAGE_SIZE));
    assert_eq!(params(Some(0), Some(9999)).page_and_size(), (1, MAX_PAGE_SIZE));
    assert_eq!(params(Some(3), Some(20)).page_and_size(), (3, 20));
}

#[test]
fn search_trims_and_blank_filters() {
    assert_eq!(
        ListParams {
            q: Some("  goblin ".into()),
            ..params(None, None)
        }
        .search(),
        Some("goblin")
    );
    assert_eq!(
        ListParams {
            q: Some("   ".into()),
            ..params(None, None)
        }
        .search(),
        None
    );
    assert_eq!(params(None, None).search(), None);
}

#[test]
fn sort_spec_defaults_to_recent_and_reuses_card_sorts() {
    // Absent, and the explicit recency keys, all resolve to newest-first.
    for sort in [None, Some("updated"), Some("recent")] {
        assert_eq!(
            ListParams {
                sort: sort.map(str::to_string),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Recent, SortDir::Desc)
        );
    }
    // A reversed recency (oldest first).
    assert_eq!(
        ListParams {
            sort: Some("updated".into()),
            dir: Some("asc".into()),
            ..params(None, None)
        }
        .sort_spec()
        .unwrap(),
        (CollectionSort::Recent, SortDir::Asc)
    );
    // Card sorts borrow the catalog field + its natural direction.
    assert_eq!(
        ListParams {
            sort: Some("name".into()),
            ..params(None, None)
        }
        .sort_spec()
        .unwrap(),
        (CollectionSort::Card(SortField::Name), SortDir::Asc)
    );
    assert_eq!(
        ListParams {
            sort: Some("price".into()),
            ..params(None, None)
        }
        .sort_spec()
        .unwrap(),
        (CollectionSort::Card(SortField::Price), SortDir::Desc)
    );
}

#[test]
fn sort_spec_rejects_unknown_values() {
    assert!(matches!(
        ListParams {
            sort: Some("nonsense".into()),
            ..params(None, None)
        }
        .sort_spec(),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        ListParams {
            dir: Some("sideways".into()),
            ..params(None, None)
        }
        .sort_spec(),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn dedupe_ids_trims_dedupes_and_drops_blanks() {
    let out = dedupe_ids(vec![
        "  a ".into(),
        "b".into(),
        "a".into(),
        "".into(),
        "   ".into(),
        "b".into(),
        "c".into(),
    ]);
    // First-seen order preserved, blanks gone, no repeats.
    assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

#[test]
fn parse_reconcile_mode_accepts_known_modes_and_rejects_others() {
    assert!(matches!(
        parse_reconcile_mode(Some("overwrite")),
        Ok(ReconcileMode::Overwrite)
    ));
    assert!(matches!(
        parse_reconcile_mode(Some(" replace ")),
        Ok(ReconcileMode::Replace)
    ));
    assert!(matches!(
        parse_reconcile_mode(Some("merge")),
        Ok(ReconcileMode::Merge)
    ));
    // Missing or unrecognised -> our JSON validation error (422), never a silent default.
    assert!(matches!(
        parse_reconcile_mode(None),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        parse_reconcile_mode(Some("wipe")),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn validate_quantity_bounds() {
    assert_eq!(validate_quantity(0, "quantity").unwrap(), 0);
    assert_eq!(validate_quantity(5, "quantity").unwrap(), 5);
    assert!(matches!(
        validate_quantity(-1, "quantity"),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        validate_quantity(MAX_CARD_QUANTITY + 1, "foil_quantity"),
        Err(AppError::Validation(_))
    ));
}

/// A minimal `mtg` card row: only the fields the collection search/sort tests
/// exercise (name, type line, USD price) are meaningful; the rest are defaulted.
fn seed_card(id: i32, name: &str, type_line: &str, price_usd: Option<&str>) -> card::Model {
    card::Model {
        name: name.into(),
        type_line: Some(type_line.into()),
        price_usd: price_usd.map(str::to_string),
        ..crate::test_support::card_model(id)
    }
}

/// The joined collection query scopes to the signed-in user, filters with the
/// shared Scryfall search over card columns, and orders by the chosen sort —
/// exercising the whole `collection_query` path against a real (in-memory) DB.
#[tokio::test]
async fn collection_query_scopes_by_user_and_applies_search_and_sort() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let mtg = catalog::find("mtg").expect("mtg game");
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    // Two users; user 2 exists only to prove their holdings never leak into
    // user 1's list (the FK on collection_items.user_id needs the rows present).
    for uid in [1, 2] {
        crate::entities::user::ActiveModel {
            id: Set(uid),
            email: Set(format!("u{uid}@example.test")),
            password_hash: Set(Some("x".into())),
            display_name: Set(None),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
            email_verified_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("insert user");
    }

    for c in [
        seed_card(1, "Goblin Guide", "Creature — Goblin", Some("5.00")),
        seed_card(2, "Forest", "Basic Land — Forest", Some("0.10")),
        seed_card(3, "Goblin King", "Creature — Goblin", Some("2.00")),
        seed_card(4, "Goblin Piker", "Creature — Goblin", Some("1.00")),
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }

    // User 1 owns cards 1..=3 (updated at increasing times so recency order is
    // 3, 2, 1); user 2 owns card 4.
    let hold = |id: i32, card_id: i32, user_id: i32, updated: &str| {
        collection_item::ActiveModel {
            id: Set(id),
            user_id: Set(user_id),
            game: Set("mtg".into()),
            card_id: Set(card_id),
            quantity: Set(1),
            foil_quantity: Set(0),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at(updated)),
        }
    };
    for h in [
        hold(1, 1, 1, "2024-01-01T00:00:00Z"),
        hold(2, 2, 1, "2024-02-01T00:00:00Z"),
        hold(3, 3, 1, "2024-03-01T00:00:00Z"),
        hold(4, 4, 2, "2024-04-01T00:00:00Z"),
    ] {
        h.insert(&db).await.expect("insert holding");
    }

    async fn names(
        db: &sea_orm::DatabaseConnection,
        set_codes: Option<&[String]>,
        search: Option<Condition>,
        sort: CollectionSort,
        dir: SortDir,
    ) -> Vec<String> {
        collection_query(1, "mtg", set_codes, search, sort, dir)
            .all(db)
            .await
            .expect("run collection query")
            .into_iter()
            .filter_map(|(_, card)| card.map(|c| c.name))
            .collect()
    }

    // Default recency (updated desc): newest holding first, user 2's card absent.
    assert_eq!(
        names(&db, None, None, CollectionSort::Recent, SortDir::Desc).await,
        ["Goblin King", "Forest", "Goblin Guide"]
    );

    // The shared Scryfall grammar runs over the joined card columns: `t:goblin`
    // keeps only user 1's two Goblins (Forest dropped; user 2's Goblin out of scope).
    let goblins = search_condition(mtg, "t:goblin").unwrap();
    assert_eq!(
        names(&db, None, Some(goblins), CollectionSort::Recent, SortDir::Desc).await,
        ["Goblin King", "Goblin Guide"]
    );

    // Price sort borrows the catalog card sort verbatim: 5.00, 2.00, 0.10.
    assert_eq!(
        names(&db, None, None, CollectionSort::Card(SortField::Price), SortDir::Desc).await,
        ["Goblin Guide", "Goblin King", "Forest"]
    );
}

/// `summary` left-joins each holding to its card, so a holding whose card row is gone
/// (a catalog re-import dropped it) is excluded from **all three** stats — matching the
/// collection list. Pins the unified behavior (the old whole-collection path counted
/// such orphans toward unique/total cards).
#[tokio::test]
async fn summary_skips_holdings_whose_card_row_is_missing() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        display_name: Set(None),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // Two real, priced cards; the third holding below points at a card_id with no card
    // row (an orphan — collection_items has no FK on card_id).
    for c in [
        seed_card(1, "Priced Card", "Creature", Some("5.00")),
        seed_card(2, "Cheap Card", "Creature", Some("2.00")),
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }

    let hold = |id: i32, card_id: i32, q: i32, f: i32| collection_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    // card 1: 2 regular + 1 foil; card 2: 1 regular; card 999: orphan (no card row).
    for h in [hold(1, 1, 2, 1), hold(2, 2, 1, 0), hold(3, 999, 5, 0)] {
        h.insert(&db).await.expect("insert holding");
    }

    let s = summary(&db, 1, "mtg", None).await.expect("summary");
    // The orphan (card 999, 5 copies) is skipped entirely: 2 distinct cards, 4 copies
    // (2 + 1 + 1), value = 2×$5.00 (card 1's foil is unpriced) + 1×$2.00 = $12.00.
    assert_eq!(s.unique_cards, 2);
    assert_eq!(s.total_cards, 4);
    assert_eq!(s.total_value_usd.as_deref(), Some("12.00"));
    // Both priced cards are $2+, so the bulk (< $1) portion is a meaningful $0.00, not null.
    assert_eq!(s.bulk_value_usd.as_deref(), Some("0.00"));
}

/// The optional set scope filters the joined card's `set_code`, and it ANDs with a
/// search over the same join — so a set-scoped, `t:goblin`-filtered list only keeps
/// the goblins in that set.
#[tokio::test]
async fn collection_query_scopes_to_a_set() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let mtg = catalog::find("mtg").expect("mtg game");
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        display_name: Set(None),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // Three sets: "aaa" holds a Goblin + a Land; "bbb" and "ccc" each hold a Goblin.
    // The third set lets the multi-code (group-span) scope prove it *excludes* the
    // sets outside its list, not just that it returns what it's given.
    let card = |id: i32, name: &str, set_code: &str, type_line: &str| {
        let mut c = seed_card(id, name, type_line, Some("1.00"));
        c.set_code = set_code.into();
        c.set_name = set_code.to_uppercase();
        c
    };
    for c in [
        card(1, "Goblin Guide", "aaa", "Creature — Goblin"),
        card(2, "Forest", "aaa", "Basic Land — Forest"),
        card(3, "Goblin King", "bbb", "Creature — Goblin"),
        card(4, "Goblin Piker", "ccc", "Creature — Goblin"),
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }
    let hold = |id: i32, card_id: i32| collection_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(1),
        foil_quantity: Set(0),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    for h in [hold(1, 1), hold(2, 2), hold(3, 3), hold(4, 4)] {
        h.insert(&db).await.expect("insert holding");
    }

    async fn names(
        db: &sea_orm::DatabaseConnection,
        set_codes: Option<&[String]>,
        search: Option<Condition>,
    ) -> Vec<String> {
        let mut out: Vec<String> = collection_query(
            1,
            "mtg",
            set_codes,
            search,
            CollectionSort::Card(SortField::Name),
            SortDir::Asc,
        )
        .all(db)
        .await
        .expect("run query")
        .into_iter()
        .filter_map(|(_, c)| c.map(|c| c.name))
        .collect();
        out.sort();
        out
    }

    let aaa = ["aaa".to_string()];
    // Scoped to set "aaa": only its two cards, not the other sets' Goblins.
    assert_eq!(names(&db, Some(&aaa), None).await, ["Forest", "Goblin Guide"]);
    // Set scope ANDs with the search: goblins in "aaa" only.
    let goblins = search_condition(mtg, "t:goblin").unwrap();
    assert_eq!(names(&db, Some(&aaa), Some(goblins)).await, ["Goblin Guide"]);
    // A multi-code scope (the include-related group view) spans exactly its sets —
    // "aaa" + "bbb", excluding "ccc"'s Goblin Piker.
    let group = ["aaa".to_string(), "bbb".to_string()];
    assert_eq!(
        names(&db, Some(&group), None).await,
        ["Forest", "Goblin Guide", "Goblin King"]
    );
    // No scope: every set's holdings, including "ccc".
    assert_eq!(
        names(&db, None, None).await,
        ["Forest", "Goblin Guide", "Goblin King", "Goblin Piker"]
    );
}

/// The drops handler's core: owned cards in a drop-grouped set, joined + ordered by
/// `collection_query`, group into their Secret Lair drops with their owned counts
/// intact — and a drop the user owns nothing in never appears.
#[tokio::test]
async fn owned_cards_group_into_drops_with_counts() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("d@example.test".into()),
        password_hash: Set(Some("x".into())),
        display_name: Set(None),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // sld cards at known Secret Lair collector numbers: 2658 -> "Wild in Bloom",
    // 168 -> "Inked", and one number not in the snapshot (which would fall into the
    // trailing "Other" group — but only if owned).
    let sld_card = |id: i32, cn: &str, cn_int: Option<i32>| {
        let mut c = seed_card(id, &format!("SLD {cn}"), "Creature", Some("1.00"));
        c.set_code = "sld".into();
        c.set_name = "Secret Lair Drop".into();
        c.collector_number = cn.into();
        c.collector_number_int = cn_int;
        c
    };
    for c in [
        sld_card(1, "2658", Some(2658)),
        sld_card(2, "168", Some(168)),
        sld_card(3, "999999", Some(999999)),
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }
    // Own the first two (2 + 1 foil of #2658; 3 of #168); leave #999999 unowned.
    let hold = |id: i32, card_id: i32, q: i32, f: i32| collection_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    for h in [hold(1, 1, 2, 1), hold(2, 2, 3, 0)] {
        h.insert(&db).await.expect("insert holding");
    }

    // The same query + grouping pass the drops handler runs.
    let scope = ["sld".to_string()];
    let rows = collection_query(
        1,
        "mtg",
        Some(&scope),
        None,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
    )
    .all(&db)
    .await
    .expect("run query");
    let pairs: Vec<(collection_item::Model, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();
    let table = crate::scryfall::drops::table("mtg", "sld").expect("sld drop table");
    let buckets = group_into_drops(table, pairs, |(_, card)| card.collector_number.as_str());

    // Only the two owned drops appear (the unowned #999999 yields no "Other" group),
    // in Scryfall's drop order (Wild in Bloom before Inked), each carrying its owned
    // holding.
    let titles: Vec<&str> = buckets.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, vec!["Wild in Bloom", "Inked"]);
    assert_eq!(buckets[0].cards.len(), 1);
    assert_eq!(buckets[0].cards[0].0.quantity, 2);
    assert_eq!(buckets[0].cards[0].0.foil_quantity, 1);
    assert_eq!(buckets[1].cards[0].0.quantity, 3);
}

/// `build_collection_sets` counts distinct owned cards + total copies per set,
/// dresses each with its `card_sets` metadata (falling back to the card's own set
/// name when the row is missing), orders newest set first (undated last), and skips
/// holdings whose card row is gone.
#[test]
fn build_collection_sets_aggregates_dresses_and_orders() {
    let ts = "2024-01-01T00:00:00Z"
        .parse::<sea_orm::prelude::DateTimeUtc>()
        .unwrap();
    let hold = |id: i32, card_id: i32, quantity: i32, foil_quantity: i32| collection_item::Model {
        id,
        user_id: 1,
        game: "mtg".into(),
        card_id,
        quantity,
        foil_quantity,
        created_at: ts,
        updated_at: ts,
    };
    let carded = |id: i32, set_code: &str, set_name: &str, usd: Option<&str>, foil: Option<&str>| {
        let mut c = seed_card(id, "Card", "Creature", usd);
        c.set_code = set_code.into();
        c.set_name = set_name.into();
        c.price_usd_foil = foil.map(str::to_string);
        c
    };
    let set_meta = |code: &str, name: &str, released: &str| card_set::Model {
        name: name.into(),
        set_type: Some("expansion".into()),
        released_at: Some(released.into()),
        card_count: 100,
        icon_svg_uri: Some(format!("https://example.test/{code}.svg")),
        ..crate::test_support::card_set_model(code)
    };

    let rows = vec![
        // Set "aaa": two distinct cards, 3 total copies (2 + 1 foil, then 1 + 0).
        // Value: 2×$1.00 + 1×$5.00 foil + 1×$2.00 = $9.00.
        (
            hold(1, 1, 2, 1),
            Some(carded(1, "aaa", "Older Set", Some("1.00"), Some("5.00"))),
        ),
        (
            hold(2, 2, 1, 0),
            Some(carded(2, "aaa", "Older Set", Some("2.00"), None)),
        ),
        // Set "bbb": one card, 4 copies — no card_sets metadata (fallback name) and
        // unpriced, so its value is `None` rather than $0.00.
        (
            hold(3, 3, 4, 0),
            Some(carded(3, "bbb", "Newer Set", None, None)),
        ),
        // A holding whose card row is gone — skipped entirely.
        (hold(4, 4, 9, 9), None),
    ];
    // Only "aaa" has metadata; "bbb" must fall back to the card's set_name.
    let sets = vec![set_meta("aaa", "Alpha", "2000-01-01")];

    let out = build_collection_sets("mtg", rows, sets);
    assert_eq!(out.len(), 2);

    // "bbb" (dated? no metadata -> released_at None) sorts after the dated "aaa".
    assert_eq!(out[0].code, "aaa");
    assert_eq!(out[0].name, "Alpha"); // dressed from card_sets, not the card
    assert_eq!(out[0].released_at.as_deref(), Some("2000-01-01"));
    assert_eq!(out[0].owned_cards, 2);
    assert_eq!(out[0].owned_copies, 4); // (2+1) + (1+0)
    assert_eq!(out[0].owned_value_usd.as_deref(), Some("9.00")); // priced holdings summed
    // Every priced finish in "aaa" is $1+ ($1.00 regular is exactly at the boundary and so
    // excluded, $5.00 foil, $2.00 regular), so the bulk portion is a meaningful $0.00.
    assert_eq!(out[0].owned_bulk_value_usd.as_deref(), Some("0.00"));

    assert_eq!(out[1].code, "bbb");
    assert_eq!(out[1].name, "Newer Set"); // fallback to the card's set_name
    assert_eq!(out[1].released_at, None);
    assert_eq!(out[1].card_count, 0);
    assert_eq!(out[1].owned_cards, 1);
    assert_eq!(out[1].owned_copies, 4);
    assert_eq!(out[1].owned_value_usd, None); // nothing priced -> null, not $0.00
    assert_eq!(out[1].owned_bulk_value_usd, None); // nothing priced -> null bulk too
}

/// The per-set bulk value is the value of just the finishes priced under $1, judged per
/// finish — a card whose regular printing is bulk but whose foil isn't contributes only
/// its regular copies to the bulk figure (and vice-versa).
#[test]
fn build_collection_sets_splits_bulk_per_finish() {
    let ts = "2024-01-01T00:00:00Z"
        .parse::<sea_orm::prelude::DateTimeUtc>()
        .unwrap();
    let hold = |id: i32, card_id: i32, quantity: i32, foil_quantity: i32| collection_item::Model {
        id,
        user_id: 1,
        game: "mtg".into(),
        card_id,
        quantity,
        foil_quantity,
        created_at: ts,
        updated_at: ts,
    };
    let carded = |id: i32, usd: Option<&str>, foil: Option<&str>| {
        let mut c = seed_card(id, "Card", "Creature", usd);
        c.set_code = "aaa".into();
        c.set_name = "Alpha".into();
        c.price_usd_foil = foil.map(str::to_string);
        c
    };

    let rows = vec![
        // Bulk regular ($0.25 ×4 = $1.00), non-bulk foil ($3.00 ×1): only the regulars
        // are bulk.
        (hold(1, 1, 4, 1), Some(carded(1, Some("0.25"), Some("3.00")))),
        // Non-bulk regular ($2.00 ×1), bulk foil ($0.50 ×2 = $1.00): only the foils.
        (hold(2, 2, 1, 2), Some(carded(2, Some("2.00"), Some("0.50")))),
    ];

    let out = build_collection_sets("mtg", rows, vec![]);
    assert_eq!(out.len(), 1);
    // Total = 1.00 + 3.00 + 2.00 + 1.00 = 7.00.
    assert_eq!(out[0].owned_value_usd.as_deref(), Some("7.00"));
    // Bulk = $1.00 (card 1 regulars) + $1.00 (card 2 foils) = $2.00.
    assert_eq!(out[0].owned_bulk_value_usd.as_deref(), Some("2.00"));
}
