//! Rate limiter for API requests, measured in requests per second.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// Limits the rate of operations to at most `requests_per_second` per second.
/// Call `acquire()` before each operation to throttle.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
}

struct RateLimiterInner {
    min_interval: Duration,
    last_request: Instant,
}

impl RateLimiter {
    /// Creates a rate limiter allowing at most `requests_per_second` operations per second.
    /// Panics if `requests_per_second` is zero or negative.
    pub fn new(requests_per_second: f64) -> Self {
        assert!(
            requests_per_second > 0.0,
            "requests_per_second must be positive"
        );
        let min_interval = Duration::from_secs_f64(1.0 / requests_per_second);
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                min_interval,
                last_request: Instant::now() - min_interval,
            })),
        }
    }

    /// Waits until the minimum interval has passed since the last acquire, then returns.
    /// Call this before each API request.
    pub async fn acquire(&self) {
        let mut inner = self.inner.lock().await;
        let elapsed = inner.last_request.elapsed();
        if elapsed < inner.min_interval {
            sleep(inner.min_interval - elapsed).await;
        }
        inner.last_request = Instant::now();
    }

    /// Duration to wait before retrying after a 429 rate limit error.
    /// Returns 2 * (1/rate_limit) = double the minimum interval.
    pub fn retry_delay(&self) -> Duration {
        if let Ok(guard) = self.inner.try_lock() {
            guard.min_interval.saturating_mul(2)
        } else {
            Duration::from_secs(1)
        }
    }

    /// Halves the rate (doubles the interval) after a 429. Use for the rest of the run.
    /// Uses blocking lock to ensure the update always persists.
    pub async fn back_off(&self) {
        let mut inner = self.inner.lock().await;
        inner.min_interval = inner.min_interval.saturating_mul(2);
    }
}
