//! Per-user decks (issue #363): authentication gating, per-deck **ownership isolation**
//! (a deck card has no `user_id`, so every route must prove the parent deck is the
//! caller's — a cross-user id is a 404, never a 403), the create/section/card round trip,
//! read-only-key write gating, and per-deck public sharing (handle-addressed, no PII,
//! CDN-cacheable; enabling requires a username first).
//!
//! Drives the real router over the seeded dummy catalog, so cards can be added by their
//! real external ids and read back in the full catalog `Card` shape.

use std::fmt::Write;

use super::harness::*;

const PW: &str = "correct-horse-battery-staple";

/// Grab `n` real card external ids from the seeded catalog.
async fn sample_card_ids(app: &Router, n: usize) -> Vec<String> {
    let (status, _, body) = send(app, get("/api/games/mtg/cards?page_size=25")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "listing seeded cards failed: {body:?}"
    );
    let data = body["data"].as_array().expect("cards data array");
    assert!(
        data.len() >= n,
        "need >= {n} seeded cards, got {}",
        data.len()
    );
    data.iter()
        .take(n)
        .map(|c| c["id"].as_str().expect("card id").to_string())
        .collect()
}

/// Create a deck for the token's user and return its full detail body.
async fn create_deck(app: &TestApp, token: &str, name: &str) -> Value {
    let (status, _, body) = send(
        app,
        json_with_bearer("POST", "/api/decks/mtg", token, json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create deck failed: {body:?}");
    body
}

/// Mint a scoped API key for a signed-in user.
async fn create_key(app: &TestApp, access: &str, scope: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            json!({ "name": "k", "scope": scope }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create key failed: {body:?}");
    body["key"].as_str().expect("plaintext key").to_string()
}

#[tokio::test]
async fn decks_require_authentication() {
    let app = test_app_with_catalog().await;

    // No bearer -> 401, and per-user data must never be shared-cached.
    let (status, headers, _) = send(&app, get("/api/decks/mtg")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn create_seeds_default_sections_and_round_trips_a_card() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "deckbuilder@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);

    let deck = create_deck(&app, &access, "Krenko Goblins").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    // The default sections are seeded on creation.
    let sections = deck["sections"].as_array().expect("sections");
    assert!(sections.len() > 5, "default sections should be seeded");
    assert!(sections.iter().any(|s| s["name"] == "Commander"));
    assert!(deck["cards"].as_array().expect("cards").is_empty());
    let section_id = sections[0]["id"].as_i64().expect("section id");

    // Add a card to a section.
    let (status, headers, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &access,
            json!({ "quantity": 3, "foil_quantity": 1, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add card failed: {body:?}");
    assert_eq!(body["quantity"], 3);
    // Per-user data: no-store.
    assert_eq!(cache_control(&headers), Some("no-store"));

    // The deck detail now carries the card + a summary of 4 copies.
    let (status, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cards = deck["cards"].as_array().expect("cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["card"]["id"], card);
    assert_eq!(cards[0]["section_id"], section_id);
    assert_eq!(deck["summary"]["total_cards"], 4);

    // The deck list shows the card count.
    let (status, _, list) = send(&app, get_with_bearer("/api/decks/mtg", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"][0]["card_count"], 4);

    // Both-zero removes the card from the deck.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &access,
            json!({ "quantity": 0, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert!(deck["cards"].as_array().expect("cards").is_empty());
}

#[tokio::test]
async fn changing_a_printing_merges_counts_and_rejects_an_unrelated_card() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "printing-swap@example.com", PW).await;
    let (status, _, printings) = send(
        &app,
        get("/api/games/mtg/cards?name=Dummy%20Reprinted%20Relic&page_size=10"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "printing lookup failed: {printings:?}"
    );
    let printings = printings["data"].as_array().expect("printing data");
    assert_eq!(
        printings.len(),
        2,
        "dummy catalog must contain a reprint pair"
    );
    let current = printings[0]["id"].as_str().expect("current id");
    let replacement = printings[1]["id"].as_str().expect("replacement id");
    let unrelated = sample_card_ids(&app, 1).await.remove(0);

    let deck = create_deck(&app, &access, "Printing swap").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][1]["id"].as_i64().expect("section id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{current}"),
            &access,
            json!({ "quantity": 3, "foil_quantity": 1, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{replacement}"),
            &access,
            json!({ "quantity": 2, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{current}/printing"),
            &access,
            json!({ "new_card_id": replacement, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "printing swap failed: {body:?}");
    assert_eq!(body["quantity"], 5);
    assert_eq!(body["foil_quantity"], 1);

    let (_, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    let cards = detail["cards"].as_array().expect("deck cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["card"]["id"], replacement);
    assert_eq!(cards[0]["quantity"], 5);
    assert_eq!(cards[0]["foil_quantity"], 1);

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{replacement}/printing"),
            &access,
            json!({ "new_card_id": unrelated, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(detail["cards"][0]["card"]["id"], replacement);
}

#[tokio::test]
async fn a_deck_is_isolated_to_its_owner() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-decks@example.com", PW).await;
    let (bob, _) = register(&app, "bob-decks@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);

    let deck = create_deck(&app, &alice, "Alice's Deck").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");

    // Bob can't read Alice's deck: 404 (never 403 — no existence oracle over deck ids).
    let (status, _, _) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &bob),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Nor mutate it (add a card): 404.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &bob,
            json!({ "quantity": 1, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Printing changes are owner-scoped too.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}/printing"),
            &bob,
            json!({ "new_card_id": card, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Nor delete it: 404. Alice's deck survives.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/decks/mtg/{deck_id}"),
            &bob,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn uploaded_deck_import_creates_exact_sections_and_owner_scoped_exports() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-imports@example.com", PW).await;
    let (bob, _) = register(&app, "bob-imports@example.com", PW).await;

    let (status, _, catalog) = send(&app, get("/api/games/mtg/cards?page_size=2")).await;
    assert_eq!(status, StatusCode::OK);
    let first = &catalog["data"][0];
    let second = &catalog["data"][1];
    // "2 Drops" is deliberately a name the plain-text grammar would misread as a card
    // row — the text export must bracket it so the round trip below keeps the section.
    let csv = format!(
        "Quantity,Name,Finish,Scryfall ID,Categories\n\
         2,,Normal,{first_id},2 Drops\n\
         1,,Foil,{first_id},2 drops\n\
         1,,Normal,{second_id},Commander\n\
         1,Missing Card,Normal,not-in-catalog,Sideboard\n",
        first_id = first["id"].as_str().expect("first id"),
        second_id = second["id"].as_str().expect("second id"),
    );
    let (status, headers, imported) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/import",
            &alice,
            json!({
                "provider": "archidekt",
                "source": null,
                "contents": csv,
                "format": "csv",
                "name": "Imported exact sections"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "deck import failed: {imported:?}");
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert_eq!(imported["total_rows"], 4);
    assert_eq!(imported["matched_cards"], 2);
    assert_eq!(imported["unmatched_cards"], 1);

    let deck = &imported["deck"];
    let deck_id = deck["id"].as_i64().expect("deck id");
    assert_eq!(deck["card_count"], 4);
    assert!(
        deck.get("cards").is_none() && deck.get("sections").is_none(),
        "the synchronous import response must stay lightweight"
    );

    let (status, _, detail) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sections = detail["sections"].as_array().expect("sections");
    assert_eq!(sections.len(), 2, "imports must not seed default sections");
    assert_eq!(sections[0]["name"], "2 Drops");
    assert_eq!(sections[1]["name"], "Commander");
    let cards = detail["cards"].as_array().expect("cards");
    assert_eq!(cards.len(), 2);
    let first_entry = cards
        .iter()
        .find(|entry| entry["card"]["id"] == first["id"])
        .expect("first imported card");
    assert_eq!(first_entry["quantity"], 2);
    assert_eq!(first_entry["foil_quantity"], 1);

    // Every format is a real authenticated download and keeps imported section names.
    let (status, headers, archidekt) = send_text(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/export?format=archidekt"),
            &alice,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/csv; charset=utf-8"));
    assert!(archidekt.contains("Scryfall ID"));
    assert!(archidekt.contains("2 Drops"));

    let (status, headers, moxfield_text) = send_text(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/export?format=moxfield-text"),
            &alice,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    assert!(
        moxfield_text.starts_with("[2 Drops]\n"),
        "a quantity-leading section name must export bracketed, got: {moxfield_text:?}"
    );
    assert!(moxfield_text.contains(" *F*"));

    let (status, _, moxfield_csv) = send_text(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/export?format=moxfield"),
            &alice,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(moxfield_csv.contains("\"Collector Number\",\"Board\""));

    // The sectioned text export is accepted by the upload path and recreates the
    // same section/card/finish structure as a new deck.
    let (status, _, round_trip) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/import",
            &alice,
            json!({
                "provider": "moxfield",
                "source": null,
                "contents": moxfield_text,
                "format": "text",
                "name": "Round trip"
            }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "text re-import failed: {round_trip:?}"
    );
    assert_eq!(round_trip["deck"]["card_count"], 4);
    assert!(round_trip["deck"].get("cards").is_none());
    let round_trip_id = round_trip["deck"]["id"].as_i64().expect("round-trip id");
    let (status, _, round_trip_detail) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{round_trip_id}"), &alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(round_trip_detail["sections"].as_array().unwrap().len(), 2);
    assert_eq!(round_trip_detail["cards"].as_array().unwrap().len(), 2);
    assert_eq!(round_trip_detail["summary"]["total_cards"], 4);

    // Export ownership is the same non-oracle 404 as every other deck-scoped read.
    let (status, _, _) = send(
        &app,
        get_with_bearer(
            &format!("/api/decks/mtg/{deck_id}/export?format=moxfield"),
            &bob,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oversized_deck_upload_is_rejected_without_creating_a_deck() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "oversized-deck@example.com", PW).await;
    let card_id = sample_card_ids(&app, 1).await.remove(0);
    let mut csv = String::from("Quantity,Name,Scryfall ID,Categories\n");
    for index in 0..=crate::deck_import::MAX_DECK_IMPORT_ROWS {
        writeln!(csv, "1,Card {index},{card_id},Mainboard").expect("write CSV row");
    }

    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/import",
            &access,
            json!({
                "provider": "archidekt",
                "source": null,
                "contents": csv,
                "format": "csv",
                "name": "Too large"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("limit is 2000")
    );

    let (status, _, decks) = send(&app, get_with_bearer("/api/decks/mtg", &access)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(decks["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn moxfield_live_deck_import_uses_the_collection_provider_gate() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "moxfield-decks@example.com", PW).await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/import",
            &access,
            json!({
                "provider": "moxfield",
                "source": "https://moxfield.com/decks/4xUdq-66IEKK6X53bhUS8Q",
                "contents": null,
                "format": null,
                "name": null
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("upload")
    );

    let (_, _, list) = send(&app, get_with_bearer("/api/decks/mtg", &access)).await;
    assert!(list["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_read_only_key_can_read_but_not_write_decks() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "keydecks@example.com", PW).await;
    let deck = create_deck(&app, &access, "Keyed").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let ro = create_key(&app, &access, "read").await;

    // A read-only key can list + read decks.
    let (status, _, _) = send(&app, get_with_bearer("/api/decks/mtg", &ro)).await;
    assert_eq!(status, StatusCode::OK);

    // But a write with a read-only key is 403 (valid credential, wrong scope).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg",
            &ro,
            json!({ "name": "should fail" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Import creates a deck, so it is a write too.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/import",
            &ro,
            json!({
                "provider": "archidekt",
                "source": null,
                "contents": "Quantity,Name,Finish,Scryfall ID,Categories\n1,X,Normal,x,Mainboard\n",
                "format": "csv",
                "name": "should fail"
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // And so is a section create.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/decks/mtg/{deck_id}/sections"),
            &ro,
            json!({ "name": "Custom" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sections_move_cards_and_the_last_section_cannot_be_deleted() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "sections@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);
    let deck = create_deck(&app, &access, "Sectioned").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let s1 = deck["sections"][0]["id"].as_i64().expect("s1");
    let s2 = deck["sections"][1]["id"].as_i64().expect("s2");

    // Put the card in section 1, move it to section 2.
    let _ = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &access,
            json!({ "quantity": 2, "foil_quantity": 0, "section_id": s1 }),
        ),
    )
    .await;
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}/move"),
            &access,
            json!({ "from_section_id": s1, "to_section_id": s2 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "move failed: {body:?}");
    let (_, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(deck["cards"][0]["section_id"], s2);

    // Delete section 2 -> its card reassigns to the fallback (still present in the deck).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/decks/mtg/{deck_id}/sections/{s2}"),
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(
        deck["cards"].as_array().expect("cards").len(),
        1,
        "card survived the delete"
    );

    // Delete every section down to the last; the last one is refused (409).
    let mut sections: Vec<i64> = deck["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_i64().unwrap())
        .collect();
    let last = sections.pop().unwrap();
    for id in sections {
        let (status, _, _) = send(
            &app,
            json_with_bearer(
                "DELETE",
                &format!("/api/decks/mtg/{deck_id}/sections/{id}"),
                &access,
                json!({}),
            ),
        )
        .await;
        assert!(
            status == StatusCode::NO_CONTENT || status == StatusCode::CONFLICT,
            "unexpected section delete status: {status}"
        );
    }
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/decks/mtg/{deck_id}/sections/{last}"),
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the last section can't be deleted"
    );
}

#[tokio::test]
async fn folders_organise_decks_and_ungroup_on_delete() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "folders@example.com", PW).await;
    let deck = create_deck(&app, &access, "Filed").await;
    let deck_id = deck["id"].as_i64().expect("deck id");

    // Create a folder, move the deck into it.
    let (status, _, folder) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/folders",
            &access,
            json!({ "name": "EDH" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create folder failed: {folder:?}");
    let folder_id = folder["id"].as_i64().expect("folder id");

    let (status, _, moved) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/folder"),
            &access,
            json!({ "folder_id": folder_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["folder_id"], folder_id);

    // The folder reports one deck.
    let (_, _, folders) = send(&app, get_with_bearer("/api/decks/mtg/folders", &access)).await;
    assert_eq!(folders["data"][0]["deck_count"], 1);

    // Deleting the folder ungroups the deck (folder_id -> null), never deletes it.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/decks/mtg/folders/{folder_id}"),
            &access,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, deck) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{deck_id}"), &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "deck survived the folder delete");
    assert!(deck["folder_id"].is_null());
}

#[tokio::test]
async fn public_sharing_requires_a_username_then_serves_a_cacheable_no_pii_view() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "sharer@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);
    let deck = create_deck(&app, &access, "Shared Deck").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");
    let _ = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &access,
            json!({ "quantity": 1, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;

    // Enabling public without a username is a 409 (a public deck is addressed by handle).
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            &access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // While still private, the public route 404s (no-store, never CDN-pinned).
    let (status, headers, _) = send(&app, get("/api/u/nobody-0001/decks/1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Set a username, then enable public.
    let (status, _, u) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            &access,
            json!({ "username": "sharer" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let handle = u["handle"].as_str().expect("handle").to_string();
    let (status, _, vis) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            &access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(vis["public"], true);

    // The public deck reads unauthenticated, CDN-cacheable, and carries no email/PII.
    let (status, headers, body) =
        send(&app, get(&format!("/api/u/{handle}/decks/{deck_id}"))).await;
    assert_eq!(status, StatusCode::OK, "public deck read failed: {body:?}");
    assert_eq!(body["id"], deck_id);
    assert_eq!(body["handle"], handle);
    assert_eq!(body["cards"].as_array().expect("cards").len(), 1);
    let cc = cache_control(&headers).unwrap_or("");
    assert!(
        cc.contains("public") && cc.contains("s-maxage"),
        "expected a public CDN cache: {cc}"
    );
    // No PII leaks through the public surface.
    let raw = body.to_string();
    assert!(
        !raw.contains("sharer@example.com"),
        "public deck leaked the owner email"
    );

    // The public deck list carries this one deck.
    let (status, _, list) = send(&app, get(&format!("/api/u/{handle}/decks"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().expect("decks").len(), 1);

    // Disable sharing -> the public read 404s again.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            &access,
            json!({ "public": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(&app, get(&format!("/api/u/{handle}/decks/{deck_id}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A user who has shared only a deck (no public collection) still has a resolvable public
/// profile (issue #391), so the profile page can list those decks — while a user with
/// nothing public stays a uniform 404 (the relaxation opens no bare-profile leak).
#[tokio::test]
async fn profile_resolves_for_a_decks_only_user() {
    let app = test_app_with_catalog().await;

    // A username'd user with nothing public: the profile 404s.
    let (access, _) = register(&app, "decksonly@example.com", PW).await;
    let (status, _, u) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            &access,
            json!({ "username": "decksonly" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let handle = u["handle"].as_str().expect("handle").to_string();

    let (status, _, _) = send(&app, get(&format!("/api/u/{handle}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing public -> 404");

    // Create a deck and make it public — the user still has NO public collection.
    let deck = create_deck(&app, &access, "Mono-Red").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            &access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Now the profile resolves (200) with an empty collection list — the deck alone is
    // enough for the page to render (and list the public deck via /api/u/{handle}/decks).
    let (status, _, body) = send(&app, get(&format!("/api/u/{handle}"))).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a public deck should make the profile resolve: {body:?}"
    );
    assert_eq!(body["username"], "decksonly");
    assert!(
        body["games"].as_array().expect("games array").is_empty(),
        "no public collection -> empty games"
    );

    // A different username'd user who shares nothing still 404s.
    let (other, _) = register(&app, "nothing@example.com", PW).await;
    let (status, _, u2) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            &other,
            json!({ "username": "nothing" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let other_handle = u2["handle"].as_str().expect("handle").to_string();
    let (status, _, _) = send(&app, get(&format!("/api/u/{other_handle}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "still nothing public -> 404");
}

#[tokio::test]
async fn a_blank_deck_name_is_rejected() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "blank@example.com", PW).await;
    let (status, _, _) = send(
        &app,
        json_with_bearer("POST", "/api/decks/mtg", &access, json!({ "name": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_foreign_section_id_cannot_smuggle_a_card_into_a_deck() {
    // The card write/move handlers take a section_id from the body; load_section must gate
    // it to the target deck, so a section belonging to *another* deck (even the caller's own)
    // is a 404 — a card can't be filed into a section that isn't the deck's.
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "smuggle@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);

    let deck_a = create_deck(&app, &access, "A").await;
    let deck_b = create_deck(&app, &access, "B").await;
    let a_id = deck_a["id"].as_i64().expect("a id");
    let a_section = deck_a["sections"][0]["id"].as_i64().expect("a section");
    let b_section = deck_b["sections"][0]["id"].as_i64().expect("b section");

    // Writing to deck A with deck B's section id is 404.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{a_id}/cards/{card}"),
            &access,
            json!({ "quantity": 1, "foil_quantity": 0, "section_id": b_section }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Legitimately add to A, then a move naming B's section is 404 too.
    let _ = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{a_id}/cards/{card}"),
            &access,
            json!({ "quantity": 1, "foil_quantity": 0, "section_id": a_section }),
        ),
    )
    .await;
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{a_id}/cards/{card}/move"),
            &access,
            json!({ "from_section_id": a_section, "to_section_id": b_section }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_folder_is_isolated_to_its_owner() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-folder@example.com", PW).await;
    let (bob, _) = register(&app, "bob-folder@example.com", PW).await;

    let (status, _, folder) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg/folders",
            &alice,
            json!({ "name": "Alice EDH" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let folder_id = folder["id"].as_i64().expect("folder id");

    // Bob can't file a new deck under Alice's folder (404 on the folder ref)...
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/decks/mtg",
            &bob,
            json!({ "name": "Bob's", "folder_id": folder_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ...nor rename or delete it...
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/folders/{folder_id}"),
            &bob,
            json!({ "name": "hijacked" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "DELETE",
            &format!("/api/decks/mtg/folders/{folder_id}"),
            &bob,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ...nor move his own deck into it.
    let bob_deck = create_deck(&app, &bob, "Bob's deck").await;
    let bob_deck_id = bob_deck["id"].as_i64().expect("bob deck id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{bob_deck_id}/folder"),
            &bob,
            json!({ "folder_id": folder_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Alice's folder is untouched.
    let (_, _, folders) = send(&app, get_with_bearer("/api/decks/mtg/folders", &alice)).await;
    assert_eq!(folders["data"].as_array().expect("folders").len(), 1);
    assert_eq!(folders["data"][0]["name"], "Alice EDH");
}

#[tokio::test]
async fn a_public_deck_is_not_readable_under_another_users_handle() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "alice-pub@example.com", PW).await;
    let (bob, _) = register(&app, "bob-pub@example.com", PW).await;

    let (_, _, au) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            &alice,
            json!({ "username": "alicepub" }),
        ),
    )
    .await;
    let alice_handle = au["handle"].as_str().expect("alice handle").to_string();
    let (_, _, bu) = send(
        &app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            &bob,
            json!({ "username": "bobpub" }),
        ),
    )
    .await;
    let bob_handle = bu["handle"].as_str().expect("bob handle").to_string();

    let alice_deck = create_deck(&app, &alice, "Alice public").await;
    let alice_deck_id = alice_deck["id"].as_i64().expect("alice deck id");
    let _ = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{alice_deck_id}/visibility"),
            &alice,
            json!({ "public": true }),
        ),
    )
    .await;
    // Bob also publishes one, so his handle resolves to a user with a public deck.
    let bob_deck = create_deck(&app, &bob, "Bob public").await;
    let bob_deck_id = bob_deck["id"].as_i64().expect("bob deck id");
    let _ = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{bob_deck_id}/visibility"),
            &bob,
            json!({ "public": true }),
        ),
    )
    .await;

    // Alice's public deck reads under Alice's handle, but NOT under Bob's (the lookup is
    // scoped to the handle's user, so a deck id can't be read across handles).
    let (status, _, _) = send(
        &app,
        get(&format!("/api/u/{alice_handle}/decks/{alice_deck_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = send(
        &app,
        get(&format!("/api/u/{bob_handle}/decks/{alice_deck_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An unknown handle and a valid-handle-miss return the SAME 404 body (no username
    // enumeration oracle).
    let (s1, _, b1) = send(&app, get("/api/u/nobody-9999/decks/1")).await;
    let (s2, _, b2) = send(
        &app,
        get(&format!("/api/u/{bob_handle}/decks/{alice_deck_id}")),
    )
    .await;
    assert_eq!(s1, StatusCode::NOT_FOUND);
    assert_eq!(s2, StatusCode::NOT_FOUND);
    assert_eq!(
        b1["error"], b2["error"],
        "unknown-handle and valid-handle-miss must be indistinguishable"
    );
}

/// Set the caller's absolute owned count for a card in the collection (issue #499 setup).
async fn own_card(app: &TestApp, token: &str, card: &str, quantity: i64) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/collection/mtg/cards/{card}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "own card failed: {body:?}");
}

/// Put `quantity` copies of a card into a deck section.
async fn add_deck_card(
    app: &TestApp,
    token: &str,
    deck_id: i64,
    section_id: i64,
    card: &str,
    quantity: i64,
) {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            token,
            json!({ "quantity": quantity, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add deck card failed: {body:?}");
}

/// Find the needed-cards entry for a given card external id, if present.
fn needed_entry<'a>(body: &'a Value, card_id: &str) -> Option<&'a Value> {
    body["data"]
        .as_array()
        .expect("needed data array")
        .iter()
        .find(|entry| entry["card"]["id"] == card_id)
}

#[tokio::test]
async fn needed_cards_sums_demand_across_decks_and_subtracts_the_collection() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "needy@example.com", PW).await;

    // Two catalog cards with distinct gameplay identities (distinct names => distinct
    // oracle groups), so the "any printing" aggregation keeps them separate.
    let (_, _, catalog) = send(&app, get("/api/games/mtg/cards?page_size=25")).await;
    let cards = catalog["data"].as_array().expect("catalog cards");
    let shared = cards[0].clone();
    let shared_id = shared["id"].as_str().expect("shared id").to_string();
    let solo = cards
        .iter()
        .find(|c| c["name"] != shared["name"])
        .expect("a second distinctly-named card");
    let solo_id = solo["id"].as_str().expect("solo id").to_string();

    // Two decks each want one copy of `shared`; deck A also wants two of `solo`.
    let deck_a = create_deck(&app, &access, "Needy A").await;
    let deck_b = create_deck(&app, &access, "Needy B").await;
    let a_id = deck_a["id"].as_i64().expect("a id");
    let b_id = deck_b["id"].as_i64().expect("b id");
    let a_section = deck_a["sections"][0]["id"].as_i64().expect("a section");
    let b_section = deck_b["sections"][0]["id"].as_i64().expect("b section");
    add_deck_card(&app, &access, a_id, a_section, &shared_id, 1).await;
    add_deck_card(&app, &access, b_id, b_section, &shared_id, 1).await;
    add_deck_card(&app, &access, a_id, a_section, &solo_id, 2).await;

    // Own one copy of `shared` and none of `solo`.
    own_card(&app, &access, &shared_id, 1).await;

    let (status, headers, body) =
        send(&app, get_with_bearer("/api/decks/mtg/needed", &access)).await;
    assert_eq!(status, StatusCode::OK, "needed failed: {body:?}");
    // Per-user data: never shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));

    // `shared`: wanted twice (once per deck), owned once => need one more, from both decks.
    let shared_entry = needed_entry(&body, &shared_id).expect("shared card is needed");
    assert_eq!(shared_entry["required"], 2);
    assert_eq!(shared_entry["owned"], 1);
    assert_eq!(shared_entry["needed"], 1);
    let decks = shared_entry["decks"].as_array().expect("affected decks");
    assert_eq!(decks.len(), 2, "both decks want the shared card");
    let names: Vec<&str> = decks.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["Needy A", "Needy B"],
        "decks are listed by name"
    );

    // `solo`: wanted twice by deck A, owned none => need two, from one deck.
    let solo_entry = needed_entry(&body, &solo_id).expect("solo card is needed");
    assert_eq!(solo_entry["required"], 2);
    assert_eq!(solo_entry["owned"], 0);
    assert_eq!(solo_entry["needed"], 2);
    assert_eq!(solo_entry["decks"].as_array().unwrap().len(), 1);

    // Owning enough of `shared` drops it from the list; `solo` remains short.
    own_card(&app, &access, &shared_id, 2).await;
    let (_, _, body) = send(&app, get_with_bearer("/api/decks/mtg/needed", &access)).await;
    assert!(
        needed_entry(&body, &shared_id).is_none(),
        "a fully-owned card is not needed"
    );
    assert!(
        needed_entry(&body, &solo_id).is_some(),
        "the still-short card stays listed"
    );
}

#[tokio::test]
async fn needed_cards_printing_mode_reports_the_exact_missing_printing() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "printing-need@example.com", PW).await;

    // The dummy catalog's reprint pair: two printings of one gameplay card.
    let (status, _, printings) = send(
        &app,
        get("/api/games/mtg/cards?name=Dummy%20Reprinted%20Relic&page_size=10"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let printings = printings["data"].as_array().expect("printings");
    assert_eq!(printings.len(), 2, "expected a reprint pair");
    let wanted = printings[0]["id"]
        .as_str()
        .expect("wanted printing")
        .to_string();
    let owned = printings[1]["id"]
        .as_str()
        .expect("owned printing")
        .to_string();

    // A deck wants two of printing A; the collection holds one of printing B (same card).
    let deck = create_deck(&app, &access, "Reprint deck").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");
    add_deck_card(&app, &access, deck_id, section_id, &wanted, 2).await;
    own_card(&app, &access, &owned, 1).await;

    // `card` mode: any printing counts, so owning one (of B) covers one of the two wanted.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/decks/mtg/needed?mode=card", &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let card_entry = needed_entry(&body, &wanted).expect("the card is needed in card mode");
    assert_eq!(card_entry["required"], 2);
    assert_eq!(card_entry["owned"], 1, "the other printing counts as owned");
    assert_eq!(card_entry["needed"], 1);
    // The owned printing isn't wanted, so it never appears.
    assert!(needed_entry(&body, &owned).is_none());

    // `printing` mode: only printing B is owned, so all two of printing A are still short.
    let (status, _, body) = send(
        &app,
        get_with_bearer("/api/decks/mtg/needed?mode=printing", &access),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let printing_entry =
        needed_entry(&body, &wanted).expect("the exact printing is needed in printing mode");
    assert_eq!(printing_entry["required"], 2);
    assert_eq!(
        printing_entry["owned"], 0,
        "the other printing does not count"
    );
    assert_eq!(printing_entry["needed"], 2);
    assert!(needed_entry(&body, &owned).is_none());
}

#[tokio::test]
async fn needed_cards_require_authentication_and_a_read_only_key_may_read() {
    let app = test_app_with_catalog().await;
    let (access, _) = register(&app, "needed-auth@example.com", PW).await;

    // Unauthenticated -> 401, no-store.
    let (status, headers, _) = send(&app, get("/api/decks/mtg/needed")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // A read-only key may read it (it's a read, taking AuthUser not WritableUser).
    let ro = create_key(&app, &access, "read").await;
    let (status, _, body) = send(&app, get_with_bearer("/api/decks/mtg/needed", &ro)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "read-only key read failed: {body:?}"
    );
    assert!(
        body["data"].as_array().expect("data array").is_empty(),
        "a user with no decks needs nothing"
    );

    // An unknown game is a 404.
    let (status, _, _) = send(&app, get_with_bearer("/api/decks/nope/needed", &access)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Give the token's user a username and make `deck_id` public; returns the owner's handle.
async fn publish_deck(app: &TestApp, access: &str, username: &str, deck_id: i64) -> String {
    let (status, _, u) = send(
        app,
        json_with_bearer(
            "PUT",
            "/api/auth/username",
            access,
            json!({ "username": username }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set username failed: {u:?}");
    let handle = u["handle"].as_str().expect("handle").to_string();
    let (status, _, _) = send(
        app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            access,
            json!({ "public": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "enabling public sharing failed");
    handle
}

#[tokio::test]
async fn a_public_deck_can_be_copied_into_your_own_decks() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "copysource@example.com", PW).await;
    let cards = sample_card_ids(&app, 2).await;

    // Alice builds a deck: card A (3 regular + 1 foil), card B (2 regular), in one section.
    let deck = create_deck(&app, &alice, "Alice's Brew").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");
    for (card, qty, foil) in [(&cards[0], 3, 1), (&cards[1], 2, 0)] {
        let (status, _, _) = send(
            &app,
            json_with_bearer(
                "PUT",
                &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
                &alice,
                json!({ "quantity": qty, "foil_quantity": foil, "section_id": section_id }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let handle = publish_deck(&app, &alice, "alice", deck_id).await;

    // Bob copies Alice's public deck into his own decks.
    let (bob, _) = register(&app, "copytarget@example.com", PW).await;
    let (status, headers, copy) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/u/{handle}/decks/{deck_id}/copy"),
            &bob,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "copy failed: {copy:?}");
    // Per-user write: never shared-cached.
    assert_eq!(cache_control(&headers), Some("no-store"));

    // The copy is a brand-new, private deck (a different id), named "(copy)", same 6 copies.
    let copy_id = copy["id"].as_i64().expect("copy id");
    assert_ne!(copy_id, deck_id, "the copy must be a new deck");
    assert_eq!(copy["is_public"], false, "a copy starts private");
    assert!(
        copy["name"].as_str().expect("name").ends_with("(copy)"),
        "the copy name should be suffixed: {:?}",
        copy["name"]
    );
    assert_eq!(copy["cards"].as_array().expect("cards").len(), 2);
    assert_eq!(copy["summary"]["total_cards"], 6);

    // The counts (regular + foil) carried across verbatim.
    let entry = copy["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["card"]["id"] == cards[0].as_str())
        .expect("copied card A");
    assert_eq!(entry["quantity"], 3);
    assert_eq!(entry["foil_quantity"], 1);

    // The copy is really Bob's: he can read it back through the authenticated route, and it
    // shows up in his deck list.
    let (status, _, mine) = send(
        &app,
        get_with_bearer(&format!("/api/decks/mtg/{copy_id}"), &bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bob cannot read his copy: {mine:?}");
    let (status, _, list) = send(&app, get_with_bearer("/api/decks/mtg", &bob)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().expect("decks").len(), 1);

    // Alice's original is untouched: still exactly one deck, still public.
    let (status, _, alice_list) = send(&app, get_with_bearer("/api/decks/mtg", &alice)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alice_list["data"].as_array().expect("decks").len(), 1);
    assert_eq!(alice_list["data"][0]["id"], deck_id);
}

#[tokio::test]
async fn copying_a_deck_requires_a_writable_credential_and_a_public_source() {
    let app = test_app_with_catalog().await;
    let (alice, _) = register(&app, "copyauth-src@example.com", PW).await;
    let card = sample_card_ids(&app, 1).await.remove(0);
    let deck = create_deck(&app, &alice, "Guarded Brew").await;
    let deck_id = deck["id"].as_i64().expect("deck id");
    let section_id = deck["sections"][0]["id"].as_i64().expect("section id");
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/cards/{card}"),
            &alice,
            json!({ "quantity": 1, "foil_quantity": 0, "section_id": section_id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (bob, _) = register(&app, "copyauth-dst@example.com", PW).await;

    // While Alice's deck is still private, nobody can copy it: a uniform 404 (no existence
    // oracle), the same body whether the handle is unknown or the deck is merely private.
    let (status, headers, private_miss) = send(
        &app,
        json_with_bearer(
            "POST",
            &format!("/api/u/nobody-0001/decks/{deck_id}/copy"),
            &bob,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Now publish it, so it's a valid, copyable source for the credential checks below.
    let handle = publish_deck(&app, &alice, "guardian", deck_id).await;
    let uri = format!("/api/u/{handle}/decks/{deck_id}/copy");

    // Unauthenticated -> 401, no-store (a write, never shared-cached).
    let (status, headers, _) = send(&app, json_post(&uri, json!({}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // A read-only API key is a valid credential but the wrong scope -> 403.
    let ro = create_key(&app, &bob, "read").await;
    let (status, _, _) = send(&app, json_with_bearer("POST", &uri, &ro, json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A private source (Alice disables sharing) 404s again, matching the unknown-handle body.
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "PUT",
            &format!("/api/decks/mtg/{deck_id}/visibility"),
            &alice,
            json!({ "public": false }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, now_private) =
        send(&app, json_with_bearer("POST", &uri, &bob, json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(now_private["error"], private_miss["error"]);
}
