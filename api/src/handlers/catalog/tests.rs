use super::*;
use super::image::{is_allowed_image_url, normalize_size};
use super::prices::PricePoint;
use crate::handlers::shared::pricing::{PriceRange, cutoff_date, downsample_rows};
use crate::db::Dialect;
use crate::entities::card_price_history;
use crate::scryfall::search::escape_like;
use chrono::NaiveDate;
use sea_orm::prelude::DateTimeUtc;

#[test]
fn normalize_size_allowlists() {
    assert_eq!(normalize_size(Some("png")), "png");
    assert_eq!(normalize_size(Some("art_crop")), "art_crop");
    assert_eq!(normalize_size(Some("../secret")), "normal");
    assert_eq!(normalize_size(None), "normal");
}

#[test]
fn price_point_maps_history_row() {
    let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    let m = card_price_history::Model {
        id: 1,
        game: "mtg".into(),
        card_id: 5,
        as_of_date: "2026-06-30".into(),
        price_usd: Some("1.23".into()),
        price_usd_foil: None,
        price_eur: Some("1.00".into()),
        price_tix: None,
        created_at: ts,
    };
    let p = PricePoint::from(m);
    assert_eq!(p.date, "2026-06-30");
    assert_eq!(p.usd.as_deref(), Some("1.23"));
    assert_eq!(p.usd_foil, None);
    assert_eq!(p.eur.as_deref(), Some("1.00"));
    assert_eq!(p.tix, None);
}

/// Build a history row for the downsample tests (only the id/date/usd matter).
fn hist(date: &str, usd: &str) -> card_price_history::Model {
    let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    card_price_history::Model {
        id: 0,
        game: "mtg".into(),
        card_id: 1,
        as_of_date: date.into(),
        price_usd: Some(usd.into()),
        price_usd_foil: None,
        price_eur: None,
        price_tix: None,
        created_at: ts,
    }
}

#[test]
fn price_range_parse_accepts_known_and_rejects_unknown() {
    assert_eq!(PriceRange::parse("7d").unwrap(), PriceRange::D7);
    assert_eq!(PriceRange::parse("30d").unwrap(), PriceRange::D30);
    assert_eq!(PriceRange::parse("1y").unwrap(), PriceRange::Y1);
    assert_eq!(PriceRange::parse("2y").unwrap(), PriceRange::Y2);
    assert_eq!(PriceRange::parse("3y").unwrap(), PriceRange::Y3);
    assert_eq!(PriceRange::parse("all").unwrap(), PriceRange::All);
    // Unknown / mis-cased / empty values are a 422, not a silent fallback.
    assert!(matches!(PriceRange::parse("7D"), Err(AppError::Validation(_))));
    assert!(matches!(PriceRange::parse("week"), Err(AppError::Validation(_))));
    assert!(matches!(PriceRange::parse(""), Err(AppError::Validation(_))));
}

#[test]
fn cutoff_date_windows_relative_to_today() {
    let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    // Inclusive cutoff = today - window days (so `>= cutoff` keeps window+1 days).
    // Multi-year cutoffs account for the 2024 leap day (hence the -07-02 edges).
    assert_eq!(cutoff_date(today, PriceRange::D7).as_deref(), Some("2026-06-24"));
    assert_eq!(cutoff_date(today, PriceRange::D30).as_deref(), Some("2026-06-01"));
    assert_eq!(cutoff_date(today, PriceRange::Y1).as_deref(), Some("2025-07-01"));
    assert_eq!(cutoff_date(today, PriceRange::Y2).as_deref(), Some("2024-07-01"));
    assert_eq!(cutoff_date(today, PriceRange::Y3).as_deref(), Some("2023-07-02"));
    // "all" has no lower bound.
    assert_eq!(cutoff_date(today, PriceRange::All), None);
}

/// Downsample the card history rows, then map to points (the handler's pipeline).
fn downsample(rows: Vec<card_price_history::Model>, bucket_days: i64) -> Vec<PricePoint> {
    downsample_rows(rows, bucket_days, |r| r.as_of_date.as_str())
        .into_iter()
        .map(PricePoint::from)
        .collect()
}

#[test]
fn downsample_daily_is_passthrough() {
    let rows = vec![hist("2026-06-01", "1"), hist("2026-06-02", "2"), hist("2026-06-03", "3")];
    let out = downsample(rows, 1);
    let dates: Vec<_> = out.iter().map(|p| p.date.clone()).collect();
    assert_eq!(dates, vec!["2026-06-01", "2026-06-02", "2026-06-03"]);
}

#[test]
fn downsample_empty_input_is_empty() {
    assert!(downsample(Vec::new(), 7).is_empty());
}

