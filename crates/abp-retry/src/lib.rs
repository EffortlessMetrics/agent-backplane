// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, warn};

/// Configuration for retry behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts after the initial attempt.
    /// `0` means only the initial attempt (no retries).
    pub max_retries: u32,
    /// Base delay for exponential backoff.
    #[serde(with = "duration_millis")]
    pub base_delay: Duration,
    /// Maximum delay cap for exponential backoff.
    #[serde(with = "duration_millis")]
    pub max_delay: Duration,
    /// Overall wall-clock timeout across all attempts.
    #[serde(with = "duration_millis")]
    pub overall_timeout: Duration,
    /// Jitter factor in `[0.0, 1.0]`. 0 = no jitter, 1 = full jitter.
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            overall_timeout: Duration::from_secs(60),
            jitter_factor: 0.5,
        }
    }
}

/// Record of a single failed attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryAttempt {
    /// Zero-indexed attempt number.
    pub attempt: u32,
    /// Error message from this attempt.
    pub error: String,
    /// Backoff delay applied before the next attempt.
    #[serde(with = "duration_millis")]
    pub delay: Duration,
}

/// Metadata captured across all retry attempts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetryMetadata {
    /// Total number of attempts made (including the successful one).
    pub total_attempts: u32,
    /// Records of each *failed* attempt.
    pub failed_attempts: Vec<RetryAttempt>,
    /// Wall-clock time spanning all attempts.
    #[serde(with = "duration_millis")]
    pub total_duration: Duration,
}

impl RetryMetadata {
    /// Convert to a `BTreeMap` suitable for embedding in receipt metadata.
    pub fn to_receipt_metadata(&self) -> BTreeMap<String, serde_json::Value> {
        let mut map = BTreeMap::new();
        map.insert(
            "retry_total_attempts".into(),
            serde_json::json!(self.total_attempts),
        );
        map.insert(
            "retry_total_duration_ms".into(),
            serde_json::json!(self.total_duration.as_millis() as u64),
        );
        if !self.failed_attempts.is_empty() {
            let attempts: Vec<_> = self
                .failed_attempts
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "attempt": a.attempt,
                        "error": a.error,
                        "delay_ms": a.delay.as_millis() as u64,
                    })
                })
                .collect();
            map.insert("retry_failed_attempts".into(), serde_json::json!(attempts));
        }
        map
    }
}

/// Result of a retry-enabled operation.
#[derive(Debug)]
pub struct RetryOutcome<T> {
    /// The successfully produced value.
    pub value: T,
    /// Retry metadata (empty `failed_attempts` when the first attempt succeeds).
    pub metadata: RetryMetadata,
}

/// Compute the backoff delay for a given zero-indexed attempt number.
pub fn compute_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let exp = 2u64.saturating_pow(attempt);
    let delay_ms = (config.base_delay.as_millis() as u64).saturating_mul(exp);
    let capped_ms = delay_ms.min(config.max_delay.as_millis() as u64);

    let jitter_factor = config.jitter_factor.clamp(0.0, 1.0);
    if jitter_factor > 0.0 && capped_ms > 0 {
        let jitter_range = (capped_ms as f64 * jitter_factor) as u64;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
        let pseudo = nanos.wrapping_mul(attempt as u64 + 1);
        let jitter = if jitter_range > 0 {
            pseudo % jitter_range
        } else {
            0
        };
        Duration::from_millis(capped_ms.saturating_sub(jitter))
    } else {
        Duration::from_millis(capped_ms)
    }
}

