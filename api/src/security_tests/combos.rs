//! The combo reads (issue #683): the deck page's "Combos" on the authed surface, its
//! public and precon mirrors, the card page's "Combos with", and the dataset mirror's
//! compact snapshot.
//!
//! What these pin, over the pure-function tests beside `analysis::combos`:
//!
//! * **What "in the deck" means, end to end** over real rows: a combo whose pieces are all
//!   held is complete; one piece away is "almost"; a piece that must be the commander only
//!   counts from the command zone of a format that leads with one; a template is always a
//!   missing card; and "almost" is filtered to the commander's colour identity.
//! * **No data is not "no combos"**: an instance without the dataset says `available:
//!   false`.
//! * The card page answers the same combos for every printing (keyed by oracle id), and
//!   is CDN-cacheable like every catalog read.
//! * The mirror's snapshot is **absent until an import completed**, gzipped, ETag-gated,
//!   and reads back through the consumer's own seam.
//!
//! Drives the real router over the seeded dummy catalog, whose `spellbook::dummy` combos
//! are built over the base set's first six cards (`dummy-oracle-base-0001..0006`).

use axum::http::header::{ETAG, IF_NONE_MATCH};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::harness::*;
use crate::entities::card;
use crate::entities::prelude::Card;

const PW: &str = "correct-horse-battery-staple";

/// The external id of the dummy base card carrying `dummy-oracle-base-{n}`.
async fn base_card(app: &TestApp, n: usize) -> String {
    Card::find()
        .filter(card::Column::OracleId.eq(format!("dummy-oracle-base-{n:04}")))
        .one(&app.state.db)
        .await
        .expect("query")
        .expect("the dummy catalog seeds oracle ids on its first base cards")
        .external_id
}

/// A deck with `format`; returns `(deck_id, section id by name)`.
async fn new_deck(app: &TestApp, token: &str, format: &str) -> (i64, serde_json::Value) {
    let (status, _, deck) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg",
            token,
            json!({ "name": "Combo deck", "format": format }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create deck failed: {deck:?}");
    (
        deck["id"].as_i64().expect("deck id"),
        deck["sections"].clone(),
    )
}

fn section_named(sections: &serde_json::Value, name: &str) -> i64 {
    sections
        .as_array()
        .expect("sections")
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("a {name} section is seeded"))["id"]
        .as_i64()
        .expect("section id")
}

async fn put_card(app: &TestApp, token: &str, deck_id: i64, card: &str, section_id: i64, qty: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            token,
            json!({ "quantity": qty, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add card failed: {body:?}");
}

async fn combos(app: &TestApp, token: &str, deck_id: i64) -> serde_json::Value {
    let (status, headers, body) = send(
        app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}/combos"), token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "combos read failed: {body:?}");
    // Per-user, never shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));
    body
}

fn ids(list: &serde_json::Value) -> Vec<&str> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|c| c["id"].as_str().expect("combo id"))
        .collect()
}

#[tokio::test]
async fn a_deck_holding_every_piece_has_the_combo_and_is_one_short_of_the_next() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "combos-1@example.com", PW).await;
    let (c1, c2) = (base_card(&app, 1).await, base_card(&app, 2).await);
    let (deck_id, sections) = new_deck(&app, &token, "Commander").await;
    let main = section_named(&sections, "Creatures");
    put_card(&app, &token, deck_id, &c1, main, 1).await;
    put_card(&app, &token, deck_id, &c2, main, 1).await;

    let body = combos(&app, &token, deck_id).await;
    assert_eq!(body["available"], true);
    assert_eq!(body["source"], "Commander Spellbook");
    assert_eq!(body["source_url"], "https://commanderspellbook.com");
    assert_eq!(ids(&body["combos"]), vec!["dummy-1-2"]);
    assert_eq!(body["combo_count"], 1);
    // The two-card combo's pieces link to the deck's own printings.
    let pieces = body["combos"][0]["pieces"].as_array().expect("pieces");
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|p| p["in_deck"] == true));
    assert_eq!(pieces[0]["card_id"], c1);
    assert_eq!(
        body["combos"][0]["url"],
        "https://commanderspellbook.com/combo/dummy-1-2"
    );
    assert_eq!(body["combos"][0]["produces"][0], "Infinite card draw");
    assert!(
        body["combos"][0]["missing"]
            .as_array()
            .expect("missing")
            .is_empty()
    );

    // One card away from the three-card combo (an empty command zone applies no colour
    // filter), and the missing card links to a catalog printing.
    assert_eq!(ids(&body["almost"]), vec!["dummy-1-2-3"]);
    assert_eq!(body["almost_count"], 1);
    let missing = body["almost"][0]["missing"].as_array().expect("missing");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["kind"], "card");
    assert_eq!(missing[0]["card_id"], base_card(&app, 3).await);
    let unheld = body["almost"][0]["pieces"]
        .as_array()
        .expect("pieces")
        .iter()
        .find(|p| p["in_deck"] == false)
        .expect("the unheld piece");
    assert_eq!(unheld["name"], missing[0]["name"]);
}