#[test]
fn downsample_keeps_last_real_row_per_bucket() {
    // 15 consecutive days, weekly (7-day) buckets.
    let rows: Vec<_> =
        (1..=15).map(|d| hist(&format!("2026-06-{d:02}"), &d.to_string())).collect();
    let out = downsample(rows, 7);
    let dates: Vec<_> = out.iter().map(|p| p.date.clone()).collect();

    // Genuinely coarser than the 15 daily rows, but more than one bucket.
    assert!(out.len() > 1 && out.len() < 15, "got {} points", out.len());
    // The newest day is always retained, with its real (un-averaged) price.
    assert_eq!(dates.last().unwrap(), "2026-06-15");
    assert_eq!(out.last().unwrap().usd.as_deref(), Some("15"));
    // One representative per bucket -> strictly increasing real input dates.
    for w in dates.windows(2) {
        assert!(w[0] < w[1], "dates must be strictly increasing: {dates:?}");
    }
    let inputs: Vec<String> = (1..=15).map(|d| format!("2026-06-{d:02}")).collect();
    assert!(dates.iter().all(|d| inputs.contains(d)), "no synthesized dates");
}

#[test]
fn escape_like_escapes_wildcards() {
    assert_eq!(escape_like("Sol Ring"), "Sol Ring");
    assert_eq!(escape_like("50%"), "50\\%");
    assert_eq!(escape_like("a_b"), "a\\_b");
    assert_eq!(escape_like("x\\y"), "x\\\\y");
}

#[test]
fn image_url_allowlist() {
    assert!(is_allowed_image_url(
        "https://cards.scryfall.io/normal/front/0/0/x.jpg"
    ));
    assert!(is_allowed_image_url("https://scryfall.io/x.png"));
    assert!(!is_allowed_image_url("http://cards.scryfall.io/x.jpg")); // not https
    assert!(!is_allowed_image_url("https://evil.example.com/x.jpg")); // wrong host
    assert!(!is_allowed_image_url("https://scryfall.io.evil.com/x.jpg")); // suffix trick
    assert!(!is_allowed_image_url("not a url"));
}

#[test]
fn image_error_maps_unavailable_to_404_not_500() {
    use crate::catalog::images::ImageError;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    // The issue #214 fix: a provider "no image" (the TCGplayer CDN `403`s a product with
    // no art) is a 404 the SPA falls back on — never the 500 that spammed the logs and
    // re-fired on every single view.
    let unavailable = image_error_response(
        ImageError::Unavailable(StatusCode::FORBIDDEN),
        "product",
        "248193",
    );
    assert_eq!(unavailable.into_response().status(), StatusCode::NOT_FOUND);

    // A local cache disk write failure is still a genuine 500.
    let io = image_error_response(
        ImageError::Io(std::io::Error::other("disk full")),
        "product",
        "1",
    );
    assert_eq!(
        io.into_response().status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

fn params(sort: Option<&str>, dir: Option<&str>) -> ListParams {
    ListParams {
        page: None,
        page_size: None,
        q: None,
        include_related: None,
        sort: sort.map(str::to_string),
        dir: dir.map(str::to_string),
        name: None,
        drop: None,
    }
}

#[test]
fn list_params_clamps_page_size() {
    let p = ListParams {
        page: Some(0),
        page_size: Some(9999),
        q: None,
        include_related: None,
        sort: None,
        dir: None,
        name: None,
        drop: None,
    };
    assert_eq!(p.page_and_size(), (1, MAX_PAGE_SIZE));
    let d = ListParams {
        page: None,
        page_size: None,
        q: Some("  ".into()),
        include_related: None,
        sort: None,
        dir: None,
        name: None,
        drop: None,
    };
    assert_eq!(d.page_and_size(), (1, DEFAULT_PAGE_SIZE));
    assert_eq!(d.search(), None);
}

#[test]
fn sort_spec_uses_endpoint_default_when_absent() {
    assert_eq!(
        params(None, None).sort_spec_with(SortField::Number, None, None).unwrap(),
        (SortField::Number, SortDir::Asc),
    );
    assert_eq!(
        params(None, None).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Name, SortDir::Asc),
    );
    // Blank values are treated as absent (fall back to the default).
    assert_eq!(
        params(Some("  "), Some("")).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Name, SortDir::Asc),
    );
}

