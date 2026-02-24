use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    min_interval: Duration,
    last_request: Mutex<Option<Instant>>,
}

impl RateLimiter {
    pub fn new(requests_per_second: f64) -> Self {
        let min_interval = if requests_per_second > 0.0 {
            Duration::from_secs_f64(1.0 / requests_per_second)
        } else {
            tracing::warn!(
                "Rate limiter initialized with non-positive rate ({requests_per_second} req/s), no rate limiting applied"
            );
            Duration::ZERO
        };
        Self {
            min_interval,
            last_request: Mutex::new(None),
        }
    }

    pub async fn wait(&self) {
        let wait_duration = {
            let last = self.last_request.lock().unwrap();
            if let Some(last_time) = *last {
                let elapsed = last_time.elapsed();
                if elapsed < self.min_interval {
                    Some(self.min_interval.checked_sub(elapsed).unwrap())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(duration) = wait_duration {
            tokio::time::sleep(duration).await;
        }

        let mut last = self.last_request.lock().unwrap();
        *last = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_first_call_immediate() {
        let limiter = RateLimiter::new(10.0);
        let start = Instant::now();
        limiter.wait().await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn rate_limiter_second_call_delayed() {
        // 10 req/s = 100ms interval
        let limiter = RateLimiter::new(10.0);
        limiter.wait().await;
        let start = Instant::now();
        limiter.wait().await;
        // Second call should wait ~100ms (allow 50ms tolerance)
        assert!(start.elapsed() >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn rate_limiter_zero_rate_no_delay() {
        let limiter = RateLimiter::new(0.0);
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        limiter.wait().await;
        // All calls should be immediate
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn rate_limiter_respects_interval() {
        // 5 req/s = 200ms interval, 3 calls = >=400ms total wait
        let limiter = RateLimiter::new(5.0);
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        limiter.wait().await;
        assert!(start.elapsed() >= Duration::from_millis(300));
    }
}