#[tokio::test]
async fn a_commander_piece_counts_only_from_the_command_zone_of_a_leading_format() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "combos-2@example.com", PW).await;
    let (c4, c5) = (base_card(&app, 4).await, base_card(&app, 5).await);

    // In the 99: one short, and the miss says why.
    let (deck_id, sections) = new_deck(&app, &token, "Commander").await;
    let main = section_named(&sections, "Creatures");
    let zone = section_named(&sections, "Commander");
    put_card(&app, &token, deck_id, &c4, main, 1).await;
    put_card(&app, &token, deck_id, &c5, main, 1).await;
    let body = combos(&app, &token, deck_id).await;
    assert!(ids(&body["combos"]).is_empty());
    assert_eq!(ids(&body["almost"]), vec!["dummy-4-5"]);
    assert_eq!(body["almost"][0]["missing"][0]["kind"], "commander");
    assert_eq!(body["almost"][0]["missing"][0]["card_id"], c4);

    // Moved into the command zone: complete.
    let (status, _, moved) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{c4}/move"),
            &token,
            json!({ "from_section_id": main, "to_section_id": zone }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved:?}");
    let body = combos(&app, &token, deck_id).await;
    assert_eq!(ids(&body["combos"]), vec!["dummy-4-5"]);
    assert!(
        body["combos"][0]["pieces"][0]["must_be_commander"]
            .as_bool()
            .expect("flag")
    );

    // In a format with no command zone the seeded `Commander` section is just part of
    // the 60, so nothing is ever the commander: a combo that needs one isn't a card away,
    // it is unreachable, and it is listed nowhere.
    let (modern_id, sections) = new_deck(&app, &token, "Modern").await;
    let zone = section_named(&sections, "Commander");
    let main = section_named(&sections, "Creatures");
    put_card(&app, &token, modern_id, &c4, zone, 1).await;
    put_card(&app, &token, modern_id, &c5, main, 1).await;
    let body = combos(&app, &token, modern_id).await;
    assert!(ids(&body["combos"]).is_empty());
    assert!(ids(&body["almost"]).is_empty(), "{:?}", body["almost"]);
    assert_eq!(body["almost_count"], 0);
}

#[tokio::test]
async fn almost_is_filtered_to_the_commanders_colours_and_a_template_is_a_missing_card() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "combos-3@example.com", PW).await;
    let (c1, c2, c4, c6) = (
        base_card(&app, 1).await,
        base_card(&app, 2).await,
        base_card(&app, 4).await,
        base_card(&app, 6).await,
    );
    let (deck_id, sections) = new_deck(&app, &token, "Commander").await;
    let main = section_named(&sections, "Creatures");
    let zone = section_named(&sections, "Commander");
    put_card(&app, &token, deck_id, &c1, main, 1).await;
    put_card(&app, &token, deck_id, &c2, main, 1).await;
    put_card(&app, &token, deck_id, &c6, main, 1).await;
    // A mono-coloured dummy commander: the three-colour `dummy-1-2-3` no longer fits.
    put_card(&app, &token, deck_id, &c4, zone, 1).await;

    let body = combos(&app, &token, deck_id).await;
    assert_eq!(
        ids(&body["combos"]),
        vec!["dummy-1-2"],
        "complete combos are never filtered"
    );
    let almost = ids(&body["almost"]);
    assert!(
        !almost.contains(&"dummy-1-2-3"),
        "a combo outside the commander's colours can't be added to: {almost:?}"
    );
    // The template combo: its one card is held, so it is exactly one (template) short —
    // and it fits, since the dummy seed gives it the commander's colour or none... either
    // way it is never *complete*.
    assert!(!ids(&body["combos"]).contains(&"dummy-6-t"));
    if almost.contains(&"dummy-6-t") {
        let template = body["almost"]
            .as_array()
            .expect("almost")
            .iter()
            .find(|c| c["id"] == "dummy-6-t")
            .expect("template combo");
        assert_eq!(template["missing"][0]["kind"], "template");
        assert_eq!(template["missing"][0]["name"], "A free sacrifice outlet");
        assert!(template["missing"][0]["card_id"].is_null());
        assert_eq!(template["templates"][0], "A free sacrifice outlet");
    }
}

