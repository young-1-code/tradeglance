use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(rate_per_sec: f64, burst: u32) -> Self {
        let capacity = f64::from(burst.max(1));
        Self {
            capacity,
            tokens: capacity,
            refill_per_sec: rate_per_sec.max(f64::EPSILON),
            last_refill: Instant::now(),
        }
    }

    pub fn try_acquire(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.tokens + f64::EPSILON >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn wait_duration(&mut self, now: Instant) -> Duration {
        self.refill(now);
        if self.tokens >= 1.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((1.0 - self.tokens) / self.refill_per_sec)
        }
    }

    fn refill(&mut self, now: Instant) {
        if now <= self.last_refill {
            return;
        }
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.last_refill = now;
    }
}

#[derive(Debug, Clone)]
pub struct AsyncRateLimiter {
    bucket: Arc<Mutex<TokenBucket>>,
}

impl AsyncRateLimiter {
    pub fn new(rate_per_sec: f64, burst: u32) -> Self {
        Self {
            bucket: Arc::new(Mutex::new(TokenBucket::new(rate_per_sec, burst))),
        }
    }

    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut bucket = self.bucket.lock().await;
                let now = Instant::now();
                if bucket.try_acquire(now) {
                    return;
                }
                bucket.wait_duration(now)
            };
            tokio::time::sleep(wait.max(Duration::from_millis(1))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::TokenBucket;

    #[test]
    fn token_bucket_admits_burst_then_throttles() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(1.0, 2);

        assert!(bucket.try_acquire(start));
        assert!(bucket.try_acquire(start));
        assert!(!bucket.try_acquire(start));

        assert!(!bucket.try_acquire(start + Duration::from_millis(999)));
        assert!(bucket.try_acquire(start + Duration::from_millis(1001)));
    }
}
