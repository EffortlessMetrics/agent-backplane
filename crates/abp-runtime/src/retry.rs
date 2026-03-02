// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retry policies and timeout configuration for resilient backend execution.

use abp_duration_serde::{duration_millis, option_duration_millis};
use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

/// Exponential-backoff retry policy with jitter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 means no retries).
    pub max_retries: u32,
    /// Base delay before the first retry.
    #[serde(with = "duration_millis")]
    pub initial_backoff: Duration,
    /// Upper bound on any single backoff delay.
    #[serde(with = "duration_millis")]
    pub max_backoff: Duration,
    /// Multiplicative factor applied to the backoff on each attempt.
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Start building a custom [`RetryPolicy`].
    #[must_use]
    pub fn builder() -> RetryPolicyBuilder {
        RetryPolicyBuilder(Self::default())
    }

    /// Compute the backoff delay for a given attempt (0-indexed).
    ///
    /// The raw delay is `initial_backoff * multiplier^attempt`, capped at
    /// [`max_backoff`](Self::max_backoff). A deterministic jitter of ±25 %
    /// is then applied so that concurrent callers don't all retry at the
    /// same instant.
    #[must_use]
    pub fn compute_delay(&self, attempt: u32) -> Duration {
        let base =
            self.initial_backoff.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_backoff.as_secs_f64());

        // Deterministic jitter derived from the attempt number.
        let jitter_factor = jitter(attempt);
        let jittered = capped * jitter_factor;

        // Clamp so we never exceed max_backoff or go below zero.
        let final_secs = jittered.max(0.0).min(self.max_backoff.as_secs_f64());
        Duration::from_secs_f64(final_secs)
    }

    /// Returns `true` when the given attempt index should be retried.
    #[must_use]
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }
}

/// Builder for [`RetryPolicy`].
#[derive(Debug, Clone)]
pub struct RetryPolicyBuilder(RetryPolicy);

impl RetryPolicyBuilder {
    /// Set the maximum number of retries.
    #[must_use]
    pub fn max_retries(mut self, n: u32) -> Self {
        self.0.max_retries = n;
        self
    }

    /// Set the initial backoff duration.
    #[must_use]
    pub fn initial_backoff(mut self, d: Duration) -> Self {
        self.0.initial_backoff = d;
        self
    }

    /// Set the maximum backoff duration.
    #[must_use]
    pub fn max_backoff(mut self, d: Duration) -> Self {
        self.0.max_backoff = d;
        self
    }

    /// Set the backoff multiplier.
    #[must_use]
    pub fn backoff_multiplier(mut self, m: f64) -> Self {
        self.0.backoff_multiplier = m;
        self
    }

    /// Consume the builder and return the configured [`RetryPolicy`].
    #[must_use]
    pub fn build(self) -> RetryPolicy {
        self.0
    }
}

/// Per-run timeout configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TimeoutConfig {
    /// Overall deadline for the entire run. `None` means no limit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_duration_millis"
    )]
    pub run_timeout: Option<Duration>,
    /// Maximum silence between consecutive events. `None` means no limit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_duration_millis"
    )]
    pub event_timeout: Option<Duration>,
}

// --- helpers ----------------------------------------------------------------

/// Produce a deterministic jitter factor in [0.75, 1.25] for the given attempt.
fn jitter(attempt: u32) -> f64 {
    let mut h = DefaultHasher::new();
    attempt.hash(&mut h);
    let bits = h.finish();
    // Map to [0, 1) then scale to [0.75, 1.25].
    let unit = (bits as f64) / (u64::MAX as f64);
    0.75 + unit * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.initial_backoff, Duration::from_millis(100));
        assert_eq!(p.max_backoff, Duration::from_secs(5));
        assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jitter_within_bounds() {
        for attempt in 0..100 {
            let factor = jitter(attempt);
            assert!(
                (0.75..=1.25).contains(&factor),
                "jitter({attempt}) = {factor}"
            );
        }
    }
}
