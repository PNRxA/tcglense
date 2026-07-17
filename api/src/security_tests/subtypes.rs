//! The by-sub-type ("card treatment") set views — the catalog `/subtypes` endpoint and
//! the `has_subtypes` gate on the set list (issue #282). Drives the real router so the
//! grouping, pagination, and the derived flag are exercised end-to-end.

use sea_orm::{ActiveModelTrait, IntoActiveModel};

use super::harness::*;
use crate::entities::{card, card_set};
use crate::test_support::{card_model, card_set_model, url_encode};

/// Seed `tst` (one card of each derivable treatment) plus `pln` (a single Normal card).
async fn seed_treated_sets(state: &crate::state::AppState) {
    for (id, code) in [(1, "tst"), (2, "pln")] {
        card_set::Model {
            id,
            ..card_set_model(code)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert set");
    }
    // tst: Normal, Borderless (border_color), Showcase (frame_effects).
    for (id, cn, border, frame) in [
        (1, "1", None::<&str>, None::<&str>),
        (2, "2", Some("borderless"), None),
        (3, "3", None, Some("legendary,showcase")),
    ] {
        card::Model {
            set_code: "tst".into(),
            collector_number: cn.into(),
            collector_number_int: cn.parse().ok(),
            border_color: border.map(str::to_string),
            frame_effects: frame.map(str::to_string),
            ..card_model(id)
        }
        .into_active_model()
        .insert(&state.db)
        .await
        .expect("insert card");
    }
    // pln: one Normal card -> no special treatment.
    card::Model {
        set_code: "pln".into(),
        collector_number: "1".into(),
        collector_number_int: Some(1),
        ..card_model(4)
    }
    .into_active_model()
    .insert(&state.db)
    .await
    .expect("insert card");
}

/// The `/subtypes` endpoint buckets a set's cards by derived treatment, Normal first then
/// the treatments in sub-type order, paginated by sub-type (`total` is a group count).
#[tokio::test]
async fn set_subtypes_groups_cards_by_treatment() {
    let state = test_state().await;
    seed_treated_sets(&state).await;
    let app = crate::build_router(state);

    let (status, _, body) = send(&app, get("/api/games/mtg/sets/tst/subtypes")).await;
    assert_eq!(status, StatusCode::OK, "subtypes must succeed: {body:?}");
    let groups = body["data"].as_array().expect("subtype groups");
    let titles: Vec<&str> = groups
        .iter()
        .map(|g| g["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Normal", "Borderless", "Showcase"]);
    assert_eq!(groups[0]["slug"].as_str(), Some("normal"));
    assert!(groups.iter().all(|g| g["card_count"].as_u64() == Some(1)));
    // Pagination is over sub-types (3 groups), not cards.
    assert_eq!(body["total"].as_u64(), Some(3));
}

/// A `q` on the by-sub-type view narrows the cards within each group, dropping now-empty
/// sub-types — the same search the flat listing accepts.
#[tokio::test]
async fn set_subtypes_search_narrows_cards() {
    let state = test_state().await;
    seed_treated_sets(&state).await;
    let app = crate::build_router(state);

    // border:borderless keeps only the one borderless card, so only its group survives.
    let uri = format!(
        "/api/games/mtg/sets/tst/subtypes?q={}",
        url_encode("border:borderless")
    );
    let (status, _, body) = send(&app, get(&uri)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "searched subtypes must succeed: {body:?}"
    );
    let groups = body["data"].as_array().expect("subtype groups");
    assert_eq!(groups.len(), 1, "one matching group: {body:?}");
    assert_eq!(groups[0]["title"].as_str(), Some("Borderless"));

    // A malformed query is a 422, like every other list route (never a 500).
    let (bad, _, _) = send(&app, get("/api/games/mtg/sets/tst/subtypes?q=boguskey%3A1")).await;
    assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);

    // An unknown set is a 404.
    let (missing, _, _) = send(&app, get("/api/games/mtg/sets/nope/subtypes")).await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

/// The set list's `has_subtypes` flag gates the SPA toggle: true for a set with a special
/// treatment, false for one with only Normal cards.
#[tokio::test]
async fn set_list_reports_has_subtypes() {
    let state = test_state().await;
    seed_treated_sets(&state).await;
    let app = crate::build_router(state);

    let (status, _, body) = send(&app, get("/api/games/mtg/sets")).await;
    assert_eq!(status, StatusCode::OK);
    let sets = body["data"].as_array().expect("sets");
    let flag = |code: &str| {
        sets.iter()
            .find(|s| s["code"].as_str() == Some(code))
            .and_then(|s| s["has_subtypes"].as_bool())
    };
    assert_eq!(
        flag("tst"),
        Some(true),
        "set with treatments offers the view"
    );
    assert_eq!(flag("pln"), Some(false), "all-Normal set does not");

    // The single-set endpoint agrees.
    let (_, _, one) = send(&app, get("/api/games/mtg/sets/tst")).await;
    assert_eq!(one["has_subtypes"].as_bool(), Some(true));
}
