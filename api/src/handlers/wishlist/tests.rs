//! Unit tests for the wish-list module's entity-coupled queries — the mirrors of the
//! collection's `wishlist_query`/`summary` tests, run against `wishlist_items`. The
//! shared params/DTOs/aggregation helpers (`ListParams`, `dedupe_ids`,
//! `validate_quantity`, `build_collection_sets`, …) are exercised once from
//! `handlers::collection::tests` — both features drive the exact same
//! `handlers::shared::holdings` code, so those tests aren't duplicated here.

use super::products::{product_summary, wanted_products_query};
use super::read::{summary, wishlist_query};

use crate::catalog;
use crate::db::Dialect;
use crate::entities::{card, product, wishlist_item, wishlist_product_item};
use crate::handlers::shared::{
    BULK_THRESHOLD_CENTS, CollectionSort, SortDir, SortField, group_into_drops, search_condition,
};
use sea_orm::{ActiveModelTrait, Condition, Set};

/// A minimal `mtg` card row: only the fields the wish-list search/sort tests
/// exercise (name, type line, USD price) are meaningful; the rest are defaulted.
fn seed_card(id: i32, name: &str, type_line: &str, price_usd: Option<&str>) -> card::Model {
    card::Model {
        name: name.into(),
        type_line: Some(type_line.into()),
        price_usd: price_usd.map(str::to_string),
        ..crate::test_support::card_model(id)
    }
}

/// The joined wish-list query scopes to the signed-in user, filters with the
/// shared Scryfall search over card columns, and orders by the chosen sort —
/// exercising the whole `wishlist_query` path against a real (in-memory) DB.
#[tokio::test]
async fn wishlist_query_scopes_by_user_and_applies_search_and_sort() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let mtg = catalog::find("mtg").expect("mtg game");
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    // Two users; user 2 exists only to prove their wish-list rows never leak into
    // user 1's list (the FK on wishlist_items.user_id needs the rows present).
    for uid in [1, 2] {
        crate::entities::user::ActiveModel {
            id: Set(uid),
            email: Set(format!("u{uid}@example.test")),
            password_hash: Set(Some("x".into())),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
            email_verified_at: Set(None),
            session_version: Set(0),
            username: Set(None),
            discriminator: Set(None),
            currency: Set("USD".into()),
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
        c.into_active_model()
            .insert(&db)
            .await
            .expect("insert card");
    }

    // User 1 wants cards 1..=3 (updated at increasing times so recency order is
    // 3, 2, 1); user 2 wants card 4.
    let want = |id: i32, card_id: i32, user_id: i32, updated: &str| wishlist_item::ActiveModel {
        id: Set(id),
        user_id: Set(user_id),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(1),
        foil_quantity: Set(0),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at(updated)),
    };
    for w in [
        want(1, 1, 1, "2024-01-01T00:00:00Z"),
        want(2, 2, 1, "2024-02-01T00:00:00Z"),
        want(3, 3, 1, "2024-03-01T00:00:00Z"),
        want(4, 4, 2, "2024-04-01T00:00:00Z"),
    ] {
        w.insert(&db).await.expect("insert wishlist row");
    }

    async fn names(
        db: &sea_orm::DatabaseConnection,
        set_codes: Option<&[String]>,
        search: Option<Condition>,
        sort: CollectionSort,
        dir: SortDir,
    ) -> Vec<String> {
        wishlist_query(1, "mtg", set_codes, search, sort, dir, Dialect::Sqlite)
            .all(db)
            .await
            .expect("run wishlist query")
            .into_iter()
            .filter_map(|(_, card)| card.map(|c| c.name))
            .collect()
    }

    // Default recency (updated desc): newest row first, user 2's card absent.
    assert_eq!(
        names(&db, None, None, CollectionSort::Recent, SortDir::Desc).await,
        ["Goblin King", "Forest", "Goblin Guide"]
    );

    // The shared Scryfall grammar runs over the joined card columns: `t:goblin`
    // keeps only user 1's two Goblins (Forest dropped; user 2's Goblin out of scope).
    let goblins = search_condition(mtg, "t:goblin", Dialect::Sqlite).unwrap();
    assert_eq!(
        names(
            &db,
            None,
            Some(goblins),
            CollectionSort::Recent,
            SortDir::Desc
        )
        .await,
        ["Goblin King", "Goblin Guide"]
    );

    // Price sort borrows the catalog card sort verbatim: 5.00, 2.00, 0.10.
    assert_eq!(
        names(
            &db,
            None,
            None,
            CollectionSort::Card(SortField::Price),
            SortDir::Desc
        )
        .await,
        ["Goblin Guide", "Goblin King", "Forest"]
    );
}