/// Generic retry loop.
///
/// Calls `op` up to `max_retries + 1` times with exponential backoff,
/// returning the first successful result along with metadata about failures.
///
/// `retryable` decides whether a given error should trigger a retry.
/// `timeout_error` constructs the timeout error returned when overall timeout
/// is reached.
pub async fn retry_async<T, E, F, Fut>(
    config: &RetryConfig,
    mut op: F,
    retryable: fn(&E) -> bool,
    timeout_error: fn(Duration) -> E,
) -> Result<RetryOutcome<T>, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let start = Instant::now();
    let max_attempts = config.max_retries + 1;
    let mut failed_attempts = Vec::new();

    for attempt in 0..max_attempts {
        if start.elapsed() >= config.overall_timeout {
            warn!(target: "abp.retry", attempt, "overall timeout exceeded");
            return Err(timeout_error(config.overall_timeout));
        }

        debug!(target: "abp.retry", attempt, max_attempts, "attempting operation");

        match op().await {
            Ok(value) => {
                let total_duration = start.elapsed();
                debug!(
                    target: "abp.retry",
                    attempt,
                    total_duration_ms = total_duration.as_millis() as u64,
                    "operation succeeded"
                );
                return Ok(RetryOutcome {
                    value,
                    metadata: RetryMetadata {
                        total_attempts: attempt + 1,
                        failed_attempts,
                        total_duration,
                    },
                });
            }
            Err(err) => {
                let is_last = attempt + 1 >= max_attempts;

                if !retryable(&err) {
                    debug!(target: "abp.retry", error = %err, "non-retryable error, giving up");
                    return Err(err);
                }

                if is_last {
                    warn!(target: "abp.retry", error = %err, attempt, "max retries exhausted");
                    return Err(err);
                }

                let delay = compute_delay(config, attempt);
                warn!(
                    target: "abp.retry",
                    error = %err,
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "retryable error, backing off"
                );

                failed_attempts.push(RetryAttempt {
                    attempt,
                    error: err.to_string(),
                    delay,
                });

                let remaining = config.overall_timeout.saturating_sub(start.elapsed());
                if delay > remaining {
                    return Err(timeout_error(config.overall_timeout));
                }

                tokio::time::sleep(delay).await;
            }
        }
    }

    Err(timeout_error(config.overall_timeout))
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Duration, ser: S) -> Result<S::Ok, S::Error> {
        val.as_millis().serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
        let ms: u64 = u64::deserialize(de)?;
        Ok(Duration::from_millis(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    enum TestError {
        Retryable,
        Fatal,
        Timeout(Duration),
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Retryable => write!(f, "retryable"),
                Self::Fatal => write!(f, "fatal"),
                Self::Timeout(d) => write!(f, "timeout after {d:?}"),
            }
        }
    }

    fn retryable(e: &TestError) -> bool {
        matches!(e, TestError::Retryable | TestError::Timeout(_))
    }

    fn timeout_error(d: Duration) -> TestError {
        TestError::Timeout(d)
    }

    #[tokio::test]
    async fn succeeds_after_retries() {
        let cfg = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            overall_timeout: Duration::from_secs(1),
            jitter_factor: 0.0,
        };

        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts_for_op = attempts.clone();
        let out = retry_async(
            &cfg,
            move || {
                let attempts = attempts_for_op.clone();
                async move {
                    let n = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    if n < 3 {
                        Err(TestError::Retryable)
                    } else {
                        Ok(7)
                    }
                }
            },
            retryable,
            timeout_error,
        )
        .await
        .expect("eventually succeeds");

        assert_eq!(out.value, 7);
        assert_eq!(out.metadata.total_attempts, 3);
        assert_eq!(out.metadata.failed_attempts.len(), 2);
    }

    #[tokio::test]
    async fn non_retryable_stops_immediately() {
        let cfg = RetryConfig {
            max_retries: 10,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            overall_timeout: Duration::from_secs(1),
            jitter_factor: 0.0,
        };

        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let attempts_for_op = attempts.clone();
        let err = retry_async(
            &cfg,
            move || {
                let attempts = attempts_for_op.clone();
                async move {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err::<(), _>(TestError::Fatal)
                }
            },
            retryable,
            timeout_error,
        )
        .await
        .expect_err("fatal should short-circuit");

        assert!(matches!(err, TestError::Fatal));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