#[tokio::test]
async fn maybeboard_pieces_do_not_count() {
    let app = test_app_with_catalog().await;
    let (token, _) = register(&app, "combos-4@example.com", PW).await;
    let (c1, c2) = (base_card(&app, 1).await, base_card(&app, 2).await);
    let (deck_id, sections) = new_deck(&app, &token, "Commander").await;
    let main = section_named(&sections, "Creatures");
    let (status, _, maybe) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/decks/mtg/{deck_id}/sections"),
            &token,
            json!({ "name": "Considering", "is_maybeboard": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{maybe:?}");
    let maybe_id = maybe["id"].as_i64().expect("section id");
    put_card(&app, &token, deck_id, &c1, main, 1).await;
    put_card(&app, &token, deck_id, &c2, maybe_id, 1).await;

    let body = combos(&app, &token, deck_id).await;
    assert!(
        ids(&body["combos"]).is_empty(),
        "a maybeboard card isn't in the deck"
    );
    assert_eq!(ids(&body["almost"]), vec!["dummy-1-2"]);
}

#[tokio::test]
async fn without_the_dataset_the_answer_is_unknown_not_none() {
    // No catalog seed: no combo rows at all.
    let app = test_app().await;
    let (token, _) = register(&app, "combos-5@example.com", PW).await;
    let (deck_id, _) = new_deck(&app, &token, "Commander").await;
    let body = combos(&app, &token, deck_id).await;
    assert_eq!(body["available"], false);
    assert!(ids(&body["combos"]).is_empty());
    assert_eq!(body["combo_count"], 0);
}

#[tokio::test]
async fn the_card_page_lists_the_combos_a_card_is_a_piece_of() {
    let app = test_app_with_catalog().await;
    let c1 = base_card(&app, 1).await;
    let (status, headers, body) =
        send(&app, get(&format!("/api/games/mtg/cards/{c1}/combos"))).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    // A public catalog read: CDN-cacheable like the rulings beside it.
    assert!(
        cache_control(&headers).is_some_and(|v| v.contains("public")),
        "{:?}",
        cache_control(&headers)
    );
    assert_eq!(body["total"], 2);
    // Most-played first: the two-card combo (popularity 120) before the three-card (40).
    assert_eq!(ids(&body["combos"]), vec!["dummy-1-2", "dummy-1-2-3"]);
    assert_eq!(body["source"], "Commander Spellbook");
    // Every piece links to a catalog printing.
    for piece in body["combos"][1]["pieces"].as_array().expect("pieces") {
        assert!(piece["card_id"].is_string(), "{piece:?}");
    }
    assert_eq!(
        body["combos"][1]["pieces"][2]["card_id"],
        base_card(&app, 3).await
    );

    // A card in no combo, and one the catalog doesn't hold.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/mtg/cards/{}/combos",
            base_card(&app, 6).await
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    let (status, _, _) = send(&app, get("/api/games/mtg/cards/no-such-card/combos")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------- The dataset mirror ----------

/// The real router with the mirror enabled over a seeded catalog, keeping the state so a
/// test can stamp the import bookkeeping the snapshot is gated on.
async fn mirror_app_with_catalog() -> TestApp {
    let mut state = test_state().await;
    let config = crate::config::Config {
        mirror_enabled: true,
        ..crate::test_support::test_config()
    };
    state.config = std::sync::Arc::new(config);
    crate::catalog::seed_all(&state.db).await;
    test_app_over(state)
}

#[tokio::test]
async fn the_mirror_snapshot_is_absent_until_an_import_completed_then_etag_gated() {
    use crate::catalog::ingest_state::{self, StateFields};
    let app = mirror_app_with_catalog().await;

    // Rows exist (the dummy seed), but no import has been recorded: a consumer must not
    // import a snapshot nobody vouched for.
    let (status, _, _) = send(&app, get("/api/mirror/spellbook/combos")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let now = chrono::Utc::now();
    ingest_state::put(
        &app.state.db,
        StateFields {
            game: "mtg",
            dataset: "combos",
            status: "complete",
            source_updated_at: Some("\"upstream-etag\""),
            detail: "test",
            sets_imported: 0,
            cards_imported: 4,
            started_at: now,
            finished_at: Some(now),
        },
    )
    .await
    .expect("stamp the import");

    let (status, headers, bytes) = send_bytes(&app, get("/api/mirror/spellbook/combos")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/gzip"));
    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("etag")
        .to_string();
    assert!(etag.starts_with("\"combos-"), "{etag}");
    assert_eq!(bytes[0], 0x1f, "gzipped");

    // Reads back through the consumer's own seam into the very records the seed wrote.
    let stream =
        futures_util::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(bytes))]);
    let mut lines = crate::scryfall::client::json_lines(stream)
        .await
        .expect("reader");
    let mut back: Vec<crate::spellbook::model::ComboRecord> = Vec::new();
    while let Some(line) = lines.next_line().await.expect("line") {
        back.push(serde_json::from_str(&line).expect("record"));
    }
    let mut got: Vec<&str> = back.iter().map(|r| r.id.as_str()).collect();
    got.sort_unstable();
    assert_eq!(
        got,
        vec!["dummy-1-2", "dummy-1-2-3", "dummy-4-5", "dummy-6-t"]
    );

    // The stored tag makes the next pull a bodyless 304.
    let req = Request::builder()
        .method("GET")
        .uri("/api/mirror/spellbook/combos")
        .header(IF_NONE_MATCH, &etag)
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send_bytes(&app, req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(
        headers.get(ETAG).and_then(|v| v.to_str().ok()),
        Some(etag.as_str())
    );
}
