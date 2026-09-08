//! The one seeded generator every **stateless, seeded** read shares: the deck goldfish
//! (`handlers::decks::analysis::goldfish`) and the sealed pack opener
//! (`handlers::catalog::boosters`). Both put a seed on the wire and promise that the same
//! URL reproduces the same result, which is why neither uses the `rand` crate: `rand`'s
//! generators explicitly do not promise a stable stream across versions, so a dependency
//! bump would silently invalidate every shared hand and every shared opening.
//!
//! SplitMix64 (Steele, Lea & Flood) is ~10 lines, specified by its constants, and can be
//! reimplemented in any client that wants to predict a draw. It is the whole generator here
//! — the mixing function *is* the output. Each caller derives its own initial state from
//! its seed (the goldfish mixes the mulligan count in, the opener the pack ordinal) and
//! warms it once so a low seed doesn't start from a near-zero mix; those derivations are
//! part of each read's wire contract and stay with the read, not here.
//!
//! Extracted from the goldfish verbatim when the opener became its second user: the
//! constants and the exact operation order are what a client reimplementing the stream
//! depends on, so there must be exactly one copy.

/// One SplitMix64 step: advance `state` by the golden-ratio increment and return the mixed
/// output. Fixed constants, no library, identical output everywhere.
pub(crate) fn split_mix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference stream from the SplitMix64 paper / `java.util.SplittableRandom`, seed 0:
    /// the first outputs are `e220a8397b1dcdaf`, `6e789e6aa1b965f4`, `06c45d188009454f`. Pinning
    /// them pins the wire contract — every shared goldfish hand and pack opening ever minted
    /// depends on these exact values.
    #[test]
    fn matches_the_reference_stream() {
        let mut state = 0u64;
        assert_eq!(split_mix64(&mut state), 0xe220_a839_7b1d_cdaf);
        assert_eq!(split_mix64(&mut state), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(split_mix64(&mut state), 0x06c4_5d18_8009_454f);
    }

    #[test]
    fn distinct_states_diverge_and_the_same_state_repeats() {
        let mut a = 42u64;
        let mut b = 42u64;
        let mut c = 43u64;
        let first_a = split_mix64(&mut a);
        assert_eq!(first_a, split_mix64(&mut b));
        assert_ne!(first_a, split_mix64(&mut c));
    }
}
