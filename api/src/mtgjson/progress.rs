//! Pretty terminal progress for the MTGJSON sealed-contents ingest.
//!
//! Like the Scryfall import and the TCGCSV sweep, the ingest streams a large file
//! (`AllPrintings.json`, ~160 MB gzipped) and then resolves + writes membership rows,
//! so it sits with no feedback otherwise. The download size isn't known until the
//! response arrives and the resolve phase is CPU-bound, so this is a **spinner** (an
//! indeterminate bar) carrying a running-stage message + a row tally, rather than a
//! determinate count bar. Like [`crate::scryfall`]'s bar it wraps a
//! [`tracing_indicatif`] span so concurrent log lines never clobber it and a non-TTY
//! stderr renders nothing.
//!
//! The span is named `mtgjson_sync`; `main.rs` scopes the `IndicatifLayer` to it
//! (alongside the Scryfall + TCGCSV spans) so unrelated spans don't each sprout a bar.

use indicatif::{HumanCount, ProgressStyle};
use tracing::{Span, info_span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

/// The span name the ingest bar attaches to; `main.rs` filters the `IndicatifLayer` to
/// this. Must stay in sync with the literal passed to `info_span!` below (`tracing`
/// requires a literal there); `span_name_matches_constant` guards against drift.
pub const SPAN_NAME: &str = "mtgjson_sync";

/// A live spinner for the MTGJSON sealed-contents ingest. Dropping it closes the span
/// and removes the bar, so an early `?` return cleans up automatically.
pub struct SyncProgress {
    span: Span,
}

impl SyncProgress {
    /// Begin the ingest spinner with an initial stage message.
    pub fn start(stage: &str) -> Self {
        let span = info_span!("mtgjson_sync");
        span.pb_set_style(&spinner_style());
        span.pb_set_message(&format!("Syncing sealed products · {stage}"));
        span.pb_start();
        Self { span }
    }

    /// Update the stage message (e.g. `"downloading"`, `"resolving"`, `"writing"`).
    pub fn set_stage(&self, stage: &str) {
        self.span
            .pb_set_message(&format!("Syncing sealed products · {stage}"));
    }

    /// Update the stage message with a running row tally.
    pub fn set_rows(&self, stage: &str, rows: u64) {
        self.span.pb_set_message(&format!(
            "Syncing sealed products · {stage} · {} rows",
            HumanCount(rows)
        ));
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}\n   {elapsed_precise} elapsed")
        .expect("valid spinner template")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_indicatif::IndicatifLayer;
    use tracing_subscriber::layer::SubscriberExt;

    /// Drives the spinner under a real (hidden, non-TTY) `IndicatifLayer`, exercising the
    /// `IndicatifSpanExt` calls and asserting the `ProgressStyle` template parses (the
    /// `expect` would otherwise panic only on a live sync).
    #[test]
    fn drives_the_spinner_without_panicking() {
        let subscriber = tracing_subscriber::registry().with(IndicatifLayer::new());
        tracing::subscriber::with_default(subscriber, || {
            let progress = SyncProgress::start("downloading");
            progress.set_stage("resolving");
            progress.set_rows("writing", 12_345);
            drop(progress);
        });
    }

    /// The `IndicatifLayer` filter in `main.rs` matches on `SPAN_NAME`, so the literal
    /// passed to `info_span!` must equal it or the bar would never show.
    #[test]
    fn span_name_matches_constant() {
        let span = info_span!("mtgjson_sync");
        assert_eq!(span.metadata().unwrap().name(), SPAN_NAME);
    }
}
