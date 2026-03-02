// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retry and recovery layer for sidecar connection (spawn + hello handshake).
//!
//! Provides exponential backoff with jitter, configurable max retries and
//! overall timeout, and captures per-attempt metadata for receipt enrichment.

use crate::{HostError, SidecarClient, SidecarSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, warn};

// ── Configuration ───────────────────────────────────────────────────

/// Configuration for retry behaviour when connecting to a sidecar.
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

/// Serde helper — `Duration` as integer milliseconds.
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

// ── Metadata ────────────────────────────────────────────────────────

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

// ── Outcome ─────────────────────────────────────────────────────────

/// Result of a retry-enabled operation.
#[derive(Debug)]
pub struct RetryOutcome<T> {
    /// The successfully produced value.
    pub value: T,
    /// Retry metadata (empty `failed_attempts` when the first attempt succeeds).
    pub metadata: RetryMetadata,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Compute the backoff delay for a given zero-indexed attempt number.
pub fn compute_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let exp = 2u64.saturating_pow(attempt);
    let delay_ms = (config.base_delay.as_millis() as u64).saturating_mul(exp);
    let capped_ms = delay_ms.min(config.max_delay.as_millis() as u64);

    let jitter_factor = config.jitter_factor.clamp(0.0, 1.0);
    if jitter_factor > 0.0 && capped_ms > 0 {
        let jitter_range = (capped_ms as f64 * jitter_factor) as u64;
        // Cheap pseudo-random: use system-clock nanos mixed with attempt index.
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
        // Subtract up to `jitter_range` from the nominal delay.
        Duration::from_millis(capped_ms.saturating_sub(jitter))
    } else {
        Duration::from_millis(capped_ms)
    }
}

/// Returns `true` if the error is eligible for retry.
///
/// Protocol violations and unrecognised-message errors are generally
/// non-transient and should *not* be retried.
pub fn is_retryable(err: &HostError) -> bool {
    matches!(
        err,
        HostError::Spawn(_)
            | HostError::Stdout(_)
            | HostError::Exited { .. }
            | HostError::SidecarCrashed { .. }
            | HostError::Timeout { .. }
    )
}

// ── Core retry loop ─────────────────────────────────────────────────

/// Generic retry loop. Calls `op` up to `max_retries + 1` times with
/// exponential backoff, returning the first successful result along with
/// metadata about failed attempts.
///
/// `retryable` decides whether a given error should trigger a retry.
pub async fn retry_async<T, F, Fut>(
    config: &RetryConfig,
    mut op: F,
    retryable: fn(&HostError) -> bool,
) -> Result<RetryOutcome<T>, HostError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
    let start = Instant::now();
    let max_attempts = config.max_retries + 1;
    let mut failed_attempts = Vec::new();

    for attempt in 0..max_attempts {
        // Check overall timeout before each attempt.
        if start.elapsed() >= config.overall_timeout {
            warn!(
                target: "abp.host.retry",
                attempt,
                "overall timeout exceeded"
            );
            return Err(HostError::Timeout {
                duration: config.overall_timeout,
            });
        }

        debug!(
            target: "abp.host.retry",
            attempt,
            max_attempts,
            "attempting operation"
        );

        match op().await {
            Ok(value) => {
                let total_duration = start.elapsed();
                debug!(
                    target: "abp.host.retry",
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
                    debug!(
                        target: "abp.host.retry",
                        error = %err,
                        "non-retryable error, giving up"
                    );
                    return Err(err);
                }

                if is_last {
                    warn!(
                        target: "abp.host.retry",
                        error = %err,
                        attempt,
                        "max retries exhausted"
                    );
                    return Err(err);
                }

                let delay = compute_delay(config, attempt);
                warn!(
                    target: "abp.host.retry",
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

                // Don't sleep past the overall timeout.
                let remaining = config.overall_timeout.saturating_sub(start.elapsed());
                if delay > remaining {
                    return Err(HostError::Timeout {
                        duration: config.overall_timeout,
                    });
                }

                tokio::time::sleep(delay).await;
            }
        }
    }

    // Should be unreachable, but handle gracefully.
    Err(HostError::Timeout {
        duration: config.overall_timeout,
    })
}

// ── Convenience wrapper ─────────────────────────────────────────────

/// Spawn a sidecar with automatic retry on transient failures.
///
/// Wraps [`SidecarClient::spawn`] with exponential backoff and captures
/// retry metadata that can later be embedded in a receipt.
pub async fn spawn_with_retry(
    spec: SidecarSpec,
    config: &RetryConfig,
) -> Result<RetryOutcome<SidecarClient>, HostError> {
    retry_async(config, || SidecarClient::spawn(spec.clone()), is_retryable).await
}
