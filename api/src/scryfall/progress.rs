//! Pretty terminal progress for the Scryfall bulk-card import.
//!
//! The import streams a ~500 MB bulk file for ~30 s; with no feedback the
//! terminal just sits idle, and raw per-batch `tracing` lines are noisy. This
//! wraps a [`tracing_indicatif`] span so the import shows a single live progress
//! display: a spinner while set metadata is fetched, then a determinate byte bar
//! as the bulk file streams. The byte bar is accurate because Scryfall's `size`
//! is the *uncompressed* length and the gzip response is decompressed for us, so
//! counting bytes read against `size` lines up.
//!
//! Driving the bar through `tracing-indicatif` (rather than a bare
//! [`indicatif::ProgressBar`]) means concurrent log lines never clobber it, and
//! when stderr is not a TTY (CI, redirected output) indicatif renders nothing —
//! so this is silent there and leaves the logs untouched.
//!
//! The span is named `scryfall_import`; `main.rs` scopes the `IndicatifLayer` to
//! that name so unrelated spans don't each sprout a bar.

use indicatif::{HumanCount, ProgressStyle};
use tracing::{Span, info_span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

/// The span name the import bar is attached to; `main.rs` filters the
/// `IndicatifLayer` to this so only the import gets a progress bar. Must stay in
/// sync with the literal passed to `info_span!` below (`tracing` requires a
/// literal there); `span_name_matches_constant` guards against drift.
pub const SPAN_NAME: &str = "scryfall_import";

/// A live progress display for one card-data import. Dropping it closes the span
/// and removes the bar, so an early `?` return cleans up automatically.
pub struct ImportProgress {
    span: Span,
    game: &'static str,
}

impl ImportProgress {
    /// Begin an import display in its indeterminate "fetching set metadata" phase
    /// (a spinner). `game` is the human-readable game name shown in the bar.
    pub fn start(game: &'static str) -> Self {
        let span = info_span!("scryfall_import");
        span.pb_set_style(&spinner_style());
        span.pb_set_message(&format!("Fetching {game} set metadata…"));
        // Show the bar now and keep it visible for the span's whole lifetime,
        // without having to hold an `enter()` guard across the import's awaits.
        span.pb_start();
        Self { span, game }
    }

    /// Switch to the determinate card-streaming phase. `total_bytes` is the
    /// uncompressed bulk-file size; `None`/`0` falls back to a byte spinner with
    /// no ETA (Scryfall always reports a size, so that path is a safety net).
    pub fn begin_cards(&self, total_bytes: Option<u64>) {
        let total = total_bytes.unwrap_or(0);
        self.span.pb_set_style(&bar_style(total > 0));
        self.span.pb_set_length(total);
        self.span.pb_set_position(0);
        self.span
            .pb_set_message(&format!("Importing {} cards", self.game));
    }

    /// Advance the byte bar by `n` decompressed bytes read from the stream.
    pub fn add_bytes(&self, n: u64) {
        self.span.pb_inc(n);
    }

    /// Update the running tally of imported cards shown alongside the bar.
    pub fn set_cards(&self, imported: u64) {
        self.span.pb_set_message(&format!(
            "Importing {} cards · {} imported",
            self.game,
            HumanCount(imported)
        ));
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}").expect("valid spinner template")
}

fn bar_style(determinate: bool) -> ProgressStyle {
    let template = if determinate {
        "{spinner:.green} {msg}\n   {elapsed_precise} ▕{bar:35.cyan/blue}▏ \
         {decimal_bytes}/{decimal_total_bytes} · {decimal_bytes_per_sec} · ETA {eta}"
    } else {
        "{spinner:.green} {msg}\n   {elapsed_precise} {decimal_bytes} streamed · \
         {decimal_bytes_per_sec}"
    };
    ProgressStyle::with_template(template)
        .expect("valid progress-bar template")
        .progress_chars("█▉▊▋▌▍▎▏ ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_indicatif::IndicatifLayer;
    use tracing_subscriber::layer::SubscriberExt;

    /// Drives every phase under a real (hidden, non-TTY) `IndicatifLayer`. This
    /// both exercises the `IndicatifSpanExt` calls end to end and asserts that
    /// every `ProgressStyle` template parses (the `expect`s would otherwise panic
    /// only on a live import).
    #[test]
    fn drives_all_phases_without_panicking() {
        let subscriber = tracing_subscriber::registry().with(IndicatifLayer::new());
        tracing::subscriber::with_default(subscriber, || {
            let progress = ImportProgress::start("Test Game");
            progress.begin_cards(Some(2_000));
            progress.add_bytes(1_000);
            progress.set_cards(400);
            progress.add_bytes(1_000);
            progress.set_cards(800);

            // Indeterminate fallback (unknown size) path.
            let fallback = ImportProgress::start("Test Game");
            fallback.begin_cards(None);
            fallback.add_bytes(500);

            drop(progress);
            drop(fallback);
        });
    }

    /// The `IndicatifLayer` filter in `main.rs` matches on `SPAN_NAME`, so the
    /// literal passed to `info_span!` must equal it or the bar would never show.
    #[test]
    fn span_name_matches_constant() {
        let span = info_span!("scryfall_import");
        assert_eq!(span.metadata().unwrap().name(), SPAN_NAME);
    }
}
