//! The table's randomness: shuffles, dice, coins and card-instance ids.
//!
//! Nothing here is seeded on the wire (unlike the goldfish / booster opener, whose
//! `shared/rng.rs` SplitMix64 stream *is* a contract), so `rand` from OS entropy is fine —
//! a shuffle only has to be fair, never reproducible.

use std::collections::BTreeMap;

use rand::{RngExt, SeedableRng};

use crate::play::types::{CardId, CardInstance};

/// One room's RNG. Not persisted; a hydrated room gets a fresh one.
pub struct PlayRng {
    inner: rand::rngs::StdRng,
}

impl PlayRng {
    /// Seeded from OS entropy.
    pub fn from_entropy() -> Self {
        Self {
            inner: rand::make_rng(),
        }
    }

    /// Deterministic — for tests, and for anyone who wants a reproducible table.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn seeded(seed: u64) -> Self {
        Self {
            inner: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }

    /// Uniform in `0..n` (`n >= 1`).
    pub fn below(&mut self, n: u32) -> u32 {
        if n <= 1 {
            return 0;
        }
        self.inner.random_range(0..n)
    }

    /// Fisher–Yates in place.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        let len = items.len();
        if len < 2 {
            return;
        }
        // Walk down from the last index, swapping each slot with a uniformly chosen slot at
        // or below it — the textbook inside-out-free Fisher–Yates, written out rather than
        // pulled from `rand::seq` so the one source of randomness stays this struct.
        for i in (1..len).rev() {
            let j = self.below_usize(i + 1);
            items.swap(i, j);
        }
    }

    /// A fresh random card-instance id not already in `taken` (never 0).
    pub fn card_id(&mut self, taken: &BTreeMap<CardId, CardInstance>) -> CardId {
        loop {
            let id: CardId = self.inner.random();
            if id != 0 && !taken.contains_key(&id) {
                return id;
            }
        }
    }

    /// Uniform in `0..n` for a length (`n >= 1`); the table never holds more than a few
    /// thousand cards, but the cast is done once, here, rather than at every call site.
    fn below_usize(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        if n <= u32::MAX as usize {
            self.below(n as u32) as usize
        } else {
            self.inner.random_range(0..n)
        }
    }
}

impl Default for PlayRng {
    fn default() -> Self {
        Self::from_entropy()
    }
}

impl std::fmt::Debug for PlayRng {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlayRng")
    }
}
