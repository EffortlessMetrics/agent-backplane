// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rate-limiting policy for agent work-order throughput.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RateLimitResult {
    /// The request is within limits.
    Allowed,
    /// The request should be retried after the given delay.
    Throttled {
        /// Milliseconds the caller should wait before retrying.
        retry_after_ms: u64,
    },
    /// The request is denied outright.
    Denied {
        /// Human-readable explanation.
        reason: String,
    },
}

impl RateLimitResult {
    /// Returns `true` when the result is [`RateLimitResult::Allowed`].
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// Returns `true` when the result is [`RateLimitResult::Throttled`].
    #[must_use]
    pub fn is_throttled(&self) -> bool {
        matches!(self, Self::Throttled { .. })
    }

    /// Returns `true` when the result is [`RateLimitResult::Denied`].
    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }
}

/// Configurable rate-limit thresholds for agent traffic.
///
/// All fields are optional — an unset limit means "no limit" for that
/// dimension. When multiple limits are set, the *most restrictive* one wins.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RateLimitPolicy {
    /// Maximum requests per minute (RPM). `None` means unlimited.
    pub max_requests_per_minute: Option<u32>,
    /// Maximum tokens per minute (TPM). `None` means unlimited.
    pub max_tokens_per_minute: Option<u64>,
    /// Maximum concurrent in-flight requests. `None` means unlimited.
    pub max_concurrent: Option<u32>,
}

impl RateLimitPolicy {
    /// Create a policy with no limits.
    #[must_use]
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Check the current counters against configured limits.
    ///
    /// Returns [`RateLimitResult::Allowed`] when all counters are within
    /// bounds, [`RateLimitResult::Throttled`] when a soft limit is hit, or
    /// [`RateLimitResult::Denied`] when a hard limit is exceeded.
    #[must_use]
    pub fn check_rate_limit(
        &self,
        current_rpm: u32,
        current_tpm: u64,
        current_concurrent: u32,
    ) -> RateLimitResult {
        // Concurrent limit is a hard deny — there is no sensible retry delay.
        if let Some(max) = self.max_concurrent {
            if current_concurrent >= max {
                return RateLimitResult::Denied {
                    reason: format!("concurrent limit exceeded ({current_concurrent}/{max})"),
                };
            }
        }

        // RPM limit → throttle with a back-off hint.
        if let Some(max) = self.max_requests_per_minute {
            if current_rpm >= max {
                let retry_ms = if max == 0 {
                    60_000
                } else {
                    60_000 / u64::from(max)
                };
                return RateLimitResult::Throttled {
                    retry_after_ms: retry_ms,
                };
            }
        }

        // TPM limit → throttle.
        if let Some(max) = self.max_tokens_per_minute {
            if current_tpm >= max {
                let retry_ms = if max == 0 { 60_000 } else { 60_000 / max };
                return RateLimitResult::Throttled {
                    retry_after_ms: retry_ms,
                };
            }
        }

        RateLimitResult::Allowed
    }
}
