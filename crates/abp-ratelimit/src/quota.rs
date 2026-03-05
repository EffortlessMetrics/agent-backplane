#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Result of a quota check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaResult {
    /// Request is within quota.
    Allowed {
        /// Remaining units in the most constrained bucket.
        remaining: u64,
    },
    /// Quota exhausted.
    Exhausted {
        /// Duration until the next reset.
        retry_after: Duration,
    },
}

/// Configuration for a single quota bucket.
#[derive(Debug, Clone)]
pub struct QuotaLimit {
    /// Maximum number of units per period.
    pub limit: u64,
    /// Time period for this quota.
    pub period: Duration,
}

/// Manages token/request quotas with configurable limits per time period.
///
/// Supports multiple named quota keys (e.g., per-backend or per-user),
/// each with one or more time-period-based limits (e.g. 100 requests/minute
/// AND 5000 requests/hour).
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use abp_ratelimit::{QuotaManager, QuotaLimit, QuotaResult};
///
/// let manager = QuotaManager::new();
/// manager.set_limits("openai", vec![
///     QuotaLimit { limit: 10, period: Duration::from_secs(60) },
///     QuotaLimit { limit: 100, period: Duration::from_secs(3600) },
/// ]);
/// assert!(matches!(manager.try_consume("openai", 1), QuotaResult::Allowed { .. }));
/// ```
#[derive(Debug, Clone)]
pub struct QuotaManager {
    inner: Arc<Mutex<QuotaManagerInner>>,
}

#[derive(Debug)]
struct QuotaManagerInner {
    quotas: HashMap<String, Vec<QuotaBucket>>,
    default_limits: Vec<QuotaLimit>,
}

#[derive(Debug)]
struct QuotaBucket {
    limit: u64,
    used: u64,
    period: Duration,
    reset_at: Instant,
}

impl QuotaBucket {
    fn new(limit: &QuotaLimit) -> Self {
        Self {
            limit: limit.limit,
            used: 0,
            period: limit.period,
            reset_at: Instant::now() + limit.period,
        }
    }

    fn maybe_reset(&mut self) {
        let now = Instant::now();
        if now >= self.reset_at {
            self.used = 0;
            self.reset_at = now + self.period;
        }
    }

    fn remaining(&mut self) -> u64 {
        self.maybe_reset();
        self.limit.saturating_sub(self.used)
    }

    fn retry_after(&self) -> Duration {
        let now = Instant::now();
        if now < self.reset_at {
            self.reset_at - now
        } else {
            Duration::ZERO
        }
    }
}

