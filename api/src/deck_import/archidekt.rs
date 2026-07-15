//! Archidekt deck URL parsing and single-object API fetch.

use reqwest::{StatusCode, header};
use serde::Deserialize;

use crate::collection_import::archidekt::{backoff_after, is_foil_finish};
use crate::collection_import::{ImportError, Provider, ProviderContext};

use super::{DeckCardRow, MAX_DECK_IMPORT_ROWS, ParsedDeck};

const API_BASE: &str = "https://archidekt.com/api/decks";
const MAX_RATE_LIMIT_RETRIES: u32 = 5;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeckPayload {
    name: String,
    #[serde(default)]
    deck_format: Option<i32>,
    #[serde(default)]
    cards: Vec<DeckRow>,
}

#[derive(Debug, Deserialize)]
struct DeckRow {
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    quantity: i32,
    #[serde(default)]
    foil: bool,
    #[serde(default)]
    modifier: Option<String>,
    card: RowCard,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RowCard {
    uid: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    oracle_card: Option<OracleCard>,
}

#[derive(Debug, Deserialize)]
struct OracleCard {
    name: String,
}

/// Parse a bare numeric id or the segment immediately after `/decks/` in an Archidekt
/// deck URL. Collection URLs are deliberately rejected (this is a sibling parser, not a
/// relaxation of `parse_collection_id`).
pub fn parse_deck_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Some(trimmed.to_string());
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
    id.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| id.to_string())
}

/// Fetch and normalize one public Archidekt deck. The endpoint is one object (not the
/// collection pagination API), but it shares the provider limiter and 429 backoff policy.
pub async fn fetch_deck(
    ctx: &ProviderContext<'_>,
    deck_id: &str,
) -> Result<ParsedDeck, ImportError> {
    fetch_deck_from_base(ctx, deck_id, API_BASE).await
}

async fn fetch_deck_from_base(
    ctx: &ProviderContext<'_>,
    deck_id: &str,
    api_base: &str,
) -> Result<ParsedDeck, ImportError> {
    let limiter = ctx.limiters.for_provider(Provider::Archidekt);
    let mut retries = 0u32;
    let payload: DeckPayload = loop {
        limiter.acquire().await;
        let response = ctx
            .http
            .get(format!("{api_base}/{deck_id}/"))
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| ImportError::Upstream(format!("request to Archidekt failed: {e}")))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            if retries >= MAX_RATE_LIMIT_RETRIES {
                return Err(ImportError::RateLimited);
            }
            retries += 1;
            limiter.back_off(backoff_after(response.headers())).await;
            continue;
        }
        if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
            return Err(ImportError::DeckNotFound(deck_id.to_string()));
        }
        let response = response
            .error_for_status()
            .map_err(|e| ImportError::Upstream(format!("Archidekt returned an error: {e}")))?;
        break response.json().await.map_err(|e| {
            ImportError::Upstream(format!("couldn't parse the Archidekt deck response: {e}"))
        })?;
    };

    if payload.cards.len() > MAX_DECK_IMPORT_ROWS {
        return Err(ImportError::TooLarge {
            count: payload.cards.len(),
            max: MAX_DECK_IMPORT_ROWS,
        });
    }
    let rows = payload
        .cards
        .into_iter()
        .filter(|row| row.quantity > 0)
        .map(|row| {
            // Archidekt's first category is the primary grouping. Copying a quantity into
            // every secondary category would multiply the deck's actual card count.
            let section = row
                .categories
                .first()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("Mainboard")
                .to_string();
            let card_name = row
                .card
                .display_name
                .filter(|name| !name.trim().is_empty())
                .or_else(|| row.card.oracle_card.map(|oracle| oracle.name))
                .unwrap_or_else(|| row.card.uid.clone());
            DeckCardRow {
                section,
                card_name,
                external_card_id: Some(row.card.uid),
                set_code: None,
                collector_number: None,
                foil: is_foil_finish(row.foil, row.modifier.as_deref()),
                quantity: row.quantity,
            }
        })
        .collect();

    Ok(ParsedDeck {
        provider: Provider::Archidekt,
        name: payload.name,
        format: archidekt_format(payload.deck_format),
        rows,
    })
}

fn archidekt_format(id: Option<i32>) -> Option<String> {
    let label = match id? {
        1 => "Standard",
        2 => "Modern",
        3 => "Commander",
        4 => "Legacy",
        5 => "Vintage",
        6 => "Pauper",
        7 => "Pioneer",
        _ => return None,
    };
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    use crate::collection_import::rate_limit::ProviderLimiters;
    use crate::collection_import::{ProgressReporter, ProviderSettings};

    use super::*;

    #[test]
    fn parses_deck_sources_but_not_collection_sources() {
        assert_eq!(parse_deck_id("12345").as_deref(), Some("12345"));
        assert_eq!(
            parse_deck_id("https://archidekt.com/decks/12345/name?x=1#cards").as_deref(),
            Some("12345")
        );
        assert_eq!(
            parse_deck_id("https://archidekt.com/collection/v2/12345"),
            None
        );
        assert_eq!(parse_deck_id("https://archidekt.com/decks/nope"), None);
    }

    #[test]
    fn deserializes_real_deck_shape() {
        let payload: DeckPayload = serde_json::from_str(
            r#"{
                "name":"Imported EDH", "deckFormat":3,
                "cards":[{
                    "categories":["Commander","Creatures"], "quantity":1,
                    "foil":false, "modifier":"Foil",
                    "card":{"uid":"uid-a","displayName":null,"oracleCard":{"name":"Atraxa"}}
                }]
            }"#,
        )
        .expect("payload");
        assert_eq!(payload.name, "Imported EDH");
        assert_eq!(payload.cards[0].categories[0], "Commander");
        assert!(is_foil_finish(
            payload.cards[0].foil,
            payload.cards[0].modifier.as_deref()
        ));
    }

    #[tokio::test]
    async fn fetches_and_normalizes_a_provider_response() {
        let app = Router::new().route(
            "/api/decks/{id}/",
            get(|| async {
                Json(json!({
                    "name": "Provider fixture",
                    "deckFormat": 3,
                    "cards": [{
                        "categories": ["Ramp", "Artifacts"],
                        "quantity": 2,
                        "foil": true,
                        "modifier": "Foil",
                        "card": {
                            "uid": "fixture-scryfall-id",
                            "displayName": "Sol Ring"
                        }
                    }]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind provider fixture");
        let address = listener.local_addr().expect("provider fixture address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let http = reqwest::Client::new();
        let limiters = ProviderLimiters::new(u32::MAX, u32::MAX);
        let settings = ProviderSettings::default();
        let progress = ProgressReporter::default();
        let context = ProviderContext {
            http: &http,
            limiters: &limiters,
            settings: &settings,
            progress: &progress,
        };
        let parsed =
            fetch_deck_from_base(&context, "12345", &format!("http://{address}/api/decks"))
                .await
                .expect("fetch fixture");

        assert_eq!(parsed.name, "Provider fixture");
        assert_eq!(parsed.format.as_deref(), Some("Commander"));
        assert_eq!(parsed.rows.len(), 1);
        assert_eq!(parsed.rows[0].section, "Ramp");
        assert_eq!(
            parsed.rows[0].external_card_id.as_deref(),
            Some("fixture-scryfall-id")
        );
        assert_eq!(parsed.rows[0].quantity, 2);
        assert!(parsed.rows[0].foil);
    }
}