/// The `quantity` sort orders wish-list rows by their **total copies** wanted (regular +
/// foil), most first (or fewest, reversed) — the wish-list side of the holdings-only sort
/// (issue #228). Mirrors the collection's `collection_query_orders_by_total_copies` since
/// `wishlist_query` applies the sort against its own entity.
#[tokio::test]
async fn wishlist_query_orders_by_total_copies() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    for c in [
        seed_card(1, "Two Total", "Creature", None),
        seed_card(2, "Five Total", "Creature", None),
        seed_card(3, "Three Total", "Creature", None),
    ] {
        c.into_active_model()
            .insert(&db)
            .await
            .expect("insert card");
    }

    // Total copies = quantity + foil_quantity: card 1 = 2 (2+0), card 2 = 5 (1+4),
    // card 3 = 3 (0+3). The regular-only counts (2, 1, 0) rank differently, so ordering
    // by the total proves the foils are folded in.
    let want = |id: i32, card_id: i32, q: i32, f: i32| wishlist_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    for w in [want(1, 1, 2, 0), want(2, 2, 1, 4), want(3, 3, 0, 3)] {
        w.insert(&db).await.expect("insert wishlist row");
    }

    async fn ordered(db: &sea_orm::DatabaseConnection, dir: SortDir) -> Vec<String> {
        wishlist_query(
            1,
            "mtg",
            None,
            None,
            CollectionSort::Quantity,
            dir,
            Dialect::Sqlite,
        )
        .all(db)
        .await
        .expect("run wishlist query")
        .into_iter()
        .filter_map(|(_, card)| card.map(|c| c.name))
        .collect()
    }

    // Most copies first: 5, 3, 2.
    assert_eq!(
        ordered(&db, SortDir::Desc).await,
        ["Five Total", "Three Total", "Two Total"]
    );
    // Fewest first reverses it.
    assert_eq!(
        ordered(&db, SortDir::Asc).await,
        ["Two Total", "Three Total", "Five Total"]
    );
}

