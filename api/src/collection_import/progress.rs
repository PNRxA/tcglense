//! Live fetch progress for a background import job.
//!
//! A network import pages a provider collection under a strict rate limit, so it can run
//! for minutes; [`ProgressReporter`] lets the fetch loop publish how far it's got (rows
//! fetched, and the total when the provider reports one up front) so the poll endpoint can
//! surface a progress bar. It's a handful of atomics behind an `Arc`: the worker writes,
//! the status handler snapshots, no lock. Deliberately provider-agnostic — every provider
//! reports the same two numbers.

use std::sync::atomic::{AtomicU32, Ordering};

/// Shared, lock-free fetch progress for one import. Held behind an `Arc` by the job and
/// by the worker's [`ProviderContext`](super::ProviderContext).
#[derive(Debug, Default)]
pub struct ProgressReporter {
    /// Rows fetched from the provider so far (accumulates across pages).
    fetched_rows: AtomicU32,
    /// Total rows to fetch, or `0` when not (yet) known — a smart sync never sets it
    /// (it stops early, so a total would be misleading).
    total_rows: AtomicU32,
}

impl ProgressReporter {
    /// Record the total row count the provider reported up front (from the first page).
    /// Saturates at `u32::MAX`, which is far above [`MAX_IMPORT_ROWS`](super::MAX_IMPORT_ROWS).
    pub fn set_total(&self, total: usize) {
        self.total_rows
            .store(total.min(u32::MAX as usize) as u32, Ordering::Relaxed);
    }

    /// Add the rows just fetched from one page to the running total.
    pub fn add_fetched(&self, rows: usize) {
        self.fetched_rows
            .fetch_add(rows.min(u32::MAX as usize) as u32, Ordering::Relaxed);
    }

    /// A consistent-enough point-in-time read for the status endpoint.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let total = self.total_rows.load(Ordering::Relaxed);
        ProgressSnapshot {
            fetched_rows: self.fetched_rows.load(Ordering::Relaxed),
            // `0` is the "not reported" sentinel — surface it as absent rather than a
            // bogus "0 total".
            total_rows: (total > 0).then_some(total),
        }
    }
}

/// A point-in-time view of an import's fetch progress, cloned out for the status endpoint.
#[derive(Debug, Clone, Copy)]
pub struct ProgressSnapshot {
    pub fetched_rows: u32,
    pub total_rows: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_fetched_and_exposes_total_when_set() {
        let p = ProgressReporter::default();
        // Nothing reported yet: zero fetched, unknown total.
        let s = p.snapshot();
        assert_eq!(s.fetched_rows, 0);
        assert_eq!(s.total_rows, None);

        p.set_total(120);
        p.add_fetched(25);
        p.add_fetched(25);
        let s = p.snapshot();
        assert_eq!(s.fetched_rows, 50);
        assert_eq!(s.total_rows, Some(120));
    }
}
