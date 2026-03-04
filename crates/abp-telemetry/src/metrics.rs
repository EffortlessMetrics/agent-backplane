// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fine-grained metrics counters, gauges, and accumulators.
//!
//! These types complement the existing [`super::MetricsCollector`] by exposing
//! individual metric primitives that can be updated independently and queried
//! at any time.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// RequestCounter
// ---------------------------------------------------------------------------

/// Counts requests by `(backend, dialect, outcome)` dimensions.
///
/// Thread-safe — all mutations go through an internal `Mutex`.
#[derive(Debug, Clone, Default)]
pub struct RequestCounter {
    inner: Arc<Mutex<BTreeMap<RequestKey, u64>>>,
}

/// Composite key for [`RequestCounter`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestKey {
    /// Backend name (e.g. `"mock"`, `"sidecar:node"`).
    pub backend: String,
    /// Dialect / vendor identifier (e.g. `"openai"`, `"anthropic"`).
    pub dialect: String,
    /// Outcome label (e.g. `"success"`, `"error"`, `"timeout"`).
    pub outcome: String,
}

impl RequestCounter {
    /// Create a new, empty counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the counter for the given dimensions by one.
    pub fn increment(&self, backend: &str, dialect: &str, outcome: &str) {
        let key = RequestKey {
            backend: backend.to_string(),
            dialect: dialect.to_string(),
            outcome: outcome.to_string(),
        };
        let mut map = self.inner.lock().expect("request counter lock poisoned");
        *map.entry(key).or_insert(0) += 1;
    }

    /// Get the current count for a specific dimension tuple.
    pub fn get(&self, backend: &str, dialect: &str, outcome: &str) -> u64 {
        let key = RequestKey {
            backend: backend.to_string(),
            dialect: dialect.to_string(),
            outcome: outcome.to_string(),
        };
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.get(&key).copied().unwrap_or(0)
    }

    /// Return a snapshot of all recorded counts.
    pub fn snapshot(&self) -> BTreeMap<RequestKey, u64> {
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.clone()
    }