/// `summary` left-joins each wish-list row to its card, so a row whose card is gone
/// (a catalog re-import dropped it) is excluded from **all three** stats — matching
/// the wish-list list, via the same shared aggregation core the collection uses.
#[tokio::test]
async fn summary_skips_rows_whose_card_row_is_missing() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // Two real, priced cards; the third row below points at a card_id with no card
    // row (an orphan — wishlist_items has no FK on card_id).
    for c in [
        seed_card(1, "Priced Card", "Creature", Some("5.00")),
        seed_card(2, "Cheap Card", "Creature", Some("2.00")),
    ] {
        c.into_active_model()
            .insert(&db)
            .await
            .expect("insert card");
    }

    let want = |id: i32, card_id: i32, q: i32, f: i32| wishlist_item::ActiveModel {
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
    for w in [want(1, 1, 2, 1), want(2, 2, 1, 0), want(3, 999, 5, 0)] {
        w.insert(&db).await.expect("insert wishlist row");
    }

    let s = summary(&db, 1, "mtg", None, BULK_THRESHOLD_CENTS)
        .await
        .expect("summary");
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
async fn wishlist_query_scopes_to_a_set() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let mtg = catalog::find("mtg").expect("mtg game");
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
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
        c.into_active_model()
            .insert(&db)
            .await
            .expect("insert card");
    }
    let want = |id: i32, card_id: i32| wishlist_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(1),
        foil_quantity: Set(0),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    for w in [want(1, 1), want(2, 2), want(3, 3), want(4, 4)] {
        w.insert(&db).await.expect("insert wishlist row");
    }

    async fn names(
        db: &sea_orm::DatabaseConnection,
        set_codes: Option<&[String]>,
        search: Option<Condition>,
    ) -> Vec<String> {
        let mut out: Vec<String> = wishlist_query(
            1,
            "mtg",
            set_codes,
            search,
            CollectionSort::Card(SortField::Name),
            SortDir::Asc,
            Dialect::Sqlite,
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
    assert_eq!(
        names(&db, Some(&aaa), None).await,
        ["Forest", "Goblin Guide"]
    );
    // Set scope ANDs with the search: goblins in "aaa" only.
    let goblins = search_condition(mtg, "t:goblin", Dialect::Sqlite).unwrap();
    assert_eq!(
        names(&db, Some(&aaa), Some(goblins)).await,
        ["Goblin Guide"]
    );
    // A multi-code scope (the include-related group view) spans exactly its sets —
    // "aaa" + "bbb", excluding "ccc"'s Goblin Piker.
    let group = ["aaa".to_string(), "bbb".to_string()];
    assert_eq!(
        names(&db, Some(&group), None).await,
        ["Forest", "Goblin Guide", "Goblin King"]
    );
    // No scope: every set's rows, including "ccc".
    assert_eq!(
        names(&db, None, None).await,
        ["Forest", "Goblin Guide", "Goblin King", "Goblin Piker"]
    );
}

/// The drops handler's core: wanted cards in a drop-grouped set, joined + ordered by
/// `wishlist_query`, group into their Secret Lair drops with their wanted counts
/// intact — and a drop the user wants nothing in never appears.
#[tokio::test]
async fn wanted_cards_group_into_drops_with_counts() {
    use sea_orm::{IntoActiveModel, prelude::DateTimeUtc};

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("d@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // sld cards at known Secret Lair collector numbers: 2658 -> "Wild in Bloom",
    // 168 -> "Inked", and one number not in the snapshot (which would fall into the
    // trailing "Other" group — but only if wanted).
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
        c.into_active_model()
            .insert(&db)
            .await
            .expect("insert card");
    }
    // Want the first two (2 + 1 foil of #2658; 3 of #168); leave #999999 unwanted.
    let want = |id: i32, card_id: i32, q: i32, f: i32| wishlist_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        card_id: Set(card_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    for w in [want(1, 1, 2, 1), want(2, 2, 3, 0)] {
        w.insert(&db).await.expect("insert wishlist row");
    }

    // The same query + grouping pass the drops handler runs.
    let scope = ["sld".to_string()];
    let rows = wishlist_query(
        1,
        "mtg",
        Some(&scope),
        None,
        CollectionSort::Card(SortField::Number),
        SortDir::Asc,
        Dialect::Sqlite,
    )
    .all(&db)
    .await
    .expect("run query");
    let pairs: Vec<(wishlist_item::Model, card::Model)> = rows
        .into_iter()
        .filter_map(|(item, card)| card.map(|c| (item, c)))
        .collect();
    let table = crate::scryfall::drops::table("mtg", "sld").expect("sld drop table");
    let buckets = group_into_drops(table, pairs, |(_, card)| card.collector_number.as_str());

    // Only the two wanted drops appear (the unwanted #999999 yields no "Other" group),
    // in Scryfall's drop order (Wild in Bloom before Inked), each carrying its wanted
    // counts.
    let titles: Vec<&str> = buckets.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, vec!["Wild in Bloom", "Inked"]);
    assert_eq!(buckets[0].cards.len(), 1);
    assert_eq!(buckets[0].cards[0].0.quantity, 2);
    assert_eq!(buckets[0].cards[0].0.foil_quantity, 1);
    assert_eq!(buckets[1].cards[0].0.quantity, 3);
}

/// The wanted-products base query scopes to the signed-in user and orders by recency
/// (newest change first), left-joining each row to its product — the sealed-product
/// counterpart of `wishlist_query`, run against `wishlist_product_items` (issue #364).
#[tokio::test]
async fn wanted_products_query_scopes_by_user_and_sorts_by_recency() {
    use sea_orm::prelude::DateTimeUtc;

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    // Two users; user 2 exists only to prove their rows never leak into user 1's list
    // (the FK on wishlist_product_items.user_id needs the rows present).
    for uid in [1, 2] {
        crate::entities::user::ActiveModel {
            id: Set(uid),
            email: Set(format!("u{uid}@example.test")),
            password_hash: Set(Some("x".into())),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at("2024-01-01T00:00:00Z")),
            email_verified_at: Set(None),
            session_version: Set(0),
            username: Set(None),
            discriminator: Set(None),
            currency: Set("USD".into()),
        }
        .insert(&db)
        .await
        .expect("insert user");
    }

    // Three products (internal ids captured); user 1 wants two, user 2 wants one.
    let p100 = crate::test_support::insert_product(
        &db,
        "100",
        "Booster Box",
        "aaa",
        "collector_display",
        Some("120.00"),
    )
    .await;
    let p200 = crate::test_support::insert_product(
        &db,
        "200",
        "Gift Bundle",
        "aaa",
        "bundle",
        Some("40.00"),
    )
    .await;
    let p300 = crate::test_support::insert_product(
        &db,
        "300",
        "Commander Deck",
        "bbb",
        "commander_deck",
        Some("25.00"),
    )
    .await;

    let want = |id: i32, product_id: i32, user_id: i32, updated: &str| {
        wishlist_product_item::ActiveModel {
            id: Set(id),
            user_id: Set(user_id),
            game: Set("mtg".into()),
            product_id: Set(product_id),
            quantity: Set(1),
            foil_quantity: Set(0),
            created_at: Set(at("2024-01-01T00:00:00Z")),
            updated_at: Set(at(updated)),
        }
    };
    // User 1 wants p100 (row id 1, but the *newer* updated_at) and p200 (row id 2, but
    // the *older* updated_at) — the higher row id deliberately gets the older timestamp,
    // so a query that (wrongly) sorted by id/insertion order instead of `updated_at`
    // would return the opposite order and fail the assertion below. User 2 wants p300.
    for w in [
        want(1, p100, 1, "2024-03-01T00:00:00Z"),
        want(2, p200, 1, "2024-02-01T00:00:00Z"),
        want(3, p300, 2, "2024-04-01T00:00:00Z"),
    ] {
        w.insert(&db).await.expect("insert wishlist product row");
    }

    let rows = wanted_products_query(1, "mtg")
        .all(&db)
        .await
        .expect("run wanted products query");

    // Exactly user 1's two rows, newest updated_at first (p100 before p200 despite its
    // lower row id), each joined to its product by external id — user 2's p300 is absent.
    let external_ids: Vec<String> = rows
        .iter()
        .map(|(_, prod)| prod.as_ref().expect("product present").external_id.clone())
        .collect();
    assert_eq!(external_ids, ["100", "200"]);
    // The wanted counts ride along on the item side of the join.
    assert_eq!(rows[0].0.quantity, 1);
}