impl QuotaManager {
    /// Create a new empty quota manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(QuotaManagerInner {
                quotas: HashMap::new(),
                default_limits: Vec::new(),
            })),
        }
    }

    /// Set default limits applied to keys without explicit configuration.
    pub fn set_default_limits(&self, limits: Vec<QuotaLimit>) {
        let mut inner = self.inner.lock().unwrap();
        inner.default_limits = limits;
    }

    /// Set quota limits for a specific key.
    pub fn set_limits(&self, key: &str, limits: Vec<QuotaLimit>) {
        let mut inner = self.inner.lock().unwrap();
        let buckets = limits.iter().map(QuotaBucket::new).collect();
        inner.quotas.insert(key.to_string(), buckets);
    }

    /// Try to consume `n` units from the quota for `key`.
    ///
    /// All buckets for the key must have sufficient remaining quota.
    /// If any bucket is exhausted, returns [`QuotaResult::Exhausted`]
    /// with the retry duration of the most constrained bucket.
    pub fn try_consume(&self, key: &str, n: u64) -> QuotaResult {
        let mut inner = self.inner.lock().unwrap();

        // Ensure buckets exist (lazily create from defaults)
        if !inner.quotas.contains_key(key) {
            if inner.default_limits.is_empty() {
                return QuotaResult::Allowed {
                    remaining: u64::MAX,
                };
            }
            let buckets = inner.default_limits.iter().map(QuotaBucket::new).collect();
            inner.quotas.insert(key.to_string(), buckets);
        }

        let buckets = inner.quotas.get_mut(key).unwrap();

        // Check all buckets first
        let mut min_remaining = u64::MAX;
        let mut max_retry = Duration::ZERO;
        let mut any_exhausted = false;

        for bucket in buckets.iter_mut() {
            bucket.maybe_reset();
            let rem = bucket.limit.saturating_sub(bucket.used);
            if rem < n {
                any_exhausted = true;
                let retry = bucket.retry_after();
                if retry > max_retry {
                    max_retry = retry;
                }
            }
            if rem < min_remaining {
                min_remaining = rem;
            }
        }

        if any_exhausted {
            return QuotaResult::Exhausted {
                retry_after: max_retry,
            };
        }

        // Consume from all buckets
        for bucket in buckets.iter_mut() {
            bucket.used += n;
        }

        QuotaResult::Allowed {
            remaining: min_remaining.saturating_sub(n),
        }
    }

    /// Return the remaining quota for a key (minimum across all buckets).
    pub fn remaining(&self, key: &str) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        if let Some(buckets) = inner.quotas.get_mut(key) {
            buckets.iter_mut().map(|b| b.remaining()).min().unwrap_or(0)
        } else {
            u64::MAX
        }
    }

    /// Reset all quotas for a specific key.
    pub fn reset(&self, key: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.quotas.remove(key);
    }

    /// Reset all quotas across all keys.
    pub fn reset_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.quotas.clear();
    }

    /// Check whether limits are configured for a key (either explicit or default).
    pub fn has_limits(&self, key: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.quotas.contains_key(key) || !inner.default_limits.is_empty()
    }
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_allows_all() {
        let mgr = QuotaManager::new();
        assert!(matches!(
            mgr.try_consume("any", 1),
            QuotaResult::Allowed {
                remaining: u64::MAX
            }
        ));
    }

    #[test]
    fn single_limit_allows_within_quota() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 10,
                period: Duration::from_secs(60),
            }],
        );
        assert!(matches!(
            mgr.try_consume("key", 5),
            QuotaResult::Allowed { remaining: 5 }
        ));
    }

    #[test]
    fn single_limit_denies_over_quota() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 3,
                period: Duration::from_secs(60),
            }],
        );
        assert!(matches!(
            mgr.try_consume("key", 3),
            QuotaResult::Allowed { .. }
        ));
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Exhausted { .. }
        ));
    }

    #[test]
    fn multiple_limits_most_constrained_wins() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![
                QuotaLimit {
                    limit: 100,
                    period: Duration::from_secs(3600),
                },
                QuotaLimit {
                    limit: 5,
                    period: Duration::from_secs(60),
                },
            ],
        );
        for _ in 0..5 {
            assert!(matches!(
                mgr.try_consume("key", 1),
                QuotaResult::Allowed { .. }
            ));
        }
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Exhausted { .. }
        ));
    }

    #[test]
    fn remaining_tracks_usage() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 10,
                period: Duration::from_secs(60),
            }],
        );
        // Force bucket creation
        mgr.try_consume("key", 0);
        assert_eq!(mgr.remaining("key"), 10);
        mgr.try_consume("key", 3);
        assert_eq!(mgr.remaining("key"), 7);
    }

    #[test]
    fn remaining_unknown_key_returns_max() {
        let mgr = QuotaManager::new();
        assert_eq!(mgr.remaining("unknown"), u64::MAX);
    }

    #[test]
    fn reset_clears_key() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 2,
                period: Duration::from_secs(60),
            }],
        );
        mgr.try_consume("key", 2);
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Exhausted { .. }
        ));
        mgr.reset("key");
        // After reset with no defaults, key is unconfigured → unlimited
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Allowed {
                remaining: u64::MAX
            }
        ));
    }

    #[test]
    fn reset_all_clears_everything() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "a",
            vec![QuotaLimit {
                limit: 1,
                period: Duration::from_secs(60),
            }],
        );
        mgr.set_limits(
            "b",
            vec![QuotaLimit {
                limit: 1,
                period: Duration::from_secs(60),
            }],
        );
        mgr.try_consume("a", 1);
        mgr.try_consume("b", 1);
        mgr.reset_all();
        assert_eq!(mgr.remaining("a"), u64::MAX);
        assert_eq!(mgr.remaining("b"), u64::MAX);
    }

    #[test]
    fn default_limits_apply_to_unknown_keys() {
        let mgr = QuotaManager::new();
        mgr.set_default_limits(vec![QuotaLimit {
            limit: 5,
            period: Duration::from_secs(60),
        }]);
        for _ in 0..5 {
            assert!(matches!(
                mgr.try_consume("any-key", 1),
                QuotaResult::Allowed { .. }
            ));
        }
        assert!(matches!(
            mgr.try_consume("any-key", 1),
            QuotaResult::Exhausted { .. }
        ));
    }

    #[test]
    fn explicit_limits_override_defaults() {
        let mgr = QuotaManager::new();
        mgr.set_default_limits(vec![QuotaLimit {
            limit: 2,
            period: Duration::from_secs(60),
        }]);
        mgr.set_limits(
            "special",
            vec![QuotaLimit {
                limit: 100,
                period: Duration::from_secs(60),
            }],
        );
        for _ in 0..50 {
            assert!(matches!(
                mgr.try_consume("special", 1),
                QuotaResult::Allowed { .. }
            ));
        }
    }

    #[test]
    fn has_limits_check() {
        let mgr = QuotaManager::new();
        assert!(!mgr.has_limits("key"));
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 10,
                period: Duration::from_secs(60),
            }],
        );
        assert!(mgr.has_limits("key"));
    }

    #[test]
    fn has_limits_with_defaults() {
        let mgr = QuotaManager::new();
        mgr.set_default_limits(vec![QuotaLimit {
            limit: 10,
            period: Duration::from_secs(60),
        }]);
        assert!(mgr.has_limits("unknown"));
    }

    #[test]
    fn exhausted_retry_after_is_positive() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 1,
                period: Duration::from_secs(60),
            }],
        );
        mgr.try_consume("key", 1);
        if let QuotaResult::Exhausted { retry_after } = mgr.try_consume("key", 1) {
            assert!(retry_after > Duration::ZERO);
            assert!(retry_after <= Duration::from_secs(60));
        } else {
            panic!("expected Exhausted");
        }
    }

    #[test]
    fn consume_zero_always_succeeds() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 0,
                period: Duration::from_secs(60),
            }],
        );
        assert!(matches!(
            mgr.try_consume("key", 0),
            QuotaResult::Allowed { remaining: 0 }
        ));
    }

    #[test]
    fn period_reset_restores_quota() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 2,
                period: Duration::from_millis(30),
            }],
        );
        mgr.try_consume("key", 2);
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Exhausted { .. }
        ));
        std::thread::sleep(Duration::from_millis(50));
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Allowed { .. }
        ));
    }

    #[test]
    fn clone_shares_state() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 3,
                period: Duration::from_secs(60),
            }],
        );
        let clone = mgr.clone();
        mgr.try_consume("key", 2);
        assert_eq!(clone.remaining("key"), 1);
    }

    #[test]
    fn default_impl() {
        let mgr = QuotaManager::default();
        assert!(!mgr.has_limits("x"));
    }

    #[test]
    fn consume_batch() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "key",
            vec![QuotaLimit {
                limit: 100,
                period: Duration::from_secs(60),
            }],
        );
        assert!(matches!(
            mgr.try_consume("key", 50),
            QuotaResult::Allowed { remaining: 50 }
        ));
        assert!(matches!(
            mgr.try_consume("key", 50),
            QuotaResult::Allowed { remaining: 0 }
        ));
        assert!(matches!(
            mgr.try_consume("key", 1),
            QuotaResult::Exhausted { .. }
        ));
    }

    #[test]
    fn independent_keys_have_separate_quotas() {
        let mgr = QuotaManager::new();
        mgr.set_limits(
            "a",
            vec![QuotaLimit {
                limit: 5,
                period: Duration::from_secs(60),
            }],
        );
        mgr.set_limits(
            "b",
            vec![QuotaLimit {
                limit: 5,
                period: Duration::from_secs(60),
            }],
        );
        mgr.try_consume("a", 5);
        // "b" should still have quota
        assert!(matches!(
            mgr.try_consume("b", 1),
            QuotaResult::Allowed { .. }
        ));
    }
}