#[test]
fn sort_spec_field_picks_natural_direction() {
    // A field with no explicit dir defaults to its natural direction.
    assert_eq!(
        params(Some("price"), None).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Price, SortDir::Desc),
    );
    assert_eq!(
        params(Some("released"), None).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Released, SortDir::Desc),
    );
    assert_eq!(
        params(Some("cmc"), None).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Cmc, SortDir::Asc),
    );
    // An explicit dir overrides the natural one; aliases resolve.
    assert_eq!(
        params(Some("collector"), Some("desc")).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Number, SortDir::Desc),
    );
    assert_eq!(
        params(Some("mv"), Some("asc")).sort_spec_with(SortField::Name, None, None).unwrap(),
        (SortField::Cmc, SortDir::Asc),
    );
}

#[test]
fn sort_spec_rejects_unknown_values() {
    assert!(matches!(
        params(Some("nonsense"), None).sort_spec_with(SortField::Name, None, None),
        Err(AppError::Validation(_)),
    ));
    assert!(matches!(
        params(None, Some("sideways")).sort_spec_with(SortField::Name, None, None),
        Err(AppError::Validation(_)),
    ));
}

#[test]
fn sort_spec_precedence_url_over_directive_over_default() {
    // An in-query order:/direction: directive fills the gap when the URL params
    // are absent.
    assert_eq!(
        params(None, None)
            .sort_spec_with(SortField::Name, Some(SortField::Edhrec), Some(SortDir::Desc))
            .unwrap(),
        (SortField::Edhrec, SortDir::Desc),
    );
    // An explicit URL sort wins over the in-query directive.
    assert_eq!(
        params(Some("cmc"), None)
            .sort_spec_with(SortField::Name, Some(SortField::Edhrec), None)
            .unwrap(),
        (SortField::Cmc, SortDir::Asc),
    );
}

