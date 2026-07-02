//! A small rate limiter for outbound provider requests.
//!
//! Collection providers enforce their own request caps (Archidekt ≈20 requests/minute;
//! Moxfield expects ≈1 request/second). We must stay under each **across every
//! import/sync in the process**, not per job, so the limiters are shared (held in the
//! import queue via [`ProviderLimiters`]) and every provider request goes through its
//! provider's [`RateLimiter::acquire`] before it is sent.
//!
//! Each provider gets its **own** limiter ([`ProviderLimiters`]) so a slow provider's
//! spacing (or a `429` back-off) never throttles a different provider's imports — the
//! caps are independent facts about each service.
//!
//! [`RateLimiter`] works by reserving evenly-spaced slots: each `acquire` claims the next
//! slot (`max(now, last_reserved) + interval`) under a short lock, then sleeps until that
//! slot outside the lock. Concurrent callers therefore get slots at `t`, `t+interval`,
//! `t+2·interval`, … — a steady stream that never exceeds the configured rate.

use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

use super::Provider;

/// Requests/minute we allow to Archidekt, across all imports (its documented cap ≈20/min).
pub const ARCHIDEKT_REQUESTS_PER_MINUTE: u32 = 20;
/// Requests/minute we allow to Moxfield, across all imports. Moxfield expects ≈1 req/s
/// (60/min), but we sit well under it for now; tune independently of Archidekt as needed.
pub const MOXFIELD_REQUESTS_PER_MINUTE: u32 = 20;

/// One [`RateLimiter`] per provider, so each provider's request cap is enforced
/// independently — a back-off or slow spacing on one never stalls another's imports.
pub struct ProviderLimiters {
    archidekt: RateLimiter,
    moxfield: RateLimiter,
}

impl ProviderLimiters {
    /// Build the per-provider limiters from explicit per-minute caps.
    pub fn new(archidekt_per_minute: u32, moxfield_per_minute: u32) -> Self {
        Self {
            archidekt: RateLimiter::per_minute(archidekt_per_minute),
            moxfield: RateLimiter::per_minute(moxfield_per_minute),
        }
    }

    /// The limiter governing requests to `provider`.
    pub fn for_provider(&self, provider: Provider) -> &RateLimiter {
        match provider {
            Provider::Archidekt => &self.archidekt,
            Provider::Moxfield => &self.moxfield,
        }
    }
}

impl Default for ProviderLimiters {
    fn default() -> Self {
        Self::new(ARCHIDEKT_REQUESTS_PER_MINUTE, MOXFIELD_REQUESTS_PER_MINUTE)
    }
}

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

    #[tokio::test(start_paused = true)]
    async fn provider_limiters_are_independent() {
        let limiters = ProviderLimiters::default();
        // A long back-off on one provider must not stall a different provider.
        limiters
            .for_provider(Provider::Archidekt)
            .back_off(Duration::from_secs(300))
            .await;
        let start = Instant::now();
        limiters.for_provider(Provider::Moxfield).acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(0));
    }

    #[tokio::test(start_paused = true)]
    async fn for_provider_maps_each_cap_to_its_own_provider() {
        // Distinct caps prove both the wiring (which cap governs which provider) and that
        // the two can differ: Archidekt at 60/min (1s spacing), Moxfield at 6/min (10s).
        // Swapping the `new` args or the `for_provider` match arms would flip these.
        let limiters = ProviderLimiters::new(60, 6);

        let archidekt = limiters.for_provider(Provider::Archidekt);
        archidekt.acquire().await; // immediate
        let start = Instant::now();
        archidekt.acquire().await; // +1s at 60/min
        assert_eq!(start.elapsed(), Duration::from_secs(1));

        let moxfield = limiters.for_provider(Provider::Moxfield);
        moxfield.acquire().await; // immediate
        let start = Instant::now();
        moxfield.acquire().await; // +10s at 6/min
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }
}
