//! Pretty terminal progress for the TCGCSV startup syncs.
//!
//! Two long-running TCGCSV operations run on boot and, like the Scryfall card
//! import, sit there with no feedback otherwise:
//!
//! - the **sealed-product sweep** ([`super::ingest`]) walks ~900 TCGplayer groups,
//!   two paced requests each, so a full sweep is a couple of minutes; and
//! - the one-time **historic price backfill** ([`super::backfill`]) walks every daily
//!   price archive since 2024-02-08 (downloading + decompressing one `7z` per day),
//!   which on a first boot is hundreds of days.
//!
//! Both are **determinate** — the group count and the archive-day count are known up
//! front — so each drives a count bar with a running tally and an ETA. Like
//! [`crate::scryfall`]'s import bar this wraps a [`tracing_indicatif`] span rather than
//! a bare [`indicatif::ProgressBar`], so concurrent log lines never clobber it and a
//! non-TTY stderr (CI, redirected output) renders nothing — silent there, logs
//! untouched.
//!
//! The span is named `tcgcsv_sync`; `main.rs` scopes the `IndicatifLayer` to it
//! (alongside the Scryfall span) so unrelated spans don't each sprout a bar.

use indicatif::{HumanCount, ProgressStyle};
use tracing::{Span, info_span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

/// The span name the sync bars attach to; `main.rs` filters the `IndicatifLayer` to
/// this so only these syncs (and the Scryfall import) get a bar. Must stay in sync
/// with the literal passed to `info_span!` below (`tracing` requires a literal there);
/// `span_name_matches_constant` guards against drift.
pub const SPAN_NAME: &str = "tcgcsv_sync";

/// A live progress bar for one TCGCSV sync — either the sealed-product sweep or the
/// historic price backfill. Dropping it closes the span and removes the bar, so an
/// early `?` return cleans up automatically.
pub struct SyncProgress {
    span: Span,
    /// Fixed leading label, e.g. `"Syncing sealed products"`.
    label: &'static str,
    /// Noun for the running tally shown after the label, e.g. `"products"`.
    unit: &'static str,
}

impl SyncProgress {
    /// Begin the sealed-product sweep bar, determinate over `total_groups` groups.
    pub fn start_products(total_groups: u64) -> Self {
        Self::start(total_groups, "Syncing sealed products", "products")
    }

    /// Begin the historic price-backfill bar, determinate over `total_days` archive
    /// days (every candidate day in the walk, including the ones with no archive).
    pub fn start_backfill(total_days: u64) -> Self {
        Self::start(total_days, "Backfilling historic prices", "price rows")
    }

    fn start(total: u64, label: &'static str, unit: &'static str) -> Self {
        let span = info_span!("tcgcsv_sync");
        span.pb_set_style(&bar_style());
        span.pb_set_length(total);
        span.pb_set_position(0);
        span.pb_set_message(label);
        // Show the bar now and keep it visible for the span's whole lifetime, without
        // having to hold an `enter()` guard across the sync's awaits.
        span.pb_start();
        Self { span, label, unit }
    }

    /// Advance the bar by one step (a group swept / an archive day processed).
    pub fn inc(&self) {
        self.span.pb_inc(1);
    }

    /// Update the running tally shown after the label (imported products / rows).
    pub fn set_count(&self, count: u64) {
        self.span.pb_set_message(&format!(
            "{} · {} {}",
            self.label,
            HumanCount(count),
            self.unit
        ));
    }
}

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} {msg}\n   {elapsed_precise} ▕{bar:35.cyan/blue}▏ \
         {human_pos}/{human_len} · ETA {eta}",
    )
    .expect("valid progress-bar template")
    .progress_chars("█▉▊▋▌▍▎▏ ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_indicatif::IndicatifLayer;
    use tracing_subscriber::layer::SubscriberExt;

    /// Drives both bars under a real (hidden, non-TTY) `IndicatifLayer`. This both
    /// exercises the `IndicatifSpanExt` calls end to end and asserts the
    /// `ProgressStyle` template parses (the `expect` would otherwise panic only on a
    /// live sync).
    #[test]
    fn drives_both_bars_without_panicking() {
        let subscriber = tracing_subscriber::registry().with(IndicatifLayer::new());
        tracing::subscriber::with_default(subscriber, || {
            let products = SyncProgress::start_products(900);
            products.inc();
            products.set_count(120);
            products.inc();

            let backfill = SyncProgress::start_backfill(500);
            backfill.inc();
            backfill.set_count(10_000);

            drop(products);
            drop(backfill);
        });
    }

    /// The `IndicatifLayer` filter in `main.rs` matches on `SPAN_NAME`, so the literal
    /// passed to `info_span!` must equal it or the bar would never show.
    #[test]
    fn span_name_matches_constant() {
        let span = info_span!("tcgcsv_sync");
        assert_eq!(span.metadata().unwrap().name(), SPAN_NAME);
    }
}