/// `unique:cards` groups by `oracle_id`, so the paginator counts distinct cards
/// (not printings) and a page holds one row per group — proving SeaORM's
/// `num_items()` counts groups over a grouped query.
#[tokio::test]
async fn unique_cards_groups_by_oracle_id() {
    use crate::handlers::shared::apply_card_sort;
    use sea_orm::{ActiveModelTrait, PaginatorTrait, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    // Two printings share oracle o1; one is o2; one has no oracle id.
    for (i, (name, oracle)) in [
        ("A1", Some("o1")),
        ("A2", Some("o1")),
        ("B", Some("o2")),
        ("C", None),
    ]
    .iter()
    .enumerate()
    {
        card::ActiveModel {
            game: Set("mtg".into()),
            external_id: Set(format!("ext-{i}")),
            name: Set((*name).into()),
            set_code: Set("tst".into()),
            set_name: Set("TST".into()),
            collector_number: Set(i.to_string()),
            lang: Set("en".into()),
            oracle_id: Set(oracle.map(str::to_owned)),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let base = || Card::find().filter(card::Column::Game.eq("mtg"));
    assert_eq!(base().all(&db).await.unwrap().len(), 4, "4 printings total");
    // unique:cards collapses the two o1 printings; the null-oracle card stays
    // distinct (grouped on '#'||id). So o1 + o2 + C = 3.
    let grouped = apply_unique(base(), Some(crate::scryfall::search::UniqueMode::Cards), Dialect::Sqlite);
    let paginator =
        apply_card_sort(grouped, SortField::Name, SortDir::Asc, false, Dialect::Sqlite).paginate(&db, 60);
    assert_eq!(paginator.num_items().await.unwrap(), 3, "distinct cards");
    assert_eq!(paginator.fetch_page(0).await.unwrap().len(), 3, "one row per group");
}

/// A minimal, insertable card row whose only meaningful fields for these tests
/// are its id and the two USD price columns.
fn test_card(id: i32, usd: Option<&str>, usd_foil: Option<&str>) -> card::Model {
    card::Model {
        price_usd: usd.map(str::to_string),
        price_usd_foil: usd_foil.map(str::to_string),
        ..crate::test_support::card_model(id)
    }
}

/// A card row whose printing-identity fields (oracle id, set, release date)
/// are what `prints_query` filters and orders on; everything else reuses the
/// minimal `test_card` shape.
fn print_card(id: i32, oracle_id: Option<&str>, set_code: &str, released: Option<&str>) -> card::Model {
    card::Model {
        oracle_id: oracle_id.map(str::to_string),
        set_code: set_code.into(),
        set_name: set_code.to_uppercase(),
        released_at: released.map(str::to_string),
        ..test_card(id, None, None)
    }
}

/// `prints_query` returns every other printing sharing the card's oracle id,
/// newest released first, and excludes the card itself, other oracle ids, and
/// cards with no oracle id.
#[tokio::test]
async fn prints_query_returns_other_printings_newest_first() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let db = crate::test_support::migrated_memory_db().await;

    // Three printings of oracle "o-1" across three sets/dates, one unrelated
    // oracle, and one card with no oracle id at all.
    for c in [
        print_card(1, Some("o-1"), "aaa", Some("2020-01-01")),
        print_card(2, Some("o-1"), "bbb", Some("2022-01-01")),
        print_card(3, Some("o-1"), "ccc", Some("2024-01-01")),
        print_card(4, Some("o-2"), "ddd", Some("2024-06-01")),
        print_card(5, None, "eee", Some("2024-06-01")),
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }

    let ids = |rows: Vec<card::Model>| rows.iter().map(|r| r.id).collect::<Vec<_>>();

    // Other "o-1" printings of card 2, newest first: ccc(2024) then aaa(2020).
    // The unrelated oracle (4) and the null-oracle card (5) are excluded.
    let rows = prints_query("mtg", "o-1", 2).all(&db).await.expect("query");
    assert_eq!(ids(rows), vec![3, 1]);

    // An oracle with only the one printing (itself) has no other printings.
    let none = prints_query("mtg", "o-2", 4).all(&db).await.expect("query");
    assert!(none.is_empty());
}

#[test]
fn exact_name_trims_and_blank_is_none() {
    let p = ListParams {
        name: Some("  Lightning Bolt ".into()),
        ..params(None, None)
    };
    assert_eq!(p.exact_name(), Some("Lightning Bolt"));
    let blank = ListParams {
        name: Some("   ".into()),
        ..params(None, None)
    };
    assert_eq!(blank.exact_name(), None);
    assert_eq!(params(None, None).exact_name(), None);
}

/// A card row with a specific name/set, reusing the minimal `test_card` shape for
/// every other column.
fn named_card(id: i32, name: &str, set_code: &str) -> card::Model {
    card::Model {
        name: name.to_string(),
        set_code: set_code.into(),
        set_name: set_code.to_uppercase(),
        ..test_card(id, None, None)
    }
}

/// `name_suggestions_query` returns distinct names containing the term (a reprint's
/// two printings collapse to one suggestion), surfaces prefix matches first, is
/// case-insensitive, and honours the limit.
#[tokio::test]
async fn name_suggestions_are_distinct_prefix_first_and_capped() {
    use sea_orm::{ActiveModelTrait, IntoActiveModel};

    let db = crate::test_support::migrated_memory_db().await;
    for c in [
        named_card(1, "Lightning Bolt", "aaa"),
        named_card(2, "Lightning Bolt", "bbb"), // reprint: same name, second printing
        named_card(3, "Bolt", "aaa"),
        named_card(4, "Bolt Catcher", "aaa"),
        named_card(5, "Sol Ring", "aaa"), // no "bolt" -> never suggested
    ] {
        c.into_active_model().insert(&db).await.expect("insert card");
    }

    // Substring match, deduplicated ("Lightning Bolt" once despite two printings),
    // prefix matches ("Bolt", "Bolt Catcher") ahead of the mid-string one, each
    // group alphabetical.
    let got = name_suggestions_query("mtg", "bolt", 10)
        .into_tuple::<String>()
        .all(&db)
        .await
        .expect("query");
    assert_eq!(
        got,
        vec![
            "Bolt".to_string(),
            "Bolt Catcher".to_string(),
            "Lightning Bolt".to_string(),
        ],
    );

    // Case-insensitive (SQLite's ASCII LIKE), same three distinct names.
    let upper = name_suggestions_query("mtg", "BOLT", 10)
        .into_tuple::<String>()
        .all(&db)
        .await
        .expect("query");
    assert_eq!(upper.len(), 3);

    // The limit caps the suggestion count, keeping the prefix-first pair.
    let capped = name_suggestions_query("mtg", "bolt", 2)
        .into_tuple::<String>()
        .all(&db)
        .await
        .expect("query");
    assert_eq!(capped, vec!["Bolt".to_string(), "Bolt Catcher".to_string()]);

    // A term nothing matches yields no suggestions.
    let none = name_suggestions_query("mtg", "zzz", 10)
        .into_tuple::<String>()
        .all(&db)
        .await
        .expect("query");
    assert!(none.is_empty());
}

#[test]
fn drop_page_and_size_clamps() {
    let p = ListParams {
        page: Some(0),
        page_size: Some(9999),
        q: None,
        include_related: None,
        sort: None,
        dir: None,
        name: None,
        drop: None,
    };
    assert_eq!(p.drop_page_and_size(), (1, MAX_DROP_PAGE_SIZE));
    let d = ListParams {
        page: None,
        page_size: None,
        q: None,
        include_related: None,
        sort: None,
        dir: None,
        name: None,
        drop: None,
    };
    assert_eq!(d.drop_page_and_size(), (1, DEFAULT_DROP_PAGE_SIZE));
}
