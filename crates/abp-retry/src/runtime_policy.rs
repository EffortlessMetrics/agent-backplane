// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runtime-facing retry and timeout policy primitives.
//!
//! These types were extracted from `abp-runtime` to keep execution policy
//! concerns in a dedicated SRP microcrate.

use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

/// Exponential-backoff retry policy with deterministic jitter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeRetryPolicy {
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

impl Default for RuntimeRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
        }
    }
}

impl RuntimeRetryPolicy {
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

    /// Start building a custom [`RuntimeRetryPolicy`].
    #[must_use]
    pub fn builder() -> RetryPolicyBuilder {
        RetryPolicyBuilder(Self::default())
    }

    /// Compute the backoff delay for a given attempt (0-indexed).
    #[must_use]
    pub fn compute_delay(&self, attempt: u32) -> Duration {
        let base =
            self.initial_backoff.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_backoff.as_secs_f64());
        let jittered = capped * jitter(attempt);
        Duration::from_secs_f64(jittered.max(0.0).min(self.max_backoff.as_secs_f64()))
    }

    /// Alias for [`compute_delay`](Self::compute_delay).
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

/// Builder for [`RuntimeRetryPolicy`].
#[derive(Debug, Clone)]
pub struct RetryPolicyBuilder(RuntimeRetryPolicy);

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

    /// Consume the builder and return the configured [`RuntimeRetryPolicy`].
    #[must_use]
    pub fn build(self) -> RuntimeRetryPolicy {
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

/// An ordered list of backend names to try when the primary backend fails.
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

    /// Returns the number of backends remaining.
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

fn jitter(attempt: u32) -> f64 {
    let mut h = DefaultHasher::new();
    attempt.hash(&mut h);
    let unit = (h.finish() as f64) / (u64::MAX as f64);
    0.75 + unit * 0.5
}

mod duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
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
        Ok(Option::<u64>::deserialize(d)?.map(Duration::from_millis))
    }
}
