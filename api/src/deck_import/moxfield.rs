//! Moxfield deck URL parsing and gated single-object API fetch.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::{StatusCode, header};
use serde::Deserialize;

use crate::collection_import::archidekt::backoff_after;
use crate::collection_import::moxfield::{is_foil_finish, valid_id};
use crate::collection_import::{ImportError, Provider, ProviderContext};

use super::{DeckCardRow, MAX_DECK_IMPORT_ROWS, ParsedDeck};

const API_BASE: &str = "https://api2.moxfield.com/v3/decks/all";
const MAX_RATE_LIMIT_RETRIES: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Default, Deserialize)]
struct DeckPayload {
    name: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    boards: Boards,
    // Older API variants exposed board maps at the top level. Keeping these aliases is
    // cheap and makes the importer tolerant of either shape.
    #[serde(default)]
    mainboard: HashMap<String, CardEntry>,
    #[serde(default)]
    sideboard: HashMap<String, CardEntry>,
    #[serde(default)]
    commanders: HashMap<String, CardEntry>,
    #[serde(default)]
    maybeboard: HashMap<String, CardEntry>,
    #[serde(default)]
    companions: HashMap<String, CardEntry>,
    #[serde(default, rename = "signatureSpells")]
    signature_spells: HashMap<String, CardEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct Boards {
    #[serde(default)]
    mainboard: Board,
    #[serde(default)]
    sideboard: Board,
    #[serde(default)]
    commanders: Board,
    #[serde(default)]
    maybeboard: Board,
    #[serde(default)]
    companions: Board,
    #[serde(default, rename = "signatureSpells")]
    signature_spells: Board,
}

#[derive(Debug, Default, Deserialize)]
struct Board {
    #[serde(default)]
    cards: HashMap<String, CardEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardEntry {
    #[serde(default = "one")]
    quantity: i32,
    #[serde(default)]
    finish: Option<String>,
    #[serde(default)]
    is_foil: bool,
    card: MoxfieldCard,
}

fn one() -> i32 {
    1
}

#[derive(Debug, Deserialize)]
struct MoxfieldCard {
    name: String,
    #[serde(default, alias = "scryfallId")]
    scryfall_id: Option<String>,
    #[serde(default, alias = "setCode")]
    set: Option<String>,
    #[serde(default, alias = "collectorNumber")]
    cn: Option<String>,
}

/// Parse a bare base64url id or the segment immediately after `/decks/` in a Moxfield
/// URL. Collection and binder URLs remain invalid.
pub fn parse_deck_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains('/') {
        return valid_id(trimmed).then(|| trimmed.to_string());
    }
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    segments
        .by_ref()
        .find(|segment| segment.eq_ignore_ascii_case("decks"))?;
    let id = segments.next()?;
    valid_id(id).then(|| id.to_string())
}

pub async fn fetch_deck(
    ctx: &ProviderContext<'_>,
    deck_id: &str,
) -> Result<ParsedDeck, ImportError> {
    let limiter = ctx.limiters.for_provider(Provider::Moxfield);
    let mut retries = 0u32;
    let payload: DeckPayload = loop {
        limiter.acquire().await;
        let mut request = ctx
            .http
            .get(format!("{API_BASE}/{deck_id}"))
            .header(header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT);
        if let Some(user_agent) = ctx.settings.moxfield_user_agent.as_deref() {
            request = request.header(header::USER_AGENT, user_agent);
        }
        let response = request
            .send()
            .await
            .map_err(|e| fetch_failure(ctx, format!("request to Moxfield failed: {e}")))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            if retries >= MAX_RATE_LIMIT_RETRIES {
                return Err(ImportError::RateLimited);
            }
            retries += 1;
            limiter.back_off(backoff_after(response.headers())).await;
            continue;
        }
        if status == StatusCode::FORBIDDEN {
            return Err(ImportError::ProviderDenied(
                "Moxfield declined the deck request: their API only serves approved clients. \
                 The server operator must configure an approved MOXFIELD_USER_AGENT, or you \
                 can upload a Moxfield deck export instead."
                    .to_string(),
            ));
        }
        if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
            return Err(ImportError::DeckNotFound(deck_id.to_string()));
        }
        let response = response
            .error_for_status()
            .map_err(|e| ImportError::Upstream(format!("Moxfield returned an error: {e}")))?;
        let body = response
            .text()
            .await
            .map_err(|e| fetch_failure(ctx, format!("couldn't read Moxfield response: {e}")))?;
        break serde_json::from_str(&body).map_err(|e| {
            ImportError::Upstream(format!("couldn't parse the Moxfield deck response: {e}"))
        })?;
    };

    normalize_payload(payload)
}

