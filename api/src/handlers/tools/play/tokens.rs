//! The two opaque strings the play tool mints: a room's **invite code** and a seat's
//! **token**.
//!
//! They look alike and are not: the code is short, shareable and public (it goes in a URL
//! anyone at the table pastes into a group chat), so it is *not* a credential and the routes
//! keyed by it authorize on something else. The seat token is the credential — 32 CSPRNG
//! bytes through [`auth::secret`](crate::auth::secret), stored only as its SHA-256 hex
//! digest, shown once by the join response, and presented in `X-Play-Seat`.
//!
//! Keeping both here (rather than inline in `rooms.rs` / `seats.rs`) is what stops the
//! alphabet, the length and the hash-only rule from being restated in three places.

use axum::http::HeaderMap;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::auth::secret::{generate_secret, sha256_hex};
use crate::entities::play_seat;
use crate::entities::prelude::PlaySeat;
use crate::error::AppError;

/// The room-code alphabet: upper-case letters and digits with the four ambiguous glyphs
/// (`0`/`O`, `1`/`I`) removed, so a code read aloud or off a screen can't be mistyped into
/// a *different* live room.
pub(crate) const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Room codes are six characters — 32^6 ≈ 1.07e9 combinations, which is plenty of headroom
/// against the handful of live rooms an instance ever holds while staying easy to dictate.
pub(crate) const CODE_LENGTH: usize = 6;
/// How many codes the create path will mint before giving up. A collision needs the unique
/// index to reject an insert, which at any realistic room count is vanishingly unlikely —
/// this only exists so the loop is bounded.
pub(crate) const CODE_ATTEMPTS: usize = 8;

/// The header a seat-scoped call carries its seat token in. Mirrored by `PLAY_SEAT_HEADER`
/// in `web/src/lib/api/play.ts`.
pub(crate) const SEAT_HEADER: &str = "x-play-seat";

/// A fresh room code. Uniqueness is the unique index's job, not this function's — see
/// [`CODE_ATTEMPTS`].
pub(crate) fn generate_code() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..CODE_LENGTH)
        .map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

/// Normalise a code from a path segment: codes are stored upper-case, so a link pasted in
/// lower case resolves to the same room.
pub(crate) fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

/// Mint a seat token, returning `(plaintext, sha256_hex)`. Only the digest is ever stored.
pub(crate) fn generate_seat_token() -> (String, String) {
    let token = generate_secret();
    let hash = sha256_hex(&token);
    (token, hash)
}

/// The seat token presented by this request, if any.
pub(crate) fn seat_token_from(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SEAT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// The one rejection every seat-scoped route answers with when the token is missing or
/// doesn't match. Deliberately identical in both cases and identical to the one a *foreign*
/// seat id produces, so it reveals nothing about which seats exist.
pub(crate) fn invalid_seat_token() -> AppError {
    AppError::Unauthorized("invalid_seat_token".to_string())
}

/// Load the seat `seat_id` of `room_id`, proving the caller holds it by seat token.
///
/// The comparison is on the **hash**, so the plaintext is never stored or logged. A seat in
/// another room, a missing token and a wrong token all answer the same `401`.
pub(crate) async fn authorize_seat<C: sea_orm::ConnectionTrait>(
    db: &C,
    room_id: i32,
    seat_id: i32,
    token: Option<&str>,
) -> Result<play_seat::Model, AppError> {
    let token = token.ok_or_else(invalid_seat_token)?;
    let seat = PlaySeat::find_by_id(seat_id)
        .filter(play_seat::Column::RoomId.eq(room_id))
        .one(db)
        .await?
        .ok_or_else(invalid_seat_token)?;
    if seat.token_hash != sha256_hex(token) {
        return Err(invalid_seat_token());
    }
    Ok(seat)
}

/// Resolve a seat of `room_id` from a bare token (no seat id) — what the socket's `hello`
/// and a token-carrying `join` do. `None` when nothing matches.
pub(crate) async fn seat_by_token<C: sea_orm::ConnectionTrait>(
    db: &C,
    room_id: i32,
    token: &str,
) -> Result<Option<play_seat::Model>, AppError> {
    Ok(PlaySeat::find()
        .filter(play_seat::Column::RoomId.eq(room_id))
        .filter(play_seat::Column::TokenHash.eq(sha256_hex(token)))
        .one(db)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_unambiguous_characters() {
        for _ in 0..64 {
            let code = generate_code();
            assert_eq!(code.chars().count(), CODE_LENGTH);
            assert!(
                code.bytes().all(|b| CODE_ALPHABET.contains(&b)),
                "{code} escaped the alphabet"
            );
            // The four glyphs a human confuses are simply not in the alphabet.
            assert!(!code.contains(['0', 'O', '1', 'I']), "{code} is ambiguous");
        }
    }

    #[test]
    fn codes_normalize_case_and_whitespace() {
        assert_eq!(normalize_code(" abc23x "), "ABC23X");
    }

    #[test]
    fn seat_tokens_are_distinct_and_stored_only_as_a_digest() {
        let (a, a_hash) = generate_seat_token();
        let (b, _) = generate_seat_token();
        assert_ne!(a, b);
        assert_eq!(a_hash, sha256_hex(&a));
        assert_ne!(a_hash, a, "the stored value is the digest, not the token");
    }
}
