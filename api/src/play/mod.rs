//! **Play** — the online manual table (the second tool beside the life counter).
//!
//! This module is the *pure* engine: the authoritative table state, the reducer that applies
//! a seat's action to it, and the per-viewer views that keep hidden information hidden. It
//! knows nothing of the database, the socket or axum — those live in
//! `handlers/tools/play/`, which owns rooms/seats/decks (REST + persistence) and the
//! WebSocket loop, and calls in here. Keeping the engine free of I/O is what makes every
//! rule unit-testable with a `RoomState` literal.
//!
//! Read [`types`] first — it is the wire contract (ts-rs exported as `Play*`). Then
//! [`engine`] (the reducer), [`view`] (per-viewer filtering) and [`rng`].

pub mod engine;
pub mod rng;
pub mod types;
pub mod view;

#[cfg(test)]
mod tests;