/// Normalize the supported Moxfield payload shapes without performing network I/O.
/// The v3 API nests boards under `boards`; older variants exposed maps at the top level.
/// Prefer each nested board independently, falling back to its legacy counterpart when
/// absent so a partially nested response cannot silently discard a board.
fn normalize_payload(payload: DeckPayload) -> Result<ParsedDeck, ImportError> {
    let DeckPayload {
        name,
        format,
        boards,
        mainboard,
        sideboard,
        commanders,
        maybeboard,
        companions,
        signature_spells,
    } = payload;
    let Boards {
        mainboard: nested_mainboard,
        sideboard: nested_sideboard,
        commanders: nested_commanders,
        maybeboard: nested_maybeboard,
        companions: nested_companions,
        signature_spells: nested_signature_spells,
    } = boards;

    let mut rows = Vec::new();
    append_board_with_fallback(&mut rows, "Mainboard", nested_mainboard, mainboard);
    append_board_with_fallback(&mut rows, "Sideboard", nested_sideboard, sideboard);
    append_board_with_fallback(&mut rows, "Commander", nested_commanders, commanders);
    append_board_with_fallback(&mut rows, "Maybeboard", nested_maybeboard, maybeboard);
    append_board_with_fallback(&mut rows, "Companion", nested_companions, companions);
    append_board_with_fallback(
        &mut rows,
        "Signature Spells",
        nested_signature_spells,
        signature_spells,
    );
    if rows.len() > MAX_DECK_IMPORT_ROWS {
        return Err(ImportError::TooLarge {
            count: rows.len(),
            max: MAX_DECK_IMPORT_ROWS,
        });
    }

    Ok(ParsedDeck {
        provider: Provider::Moxfield,
        name,
        format: format.map(pretty_format),
        rows,
    })
}

fn append_board_with_fallback(
    rows: &mut Vec<DeckCardRow>,
    section: &str,
    nested: Board,
    legacy: HashMap<String, CardEntry>,
) {
    append_board(
        rows,
        section,
        if nested.cards.is_empty() {
            legacy
        } else {
            nested.cards
        },
    );
}

fn append_board(rows: &mut Vec<DeckCardRow>, section: &str, cards: HashMap<String, CardEntry>) {
    for entry in cards.into_values().filter(|entry| entry.quantity > 0) {
        rows.push(DeckCardRow {
            section: section.to_string(),
            card_name: entry.card.name,
            external_card_id: entry.card.scryfall_id,
            set_code: entry.card.set.map(|set| set.to_ascii_lowercase()),
            collector_number: entry.card.cn,
            foil: is_foil_finish(entry.finish.as_deref(), entry.is_foil),
            quantity: entry.quantity,
        });
    }
}

