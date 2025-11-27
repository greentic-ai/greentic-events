use std::time::Duration;

/// Backoff strategy used for retries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackoffStrategy {
    Fixed,
    Exponential,
    ExponentialWithJitter,
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        Self::ExponentialWithJitter
    }
}

/// Configuration for retrying failed publishes.
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub strategy: BackoffStrategy,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub retryable_errors: Vec<String>,
}

impl RetryPolicy {
    pub fn default_with_budget(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    /// Returns true if the error message should be retried.
    pub fn is_retryable(&self, error_message: &str) -> bool {
        if self.retryable_errors.is_empty() {
            return true;
        }
        self.retryable_errors
            .iter()
            .any(|pat| error_message.contains(pat))
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        match self.strategy {
            BackoffStrategy::Fixed => self
                .max_delay
                .min(self.base_delay.saturating_mul(attempt.max(1))),
            BackoffStrategy::Exponential | BackoffStrategy::ExponentialWithJitter => {
                let exp = 2_u32.saturating_pow(attempt);
                let candidate = self.base_delay.checked_mul(exp).unwrap_or(self.max_delay);
                let capped = self.max_delay.min(candidate);
                if self.strategy == BackoffStrategy::ExponentialWithJitter {
                    jitter(capped)
                } else {
                    capped
                }
            }
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            strategy: BackoffStrategy::ExponentialWithJitter,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            retryable_errors: Vec::new(),
        }
    }
}

fn jitter(duration: Duration) -> Duration {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::from_entropy();
    let nanos = duration.as_nanos();
    let jitter = rng.gen_range(0..=nanos as u64);
    Duration::from_nanos(jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_increases() {
        let policy = RetryPolicy {
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(1),
            strategy: BackoffStrategy::Exponential,
            max_retries: 3,
            retryable_errors: Vec::new(),
        };
        assert!(policy.delay_for_attempt(1) >= Duration::from_millis(10));
        assert!(policy.delay_for_attempt(2) >= Duration::from_millis(20));
    }
}
