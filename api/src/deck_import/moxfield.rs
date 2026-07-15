//! Moxfield deck URL parsing and gated single-object API fetch.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::{StatusCode, header};
use serde::Deserialize;

use crate::collection_import::archidekt::backoff_after;
use crate::collection_import::moxfield::{is_foil_finish, valid_id};
use crate::collection_import::{ImportError, MAX_IMPORT_ROWS, Provider, ProviderContext};

use super::{DeckCardRow, ParsedDeck};

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

    let has_nested_boards = !payload.boards.mainboard.cards.is_empty()
        || !payload.boards.sideboard.cards.is_empty()
        || !payload.boards.commanders.cards.is_empty()
        || !payload.boards.maybeboard.cards.is_empty()
        || !payload.boards.companions.cards.is_empty();
    let mut rows = Vec::new();
    append_board(&mut rows, "Mainboard", payload.boards.mainboard.cards);
    append_board(&mut rows, "Sideboard", payload.boards.sideboard.cards);
    append_board(&mut rows, "Commander", payload.boards.commanders.cards);
    append_board(&mut rows, "Maybeboard", payload.boards.maybeboard.cards);
    append_board(&mut rows, "Companion", payload.boards.companions.cards);
    if !has_nested_boards {
        append_board(&mut rows, "Mainboard", payload.mainboard);
        append_board(&mut rows, "Sideboard", payload.sideboard);
        append_board(&mut rows, "Commander", payload.commanders);
    }
    if rows.len() > MAX_IMPORT_ROWS {
        return Err(ImportError::TooLarge {
            count: rows.len(),
            max: MAX_IMPORT_ROWS,
        });
    }

    Ok(ParsedDeck {
        provider: Provider::Moxfield,
        name: payload.name,
        format: payload.format.map(pretty_format),
        rows,
    })
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
    fn deserializes_nested_board_shape() {
        let payload: DeckPayload = serde_json::from_str(
            r#"{
              "name":"Deck", "format":"commander",
              "boards":{"mainboard":{"cards":{"a":{"quantity":2,"finish":"foil","card":{"name":"Sol Ring","scryfall_id":"uid","set":"c21","cn":"263"}}}}}
            }"#,
        )
        .expect("payload");
        let entry = payload.boards.mainboard.cards.get("a").expect("entry");
        assert_eq!(entry.quantity, 2);
        assert_eq!(entry.card.scryfall_id.as_deref(), Some("uid"));
    }
}
