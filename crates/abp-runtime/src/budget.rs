// SPDX-License-Identifier: MIT OR Apache-2.0
//! Budget enforcement for runtime runs.
//!
//! Tracks token usage, cost, turn count, and wall-clock duration against
//! configurable limits and reports when any dimension is exceeded or
//! approaching its cap.

use abp_duration_serde::option_duration_millis as optional_duration_ms;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

// Warning fires at this fraction of any limit.
const WARNING_THRESHOLD: f64 = 0.8;

/// Per-dimension caps for a single run. `None` means unlimited.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetLimit {
    /// Maximum number of tokens (prompt + completion combined).
    pub max_tokens: Option<u64>,
    /// Maximum spend in USD.
    pub max_cost_usd: Option<f64>,
    /// Maximum number of agent turns / iterations.
    pub max_turns: Option<u32>,
    /// Maximum wall-clock duration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_duration_ms"
    )]
    pub max_duration: Option<Duration>,
}

/// Thread-safe budget tracker backed by atomic counters.
///
/// # Examples
///
/// ```
/// use abp_runtime::budget::{BudgetTracker, BudgetLimit, BudgetStatus};
///
/// let tracker = BudgetTracker::new(BudgetLimit {
///     max_tokens: Some(1000),
///     max_turns: Some(5),
///     ..BudgetLimit::default()
/// });
///
/// tracker.record_tokens(200);
/// tracker.record_turn();
/// assert_eq!(tracker.check(), BudgetStatus::WithinLimits);
///
/// // Exceed the token budget.
/// tracker.record_tokens(900);
/// assert!(matches!(tracker.check(), BudgetStatus::Exceeded(_)));
/// ```
pub struct BudgetTracker {
    limit: BudgetLimit,
    tokens_used: AtomicU64,
    /// Cost stored as integer micro-dollars (1 USD = 1_000_000).
    cost_micro: AtomicU64,
    turns_used: AtomicU32,
    start: std::sync::Mutex<Option<Instant>>,
}

impl fmt::Debug for BudgetTracker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BudgetTracker")
            .field("limit", &self.limit)
            .field("tokens_used", &self.tokens_used.load(Relaxed))
            .field("cost_micro", &self.cost_micro.load(Relaxed))
            .field("turns_used", &self.turns_used.load(Relaxed))
            .finish()
    }
}

impl BudgetTracker {
    /// Create a new tracker with the given limits. All counters start at zero.
    #[must_use]
    pub fn new(limit: BudgetLimit) -> Self {
        Self {
            limit,
            tokens_used: AtomicU64::new(0),
            cost_micro: AtomicU64::new(0),
            turns_used: AtomicU32::new(0),
            start: std::sync::Mutex::new(None),
        }
    }

    /// Mark the beginning of execution (wall-clock timer).
    pub fn start_timer(&self) {
        *self.start.lock().expect("start mutex poisoned") = Some(Instant::now());
    }

    /// Record `count` tokens consumed.
    pub fn record_tokens(&self, count: u64) {
        self.tokens_used.fetch_add(count, Relaxed);
    }

    /// Record a cost increment in USD.
    pub fn record_cost(&self, amount: f64) {
        let micros = (amount * 1_000_000.0) as u64;
        self.cost_micro.fetch_add(micros, Relaxed);
    }

    /// Record one agent turn / iteration.
    pub fn record_turn(&self) {
        self.turns_used.fetch_add(1, Relaxed);
    }

    /// Check current usage against the limits.
    #[must_use]
    pub fn check(&self) -> BudgetStatus {
        let tokens = self.tokens_used.load(Relaxed);
        let cost_usd = self.cost_micro.load(Relaxed) as f64 / 1_000_000.0;
        let turns = self.turns_used.load(Relaxed);
        let elapsed = self.elapsed();

        // Check hard limits first.
        if let Some(max) = self.limit.max_tokens
            && tokens > max
        {
            return BudgetStatus::Exceeded(BudgetViolation::TokensExceeded {
                used: tokens,
                limit: max,
            });
        }
        if let Some(max) = self.limit.max_cost_usd
            && cost_usd > max
        {
            return BudgetStatus::Exceeded(BudgetViolation::CostExceeded {
                used: cost_usd,
                limit: max,
            });
        }
        if let Some(max) = self.limit.max_turns
            && turns > max
        {
            return BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded {
                used: turns,
                limit: max,
            });
        }
        if let Some(max_dur) = self.limit.max_duration
            && let Some(el) = elapsed
            && el > max_dur
        {
            return BudgetStatus::Exceeded(BudgetViolation::DurationExceeded {
                elapsed: el,
                limit: max_dur,
            });
        }

        // Check warning thresholds.
        let mut max_pct: f64 = 0.0;
        if let Some(max) = self.limit.max_tokens {
            max_pct = max_pct.max(tokens as f64 / max as f64);
        }
        if let Some(max) = self.limit.max_cost_usd
            && max > 0.0
        {
            max_pct = max_pct.max(cost_usd / max);
        }
        if let Some(max) = self.limit.max_turns {
            max_pct = max_pct.max(turns as f64 / max as f64);
        }
        if let Some(max_dur) = self.limit.max_duration
            && let Some(el) = elapsed
        {
            max_pct = max_pct.max(el.as_secs_f64() / max_dur.as_secs_f64());
        }

