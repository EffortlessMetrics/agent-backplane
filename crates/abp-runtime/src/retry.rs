// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retry policies and timeout configuration for resilient backend execution.

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
    /// A policy that disables retries entirely.
    #[must_use]
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            backoff_multiplier: 1.0,
        }
    }

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

    /// Alias for [`compute_delay`](Self::compute_delay) — returns the
    /// backoff duration (with jitter) for the given attempt.
    #[must_use]
    pub fn delay_for(&self, attempt: u32) -> Duration {
        self.compute_delay(attempt)
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

// --- FallbackChain -----------------------------------------------------------

/// An ordered list of backend names to try when the primary backend fails.
///
/// Call [`next_backend`](Self::next_backend) to advance through the chain. Once all backends
/// have been consumed the iterator yields `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackChain {
    backends: Vec<String>,
    #[serde(skip, default)]
    index: usize,
}

impl FallbackChain {
    /// Create a new chain from an ordered list of backend names.
    #[must_use]
    pub fn new(backends: Vec<String>) -> Self {
        Self { backends, index: 0 }
    }

    /// Advance to the next backend in the chain.
    ///
    /// Returns `None` when all backends have been exhausted.
    pub fn next_backend(&mut self) -> Option<&str> {
        if self.index < self.backends.len() {
            let name = &self.backends[self.index];
            self.index += 1;
            Some(name)
        } else {
            None
        }
    }

    /// Reset the chain so it can be iterated again from the start.
    pub fn reset(&mut self) {
        self.index = 0;
    }

    /// Returns the number of backends remaining (not yet consumed by [`next_backend`](Self::next_backend)).
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.backends.len().saturating_sub(self.index)
    }

    /// Returns `true` when the chain has no backends at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    /// Returns the total number of backends in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.backends.len()
    }
}

// --- serde helpers for Duration as milliseconds -----------------------------

mod duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

mod option_duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => s.serialize_some(&(d.as_millis() as u64)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(d)?;
        Ok(opt.map(Duration::from_millis))
    }
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