/// A wanted-product row whose product row is missing (an orphan — the table has no FK on
/// `product_id`, mirroring `wishlist_items.card_id`) comes back from the LEFT join as
/// `(item, None)`, so the handler's `filter_map` drops it; a valid row keeps `Some(product)`.
#[tokio::test]
async fn wanted_products_query_returns_none_for_orphaned_products() {
    use sea_orm::prelude::DateTimeUtc;

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    let valid = crate::test_support::insert_product(
        &db,
        "100",
        "Booster Box",
        "aaa",
        "collector_display",
        Some("120.00"),
    )
    .await;

    let want = |id: i32, product_id: i32, updated: &str| wishlist_product_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        product_id: Set(product_id),
        quantity: Set(1),
        foil_quantity: Set(0),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at(updated)),
    };
    // A valid row (newest) plus an orphan pointing at product_id 9999 (no product row).
    for w in [
        want(1, valid, "2024-02-01T00:00:00Z"),
        want(2, 9999, "2024-01-15T00:00:00Z"),
    ] {
        w.insert(&db).await.expect("insert wishlist product row");
    }

    let rows = wanted_products_query(1, "mtg")
        .all(&db)
        .await
        .expect("run wanted products query");
    assert_eq!(rows.len(), 2);
    // Recency order: the valid row (newer) first with its product, the orphan second as
    // `None` — exactly what the handler's `filter_map` skips.
    assert_eq!(
        rows[0].1.as_ref().map(|p| p.external_id.clone()),
        Some("100".to_string())
    );
    assert!(rows[1].1.is_none());
    assert_eq!(rows[1].0.product_id, 9999);
}

