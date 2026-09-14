//! The table's randomness: shuffles, dice, coins and card-instance ids.
//!
//! Nothing here is seeded on the wire (unlike the goldfish / booster opener, whose
//! `shared/rng.rs` SplitMix64 stream *is* a contract), so `rand` from OS entropy is fine —
//! a shuffle only has to be fair, never reproducible.

use std::collections::BTreeMap;

use crate::play::types::{CardId, CardInstance};

/// One room's RNG. Not persisted; a hydrated room gets a fresh one.
pub struct PlayRng {
    inner: rand::rngs::StdRng,
}

impl PlayRng {
    /// Seeded from OS entropy.
    pub fn from_entropy() -> Self {
        todo!("engine implementer")
    }

    /// Deterministic, for tests only.
    pub fn seeded(seed: u64) -> Self {
        let _ = seed;
        todo!("engine implementer")
    }

    /// Uniform in `0..n` (`n >= 1`).
    pub fn below(&mut self, n: u32) -> u32 {
        let _ = n;
        todo!("engine implementer")
    }

    /// Fisher–Yates in place.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        let _ = items;
        todo!("engine implementer")
    }

    /// A fresh random card-instance id not already in `taken` (never 0).
    pub fn card_id(&mut self, taken: &BTreeMap<CardId, CardInstance>) -> CardId {
        let _ = taken;
        todo!("engine implementer")
    }
}
