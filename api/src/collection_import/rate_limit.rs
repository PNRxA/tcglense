//! A small global rate limiter for outbound provider requests.
//!
//! Collection providers enforce strict request caps (Archidekt ≈20 requests/minute;
//! Moxfield expects ≈1 request/second, which the same spacing sits comfortably under).
//! We must stay under them **across every import/sync in the process**, not per job, so
//! the limiter is shared (held in the import queue) and every provider request — any
//! provider — goes through [`RateLimiter::acquire`] before it is sent.
//!
//! It works by reserving evenly-spaced slots: each `acquire` claims the next slot
//! (`max(now, last_reserved) + interval`) under a short lock, then sleeps until that
//! slot outside the lock. Concurrent callers therefore get slots at `t`, `t+interval`,
//! `t+2·interval`, … — a steady stream that never exceeds the configured rate.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

pub struct RateLimiter {
    /// Minimum spacing between consecutive requests (e.g. 3s for 20/min).
    min_interval: Duration,
    /// The earliest instant the next request may be sent. `None` until the first call.
    next_slot: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// A limiter permitting at most `per_minute` requests per minute (spaced evenly).
    /// `per_minute == 0` is treated as 1/min so it can never divide by zero or busy-loop.
    pub fn per_minute(per_minute: u32) -> Self {
        let per_minute = per_minute.max(1);
        Self {
            min_interval: Duration::from_secs(60) / per_minute,
            next_slot: Mutex::new(None),
        }
    }

    /// Wait until it is permitted to send the next request, reserving this slot. Returns
    /// immediately when the limiter is idle; otherwise sleeps until the reserved instant.
    pub async fn acquire(&self) {
        let slot = {
            let mut next = self.next_slot.lock().await;
            let now = Instant::now();
            // This request runs at the later of "now" and the reserved next slot.
            let slot = next.map_or(now, |t| t.max(now));
            // Reserve the following slot for whoever comes next.
            *next = Some(slot + self.min_interval);
            slot
        };
        sleep_until(slot).await;
    }

    /// Push the next permitted request out by at least `delay` — e.g. after the provider
    /// returns `429`, so every caller backs off, not just the one that was throttled.
    /// Never brings the next slot earlier than already scheduled.
    pub async fn back_off(&self, delay: Duration) {
        let target = Instant::now() + delay;
        let mut next = self.next_slot.lock().await;
        *next = Some(next.map_or(target, |t| t.max(target)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn spaces_consecutive_requests_by_the_interval() {
        // 20/min => a 3s spacing.
        let rl = RateLimiter::per_minute(20);
        let start = Instant::now();
        rl.acquire().await; // immediate
        rl.acquire().await; // +3s
        rl.acquire().await; // +6s
        // Under paused time, sleep_until advances the virtual clock exactly.
        assert_eq!(start.elapsed(), Duration::from_secs(6));
    }

    #[tokio::test(start_paused = true)]
    async fn first_request_is_immediate() {
        let rl = RateLimiter::per_minute(20);
        let start = Instant::now();
        rl.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(0));
    }

    #[tokio::test(start_paused = true)]
    async fn back_off_delays_the_next_acquire() {
        let rl = RateLimiter::per_minute(20);
        rl.acquire().await; // immediate; next slot reserved 3s out
        rl.back_off(Duration::from_secs(60)).await;
        let start = Instant::now();
        rl.acquire().await; // must wait out the 60s backoff, not just the 3s spacing
        assert_eq!(start.elapsed(), Duration::from_secs(60));
    }
}