/// `product_summary` counts distinct wanted products and total copies (regular + foil),
/// values regular copies at the market `usd` and foils at `usd_foil`, skips an orphaned
/// product (no product row) entirely for all three stats, and lets an unpriced product
/// contribute nothing to the value — the sealed-product counterpart of
/// `summary_skips_rows_whose_card_row_is_missing`.
#[tokio::test]
async fn product_summary_counts_values_and_skips_orphans() {
    use sea_orm::prelude::DateTimeUtc;

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // A booster box (regular only), a bundle with both a regular and a foil price, and an
    // unpriced deck. `insert_product` always leaves `price_usd_foil` None, so set the
    // bundle's foil price directly.
    let a = crate::test_support::insert_product(
        &db,
        "100",
        "Booster Box",
        "aaa",
        "collector_display",
        Some("120.00"),
    )
    .await;
    let b =
        crate::test_support::insert_product(&db, "200", "Bundle", "aaa", "bundle", Some("40.00"))
            .await;
    product::ActiveModel {
        id: Set(b),
        price_usd_foil: Set(Some("50.00".into())),
        ..Default::default()
    }
    .update(&db)
    .await
    .expect("set foil price");
    let c = crate::test_support::insert_product(&db, "300", "Deck", "aaa", "deck", None).await;

    let want = |id: i32, product_id: i32, q: i32, f: i32| wishlist_product_item::ActiveModel {
        id: Set(id),
        user_id: Set(1),
        game: Set("mtg".into()),
        product_id: Set(product_id),
        quantity: Set(q),
        foil_quantity: Set(f),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    };
    // a: 2 regular; b: 1 regular + 2 foil; c: 1 regular (unpriced); plus an orphan
    // (product_id 9999, 5 copies) with no product row.
    for w in [
        want(1, a, 2, 0),
        want(2, b, 1, 2),
        want(3, c, 1, 0),
        want(4, 9999, 5, 0),
    ] {
        w.insert(&db).await.expect("insert wishlist product row");
    }

    let s = product_summary(&db, 1, "mtg")
        .await
        .expect("product summary");
    // The orphan is skipped for all three stats: 3 distinct products, 6 copies
    // (2 + 3 + 1; the orphan's 5 excluded), value = 2×$120 + 1×$40 + 2×$50 = $380.00
    // (the unpriced deck contributes nothing).
    assert_eq!(s.unique_products, 3);
    assert_eq!(s.total_products, 6);
    assert_eq!(s.total_value_usd.as_deref(), Some("380.00"));
}

/// An all-unpriced wanted set reports a `null` value (not `"0.00"`), and a product's
/// curated MSRP is never folded into the cost — only the market `usd`/`usd_foil` prices
/// count. Pins both the null-when-unpriced contract and msrp independence.
#[tokio::test]
async fn product_summary_value_is_null_when_nothing_priced_and_ignores_msrp() {
    use sea_orm::prelude::DateTimeUtc;

    let db = crate::test_support::migrated_memory_db().await;
    let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

    crate::entities::user::ActiveModel {
        id: Set(1),
        email: Set("u1@example.test".into()),
        password_hash: Set(Some("x".into())),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
        email_verified_at: Set(None),
        session_version: Set(0),
        username: Set(None),
        discriminator: Set(None),
        currency: Set("USD".into()),
    }
    .insert(&db)
    .await
    .expect("insert user");

    // An unpriced product that nonetheless carries a curated MSRP — the value must stay
    // null (msrp is retail metadata, never a market price).
    let p = crate::test_support::insert_product(&db, "100", "Deck", "aaa", "deck", None).await;
    crate::test_support::set_product_msrp(&db, "100", "59.99").await;

    wishlist_product_item::ActiveModel {
        id: Set(1),
        user_id: Set(1),
        game: Set("mtg".into()),
        product_id: Set(p),
        quantity: Set(3),
        foil_quantity: Set(0),
        created_at: Set(at("2024-01-01T00:00:00Z")),
        updated_at: Set(at("2024-01-01T00:00:00Z")),
    }
    .insert(&db)
    .await
    .expect("insert wishlist product row");

    let s = product_summary(&db, 1, "mtg")
        .await
        .expect("product summary");
    assert_eq!(s.unique_products, 1);
    assert_eq!(s.total_products, 3);
    assert_eq!(s.total_value_usd, None);
}
