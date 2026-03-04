#![allow(dead_code, unused_imports)]
//! Error aggregation — collects errors over time and produces summaries with
//! counts by category and code, including trending (most-common in a window).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::category::RecoveryCategory;
use crate::{AbpError, ErrorCategory, ErrorCode};

/// A single recorded error entry with a timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEntry {
    /// The error code that was recorded.
    pub code: ErrorCode,
    /// The category of the error.
    pub category: ErrorCategory,
    /// The recovery category of the error.
    pub recovery_category: RecoveryCategory,
    /// Human-readable message.
    pub message: String,
    /// Seconds since the aggregator was created (monotonic, not wall-clock).
    pub elapsed_secs: f64,
}

/// Summary of aggregated errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSummary {
    /// Total number of errors recorded.
    pub total: usize,
    /// Counts grouped by [`ErrorCode`].
    pub by_code: BTreeMap<ErrorCode, usize>,
    /// Counts grouped by [`ErrorCategory`].
    pub by_category: BTreeMap<ErrorCategory, usize>,
    /// Counts grouped by [`RecoveryCategory`].
    pub by_recovery_category: BTreeMap<RecoveryCategory, usize>,
}

/// A trending entry — an error code with its count in the window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrendingEntry {
    /// The error code.
    pub code: ErrorCode,
    /// Number of occurrences in the window.
    pub count: usize,
}

/// Collects errors over time and produces summaries.
#[derive(Debug)]
pub struct ErrorAggregator {
    entries: Vec<ErrorEntry>,
    start: Instant,
}