fn pretty_format(format: String) -> String {
    format
        .split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fetch_failure(ctx: &ProviderContext<'_>, detail: String) -> ImportError {
    if ctx.settings.moxfield_user_agent.is_none() {
        return ImportError::ProviderDenied(
            "Moxfield didn't answer the deck request in time. Configure an approved \
             MOXFIELD_USER_AGENT, or upload a Moxfield deck export instead."
                .to_string(),
        );
    }
    ImportError::Upstream(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "4xUdq-66IEKK6X53bhUS8Q";

    #[test]
    fn parses_only_deck_sources() {
        assert_eq!(parse_deck_id(ID).as_deref(), Some(ID));
        assert_eq!(
            parse_deck_id(&format!("https://moxfield.com/decks/{ID}/?x=1")).as_deref(),
            Some(ID)
        );
        assert_eq!(
            parse_deck_id(&format!("https://moxfield.com/collection/{ID}")),
            None
        );
    }

    #[test]
    fn normalizes_every_nested_board_including_signature_spells() {
        let payload: DeckPayload = serde_json::from_str(
            r#"{
              "name":"Deck", "format":"commander",
              "boards":{
                "mainboard":{"cards":{"a":{"quantity":2,"finish":"foil","card":{"name":"Main","scryfallId":"uid-main"}}}},
                "sideboard":{"cards":{"b":{"card":{"name":"Side","scryfallId":"uid-side"}}}},
                "commanders":{"cards":{"c":{"card":{"name":"Commander","scryfallId":"uid-commander"}}}},
                "maybeboard":{"cards":{"d":{"card":{"name":"Maybe","scryfallId":"uid-maybe"}}}},
                "companions":{"cards":{"e":{"card":{"name":"Companion","scryfallId":"uid-companion"}}}},
                "signatureSpells":{"cards":{"f":{"card":{"name":"Signature","scryfallId":"uid-signature"}}}}
              }
            }"#,
        )
        .expect("payload");
        let parsed = normalize_payload(payload).expect("normalize");
        assert_eq!(parsed.format.as_deref(), Some("Commander"));
        assert_eq!(parsed.rows.len(), 6);
        assert_eq!(
            parsed
                .rows
                .iter()
                .map(|row| row.section.as_str())
                .collect::<Vec<_>>(),
            [
                "Mainboard",
                "Sideboard",
                "Commander",
                "Maybeboard",
                "Companion",
                "Signature Spells",
            ]
        );
        assert_eq!(parsed.rows[0].quantity, 2);
        assert!(parsed.rows[0].foil);
    }

    #[test]
    fn falls_back_per_board_to_legacy_top_level_maps() {
        let payload: DeckPayload = serde_json::from_str(
            r#"{
              "name":"Legacy",
              "boards":{"mainboard":{"cards":{"nested":{"card":{"name":"Nested main","scryfallId":"uid-main"}}}}},
              "mainboard":{"legacy-main":{"card":{"name":"Ignored duplicate main","scryfallId":"uid-old-main"}}},
              "sideboard":{"side":{"card":{"name":"Side","scryfallId":"uid-side"}}},
              "commanders":{"commander":{"card":{"name":"Commander","scryfallId":"uid-commander"}}},
              "maybeboard":{"maybe":{"card":{"name":"Maybe","scryfallId":"uid-maybe"}}},
              "companions":{"companion":{"card":{"name":"Companion","scryfallId":"uid-companion"}}},
              "signatureSpells":{"signature":{"card":{"name":"Signature","scryfallId":"uid-signature"}}}
            }"#,
        )
        .expect("payload");
        let parsed = normalize_payload(payload).expect("normalize");
        assert_eq!(parsed.rows.len(), 6);
        assert_eq!(parsed.rows[0].card_name, "Nested main");
        assert!(
            parsed
                .rows
                .iter()
                .any(|row| row.section == "Signature Spells")
        );
        assert!(
            !parsed
                .rows
                .iter()
                .any(|row| row.card_name == "Ignored duplicate main")
        );
    }

    #[test]
    fn rejects_an_offline_payload_above_the_deck_row_cap() {
        let cards = (0..=MAX_DECK_IMPORT_ROWS)
            .map(|index| {
                (
                    index.to_string(),
                    CardEntry {
                        quantity: 1,
                        finish: None,
                        is_foil: false,
                        card: MoxfieldCard {
                            name: format!("Card {index}"),
                            scryfall_id: Some(format!("uid-{index}")),
                            set: None,
                            cn: None,
                        },
                    },
                )
            })
            .collect();
        let payload = DeckPayload {
            name: "Too large".into(),
            boards: Boards {
                mainboard: Board { cards },
                ..Default::default()
            },
            ..Default::default()
        };
        let err = normalize_payload(payload).expect_err("oversized payload");
        assert!(matches!(
            err,
            ImportError::TooLarge { count, max }
                if count == MAX_DECK_IMPORT_ROWS + 1 && max == MAX_DECK_IMPORT_ROWS
        ));
    }
}