    /// Total count across all dimension tuples.
    pub fn total(&self) -> u64 {
        let map = self.inner.lock().expect("request counter lock poisoned");
        map.values().sum()
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("request counter lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// ErrorCounter
// ---------------------------------------------------------------------------

/// Counts errors by error-code string.
///
/// Thread-safe via an internal `Mutex`.
#[derive(Debug, Clone, Default)]
pub struct ErrorCounter {
    inner: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl ErrorCounter {
    /// Create a new, empty error counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the count for `error_code` by one.
    pub fn increment(&self, error_code: &str) {
        let mut map = self.inner.lock().expect("error counter lock poisoned");
        *map.entry(error_code.to_string()).or_insert(0) += 1;
    }

    /// Get the current count for `error_code`.
    pub fn get(&self, error_code: &str) -> u64 {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.get(error_code).copied().unwrap_or(0)
    }

    /// Return a snapshot of all error counts.
    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.clone()
    }

    /// Total number of errors across all codes.
    pub fn total(&self) -> u64 {
        let map = self.inner.lock().expect("error counter lock poisoned");
        map.values().sum()
    }

    /// Reset all counters.
    pub fn reset(&self) {
        let mut map = self.inner.lock().expect("error counter lock poisoned");
        map.clear();
    }
}

// ---------------------------------------------------------------------------
// ActiveRequestGauge
// ---------------------------------------------------------------------------

/// Atomic gauge tracking the number of in-flight requests.
///
/// Use [`increment`](Self::increment) when a request starts and
/// [`decrement`](Self::decrement) when it completes.
#[derive(Debug, Default)]
pub struct ActiveRequestGauge {
    value: AtomicI64,
}

impl ActiveRequestGauge {
    /// Create a gauge initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one to the gauge (request started).
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Subtract one from the gauge (request finished).
    pub fn decrement(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Current gauge value.
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// TokenAccumulator
// ---------------------------------------------------------------------------

/// Atomic accumulator for token usage (input + output).
#[derive(Debug, Default)]
pub struct TokenAccumulator {
    input: AtomicU64,
    output: AtomicU64,
}

impl TokenAccumulator {
    /// Create an accumulator initialised to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add token counts from a single request.
    pub fn add(&self, input_tokens: u64, output_tokens: u64) {
        self.input.fetch_add(input_tokens, Ordering::Relaxed);
        self.output.fetch_add(output_tokens, Ordering::Relaxed);
    }

    /// Total input tokens recorded so far.
    pub fn total_input(&self) -> u64 {
        self.input.load(Ordering::Relaxed)
    }

    /// Total output tokens recorded so far.
    pub fn total_output(&self) -> u64 {
        self.output.load(Ordering::Relaxed)
    }

    /// Combined total of input + output tokens.
    pub fn total(&self) -> u64 {
        self.total_input() + self.total_output()
    }

    /// Reset both counters to zero.
    pub fn reset(&self) {
        self.input.store(0, Ordering::Relaxed);
        self.output.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- RequestCounter ---

    #[test]
    fn request_counter_empty() {
        let c = RequestCounter::new();
        assert_eq!(c.total(), 0);
        assert_eq!(c.get("mock", "openai", "success"), 0);
    }

    #[test]
    fn request_counter_increment_and_get() {
        let c = RequestCounter::new();
        c.increment("mock", "openai", "success");
        c.increment("mock", "openai", "success");
        c.increment("mock", "openai", "error");
        assert_eq!(c.get("mock", "openai", "success"), 2);
        assert_eq!(c.get("mock", "openai", "error"), 1);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn request_counter_snapshot() {
        let c = RequestCounter::new();
        c.increment("a", "d1", "ok");
        c.increment("b", "d2", "ok");
        let snap = c.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn request_counter_reset() {
        let c = RequestCounter::new();
        c.increment("a", "d", "ok");
        c.reset();
        assert_eq!(c.total(), 0);
    }

    // --- ErrorCounter ---

    #[test]
    fn error_counter_empty() {
        let c = ErrorCounter::new();
        assert_eq!(c.total(), 0);
        assert_eq!(c.get("E001"), 0);
    }

    #[test]
    fn error_counter_increment_and_get() {
        let c = ErrorCounter::new();
        c.increment("E001");
        c.increment("E001");
        c.increment("E002");
        assert_eq!(c.get("E001"), 2);
        assert_eq!(c.get("E002"), 1);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn error_counter_reset() {
        let c = ErrorCounter::new();
        c.increment("E001");
        c.reset();
        assert_eq!(c.total(), 0);
    }

    // --- ActiveRequestGauge ---

    #[test]
    fn gauge_starts_at_zero() {
        let g = ActiveRequestGauge::new();
        assert_eq!(g.get(), 0);
    }

    #[test]
    fn gauge_increment_decrement() {
        let g = ActiveRequestGauge::new();
        g.increment();
        g.increment();
        assert_eq!(g.get(), 2);
        g.decrement();
        assert_eq!(g.get(), 1);
    }

    // --- TokenAccumulator ---

    #[test]
    fn token_accumulator_empty() {
        let t = TokenAccumulator::new();
        assert_eq!(t.total_input(), 0);
        assert_eq!(t.total_output(), 0);
        assert_eq!(t.total(), 0);
    }

    #[test]
    fn token_accumulator_add() {
        let t = TokenAccumulator::new();
        t.add(100, 200);
        t.add(50, 75);
        assert_eq!(t.total_input(), 150);
        assert_eq!(t.total_output(), 275);
        assert_eq!(t.total(), 425);
    }

    #[test]
    fn token_accumulator_reset() {
        let t = TokenAccumulator::new();
        t.add(100, 200);
        t.reset();
        assert_eq!(t.total(), 0);
    }
}