impl ErrorAggregator {
    /// Create a new, empty aggregator.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            start: Instant::now(),
        }
    }

    /// Record an error.
    pub fn add(&mut self, error: &AbpError) {
        let elapsed = self.start.elapsed();
        self.entries.push(ErrorEntry {
            code: error.code,
            category: error.code.category(),
            recovery_category: crate::category::categorize(error.code),
            message: error.message.clone(),
            elapsed_secs: elapsed.as_secs_f64(),
        });
    }

    /// Produce a summary of all recorded errors.
    pub fn summary(&self) -> ErrorSummary {
        let mut by_code: BTreeMap<ErrorCode, usize> = BTreeMap::new();
        let mut by_category: BTreeMap<ErrorCategory, usize> = BTreeMap::new();
        let mut by_recovery: BTreeMap<RecoveryCategory, usize> = BTreeMap::new();

        for entry in &self.entries {
            *by_code.entry(entry.code).or_default() += 1;
            *by_category.entry(entry.category).or_default() += 1;
            *by_recovery.entry(entry.recovery_category).or_default() += 1;
        }

        ErrorSummary {
            total: self.entries.len(),
            by_code,
            by_category,
            by_recovery_category: by_recovery,
        }
    }

    /// Return the most common error codes within the last `window` duration,
    /// sorted by count descending.
    pub fn trending(&self, window: Duration) -> Vec<TrendingEntry> {
        let cutoff = self.start.elapsed().as_secs_f64() - window.as_secs_f64();
        let mut counts: BTreeMap<ErrorCode, usize> = BTreeMap::new();

        for entry in &self.entries {
            if entry.elapsed_secs >= cutoff {
                *counts.entry(entry.code).or_default() += 1;
            }
        }

        let mut trending: Vec<TrendingEntry> = counts
            .into_iter()
            .map(|(code, count)| TrendingEntry { code, count })
            .collect();
        trending.sort_by(|a, b| b.count.cmp(&a.count).then(a.code.cmp(&b.code)));
        trending
    }

    /// Reset the aggregator, clearing all recorded entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.start = Instant::now();
    }

    /// Number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the aggregator is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ErrorAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_error(code: ErrorCode, msg: &str) -> AbpError {
        AbpError::new(code, msg)
    }

    #[test]
    fn empty_aggregator() {
        let agg = ErrorAggregator::new();
        assert!(agg.is_empty());
        assert_eq!(agg.len(), 0);
        let summary = agg.summary();
        assert_eq!(summary.total, 0);
        assert!(summary.by_code.is_empty());
        assert!(summary.by_category.is_empty());
        assert!(summary.by_recovery_category.is_empty());
    }

    #[test]
    fn add_and_summary() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::BackendTimeout, "t1"));
        agg.add(&make_error(ErrorCode::BackendTimeout, "t2"));
        agg.add(&make_error(ErrorCode::PolicyDenied, "p1"));

        assert_eq!(agg.len(), 3);
        assert!(!agg.is_empty());

        let s = agg.summary();
        assert_eq!(s.total, 3);
        assert_eq!(s.by_code[&ErrorCode::BackendTimeout], 2);
        assert_eq!(s.by_code[&ErrorCode::PolicyDenied], 1);
        assert_eq!(s.by_category[&ErrorCategory::Backend], 2);
        assert_eq!(s.by_category[&ErrorCategory::Policy], 1);
    }

    #[test]
    fn trending_returns_sorted_by_count() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::PolicyDenied, "p1"));
        agg.add(&make_error(ErrorCode::BackendTimeout, "t1"));
        agg.add(&make_error(ErrorCode::BackendTimeout, "t2"));
        agg.add(&make_error(ErrorCode::BackendTimeout, "t3"));
        agg.add(&make_error(ErrorCode::PolicyDenied, "p2"));

        let trending = agg.trending(Duration::from_secs(600));
        assert_eq!(trending.len(), 2);
        assert_eq!(trending[0].code, ErrorCode::BackendTimeout);
        assert_eq!(trending[0].count, 3);
        assert_eq!(trending[1].code, ErrorCode::PolicyDenied);
        assert_eq!(trending[1].count, 2);
    }

    #[test]
    fn trending_with_zero_window() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::Internal, "i1"));
        // A zero-second window may still include events added in the same instant.
        let trending = agg.trending(Duration::ZERO);
        // All events happened at elapsed ~0, so they might or might not be included.
        // We simply verify no panic and the result is valid.
        assert!(trending.len() <= 1);
    }

    #[test]
    fn clear_resets() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::BackendTimeout, "t1"));
        agg.add(&make_error(ErrorCode::Internal, "i1"));
        assert_eq!(agg.len(), 2);

        agg.clear();
        assert!(agg.is_empty());
        assert_eq!(agg.len(), 0);
        let s = agg.summary();
        assert_eq!(s.total, 0);
    }

    #[test]
    fn summary_recovery_categories() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::BackendAuthFailed, "a1"));
        agg.add(&make_error(ErrorCode::BackendRateLimited, "r1"));
        agg.add(&make_error(ErrorCode::BackendTimeout, "t1"));

        let s = agg.summary();
        assert_eq!(
            s.by_recovery_category[&RecoveryCategory::Authentication],
            1
        );
        assert_eq!(s.by_recovery_category[&RecoveryCategory::RateLimit], 1);
        assert_eq!(
            s.by_recovery_category[&RecoveryCategory::NetworkTransient],
            1
        );
    }

    #[test]
    fn error_entry_serde_roundtrip() {
        let entry = ErrorEntry {
            code: ErrorCode::BackendTimeout,
            category: ErrorCategory::Backend,
            recovery_category: RecoveryCategory::NetworkTransient,
            message: "timed out".into(),
            elapsed_secs: 1.5,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: ErrorEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, entry.code);
        assert_eq!(back.category, entry.category);
        assert_eq!(back.recovery_category, entry.recovery_category);
    }

    #[test]
    fn error_summary_serde_roundtrip() {
        let mut agg = ErrorAggregator::new();
        agg.add(&make_error(ErrorCode::Internal, "i1"));
        let s = agg.summary();
        let json = serde_json::to_string(&s).unwrap();
        let back: ErrorSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total, s.total);
    }

    #[test]
    fn trending_entry_serde_roundtrip() {
        let te = TrendingEntry {
            code: ErrorCode::PolicyDenied,
            count: 42,
        };
        let json = serde_json::to_string(&te).unwrap();
        let back: TrendingEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(te, back);
    }

    #[test]
    fn default_aggregator() {
        let agg = ErrorAggregator::default();
        assert!(agg.is_empty());
    }
}