        if max_pct >= WARNING_THRESHOLD {
            BudgetStatus::Warning {
                usage_pct: max_pct * 100.0,
            }
        } else {
            BudgetStatus::WithinLimits
        }
    }

    /// Return the remaining budget in each dimension.
    #[must_use]
    pub fn remaining(&self) -> BudgetRemaining {
        let tokens = self.tokens_used.load(Relaxed);
        let cost_usd = self.cost_micro.load(Relaxed) as f64 / 1_000_000.0;
        let turns = self.turns_used.load(Relaxed);
        let elapsed = self.elapsed();

        BudgetRemaining {
            tokens: self.limit.max_tokens.map(|m| m.saturating_sub(tokens)),
            cost_usd: self.limit.max_cost_usd.map(|m| (m - cost_usd).max(0.0)),
            turns: self.limit.max_turns.map(|m| m.saturating_sub(turns)),
            duration: self
                .limit
                .max_duration
                .map(|m| elapsed.map_or(m, |el| m.saturating_sub(el))),
        }
    }

    // --- helpers ---

    fn elapsed(&self) -> Option<Duration> {
        self.start
            .lock()
            .expect("start mutex poisoned")
            .map(|s| s.elapsed())
    }
}

/// Result of a budget check.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetStatus {
    /// All dimensions are within limits (below warning threshold).
    WithinLimits,
    /// At least one dimension has exceeded its hard limit.
    Exceeded(BudgetViolation),
    /// At least one dimension is at or above the warning threshold (80 %+).
    Warning {
        /// Highest usage percentage across all dimensions.
        usage_pct: f64,
    },
}

/// Details about which dimension was exceeded.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetViolation {
    /// Token limit exceeded.
    TokensExceeded {
        /// Tokens consumed so far.
        used: u64,
        /// Configured token cap.
        limit: u64,
    },
    /// Cost limit exceeded.
    CostExceeded {
        /// Cost in USD consumed so far.
        used: f64,
        /// Configured cost cap in USD.
        limit: f64,
    },
    /// Turn limit exceeded.
    TurnsExceeded {
        /// Turns consumed so far.
        used: u32,
        /// Configured turn cap.
        limit: u32,
    },
    /// Duration limit exceeded.
    DurationExceeded {
        /// Wall-clock time elapsed.
        elapsed: Duration,
        /// Configured duration cap.
        limit: Duration,
    },
}

impl fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokensExceeded { used, limit } => {
                write!(f, "token budget exceeded: used {used}, limit {limit}")
            }
            Self::CostExceeded { used, limit } => {
                write!(
                    f,
                    "cost budget exceeded: used ${used:.4}, limit ${limit:.4}"
                )
            }
            Self::TurnsExceeded { used, limit } => {
                write!(f, "turn budget exceeded: used {used}, limit {limit}")
            }
            Self::DurationExceeded { elapsed, limit } => {
                write!(
                    f,
                    "duration budget exceeded: elapsed {:.1}s, limit {:.1}s",
                    elapsed.as_secs_f64(),
                    limit.as_secs_f64()
                )
            }
        }
    }
}

/// Remaining budget in each dimension. `None` means that dimension is unlimited.
#[derive(Debug, Clone)]
pub struct BudgetRemaining {
    /// Remaining tokens, if a token limit was set.
    pub tokens: Option<u64>,
    /// Remaining cost in USD, if a cost limit was set.
    pub cost_usd: Option<f64>,
    /// Remaining turns, if a turn limit was set.
    pub turns: Option<u32>,
    /// Remaining wall-clock duration, if a duration limit was set.
    pub duration: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlimited() -> BudgetLimit {
        BudgetLimit::default()
    }

    #[test]
    fn within_limits_when_no_caps() {
        let t = BudgetTracker::new(unlimited());
        t.record_tokens(999_999);
        t.record_cost(100.0);
        t.record_turn();
        assert_eq!(t.check(), BudgetStatus::WithinLimits);
    }

    #[test]
    fn tokens_exceeded() {
        let t = BudgetTracker::new(BudgetLimit {
            max_tokens: Some(100),
            ..Default::default()
        });
        t.record_tokens(101);
        assert!(matches!(
            t.check(),
            BudgetStatus::Exceeded(BudgetViolation::TokensExceeded {
                used: 101,
                limit: 100
            })
        ));
    }

    #[test]
    fn cost_exceeded() {
        let t = BudgetTracker::new(BudgetLimit {
            max_cost_usd: Some(1.0),
            ..Default::default()
        });
        t.record_cost(0.6);
        t.record_cost(0.5);
        assert!(matches!(
            t.check(),
            BudgetStatus::Exceeded(BudgetViolation::CostExceeded { .. })
        ));
    }

    #[test]
    fn turns_exceeded() {
        let t = BudgetTracker::new(BudgetLimit {
            max_turns: Some(3),
            ..Default::default()
        });
        for _ in 0..4 {
            t.record_turn();
        }
        assert!(matches!(
            t.check(),
            BudgetStatus::Exceeded(BudgetViolation::TurnsExceeded { used: 4, limit: 3 })
        ));
    }

    #[test]
    fn duration_exceeded() {
        let t = BudgetTracker::new(BudgetLimit {
            max_duration: Some(Duration::from_millis(1)),
            ..Default::default()
        });
        t.start_timer();
        std::thread::sleep(Duration::from_millis(10));
        assert!(matches!(
            t.check(),
            BudgetStatus::Exceeded(BudgetViolation::DurationExceeded { .. })
        ));
    }
}
