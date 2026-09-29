//! Process-wide pacing of every request sent to Airbnb (I3).
//!
//! One `RateLimiter` is built in `application::build_client` and shared by the
//! GraphQL client, the HTML scraper and the API-key manager, so
//! `rate_limit_per_second` bounds the **total** request rate.
//!
//! Slots are reserved under the lock and slept outside it. N concurrent callers
//! therefore get N distinct slots spaced by `min_interval`, instead of all
//! computing the same deadline and firing together. After a 429,
//! [`RateLimiter::penalize`] holds back every caller, including the ones
//! already sleeping on a slot they reserved before the 429.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

/// Interval used when the configured rate is not a finite positive number.
pub const FALLBACK_INTERVAL: Duration = Duration::from_secs(2);

/// Longest interval between two requests; slower rates are clamped to it.
pub const MAX_INTERVAL: Duration = Duration::from_secs(3600);

/// Reservation-based limiter: one request per `min_interval`, process-wide.
pub struct RateLimiter {
    min_interval: Duration,
    state: Mutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
    /// Earliest instant the next reservation may fire at.
    next_slot: Option<Instant>,
    /// Set by `penalize`: nobody fires before this instant, not even a caller
    /// that reserved an earlier slot.
    paused_until: Option<Instant>,
}

impl RateLimiter {
    /// Limiter allowing `requests_per_second` requests per second.
    ///
    /// `Config::validate` rejects bad rates at load time. This constructor
    /// still never panics and never disables pacing: a non-finite or
    /// non-positive rate falls back to [`FALLBACK_INTERVAL`], and a tiny rate
    /// is clamped to [`MAX_INTERVAL`].
    pub fn new(requests_per_second: f64) -> Self {
        let min_interval = if requests_per_second.is_finite() && requests_per_second > 0.0 {
            Duration::try_from_secs_f64(1.0 / requests_per_second)
                .map_or(MAX_INTERVAL, |interval| interval.min(MAX_INTERVAL))
        } else {
            tracing::warn!(
                requests_per_second,
                "invalid rate limit; falling back to one request every {} s",
                FALLBACK_INTERVAL.as_secs()
            );
            FALLBACK_INTERVAL
        };
        Self::from_interval(min_interval)
    }

    /// Limiter with an explicit interval between two requests.
    pub fn from_interval(min_interval: Duration) -> Self {
        Self {
            min_interval,
            state: Mutex::new(SlotState::default()),
        }
    }

    /// Interval enforced between two requests.
    pub fn min_interval(&self) -> Duration {
        self.min_interval
    }

    /// Wait for this caller's slot. The slot is reserved under the lock and
    /// slept outside it. If a 429 paused the process while this caller slept,
    /// it takes a fresh slot after the pause instead of firing.
    pub async fn wait(&self) {
        loop {
            let slot = self.reserve();
            if slot > Instant::now() {
                tokio::time::sleep_until(slot).await;
            }
            if !self.is_paused() {
                return;
            }
        }
    }

    /// Hold back every caller until at least `now + delay` (capped at
    /// [`MAX_INTERVAL`]), including callers already sleeping on a reserved
    /// slot. Used after a 429 so the whole process backs off, not just one
    /// request. A shorter penalty never shortens an existing pause or slot.
    pub fn penalize(&self, delay: Duration) {
        let until = Instant::now() + delay.min(MAX_INTERVAL);
        let mut state = self.lock();
        state.paused_until = Some(state.paused_until.map_or(until, |paused| paused.max(until)));
        state.next_slot = Some(state.next_slot.map_or(until, |free_at| free_at.max(until)));
    }

    fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reserve(&self) -> Instant {
        let now = Instant::now();
        let mut state = self.lock();
        let earliest = state.paused_until.map_or(now, |paused| paused.max(now));
        let slot = state
            .next_slot
            .map_or(earliest, |free_at| free_at.max(earliest));
        state.next_slot = Some(slot + self.min_interval);
        slot
    }

    fn is_paused(&self) -> bool {
        let now = Instant::now();
        self.lock().paused_until.is_some_and(|paused| paused > now)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn first_call_is_immediate() {
        let limiter = RateLimiter::from_interval(Duration::from_secs(2));
        let start = Instant::now();
        limiter.wait().await;
        assert_eq!(Instant::now(), start);
    }

    #[tokio::test(start_paused = true)]
    async fn sequential_calls_are_spaced_by_the_interval() {
        let limiter = RateLimiter::new(0.5);
        let start = Instant::now();
        for _ in 0..3 {
            limiter.wait().await;
        }
        assert!(Instant::now() - start >= Duration::from_secs(4));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_get_distinct_spaced_slots() {
        let limiter = Arc::new(RateLimiter::new(0.5));
        let start = Instant::now();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..6 {
            let limiter = Arc::clone(&limiter);
            tasks.spawn(async move {
                limiter.wait().await;
                Instant::now()
            });
        }
        let mut fired = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            fired.push(joined.expect("waiter task panicked"));
        }
        fired.sort();
        assert_eq!(fired[0], start, "the first request must not wait");
        for pair in fired.windows(2) {
            assert!(
                pair[1] - pair[0] >= Duration::from_secs(2),
                "two requests fired {:?} apart",
                pair[1] - pair[0]
            );
        }
        assert!(fired[5] - start >= Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn penalize_pushes_the_next_slot_back_for_everyone() {
        let limiter = RateLimiter::from_interval(Duration::from_secs(2));
        limiter.wait().await;
        let start = Instant::now();
        limiter.penalize(Duration::from_secs(30));
        limiter.wait().await;
        assert!(Instant::now() - start >= Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn penalize_never_shortens_an_existing_reservation() {
        let limiter = RateLimiter::from_interval(Duration::from_secs(60));
        limiter.wait().await; // next free slot: now + 60 s
        let start = Instant::now();
        limiter.penalize(Duration::from_secs(1));
        limiter.wait().await;
        assert!(Instant::now() - start >= Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn penalize_also_holds_back_callers_already_waiting() {
        let limiter = Arc::new(RateLimiter::from_interval(Duration::from_secs(2)));
        let start = Instant::now();
        limiter.wait().await; // slot `start`; the next free slot is start + 2 s
        let waiter = {
            let limiter = Arc::clone(&limiter);
            tokio::spawn(async move {
                limiter.wait().await; // reserves start + 2 s, then sleeps
                Instant::now()
            })
        };
        // Let the waiter reserve its slot and go to sleep before the 429 arrives.
        tokio::time::sleep(Duration::from_millis(10)).await;
        limiter.penalize(Duration::from_secs(30));
        let fired = waiter.await.expect("waiter task panicked");
        assert!(
            fired - start >= Duration::from_secs(30),
            "a caller that reserved its slot before the 429 fired {:?} after start",
            fired - start
        );
    }

    #[test]
    fn invalid_rates_fall_back_instead_of_disabling_limiting() {
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                RateLimiter::new(rate).min_interval(),
                FALLBACK_INTERVAL,
                "rate {rate}"
            );
        }
    }

    #[test]
    fn tiny_rates_are_clamped_instead_of_panicking() {
        assert_eq!(RateLimiter::new(1e-30).min_interval(), MAX_INTERVAL);
        assert_eq!(RateLimiter::new(1e-300).min_interval(), MAX_INTERVAL);
    }

    #[test]
    fn normal_rates_map_to_their_interval() {
        assert_eq!(RateLimiter::new(0.5).min_interval(), Duration::from_secs(2));
        assert_eq!(
            RateLimiter::new(4.0).min_interval(),
            Duration::from_millis(250)
        );
    }
}
