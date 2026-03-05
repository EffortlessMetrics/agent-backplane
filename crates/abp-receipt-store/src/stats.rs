// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt aggregation and statistics.

use std::collections::BTreeMap;

use abp_core::{Outcome, Receipt};

/// Aggregated statistics computed from a set of receipts.
#[derive(Debug, Clone, Default)]
pub struct ReceiptStats {
    /// Total number of receipts.
    pub total: usize,
    /// Count of receipts per outcome.
    pub by_outcome: BTreeMap<String, usize>,
    /// Count of receipts per backend.
    pub by_backend: BTreeMap<String, usize>,
    /// Average duration in milliseconds (if any receipts exist).
    pub avg_duration_ms: Option<f64>,
    /// Minimum duration in milliseconds.
    pub min_duration_ms: Option<u64>,
    /// Maximum duration in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Total input tokens across all receipts.
    pub total_input_tokens: u64,
    /// Total output tokens across all receipts.
    pub total_output_tokens: u64,
    /// Success rate as a fraction in `[0.0, 1.0]`.
    pub success_rate: Option<f64>,
}

impl ReceiptStats {
    /// Compute aggregate statistics from a slice of receipts.
    #[must_use]
    pub fn from_receipts(receipts: &[Receipt]) -> Self {
        if receipts.is_empty() {
            return Self::default();
        }

        let mut by_outcome: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_backend: BTreeMap<String, usize> = BTreeMap::new();
        let mut total_duration: u64 = 0;
        let mut min_dur: u64 = u64::MAX;
        let mut max_dur: u64 = 0;
        let mut total_input: u64 = 0;
        let mut total_output: u64 = 0;
        let mut complete_count: usize = 0;

        for r in receipts {
            let outcome_key = format!("{:?}", r.outcome);
            *by_outcome.entry(outcome_key).or_default() += 1;
            *by_backend.entry(r.backend.id.clone()).or_default() += 1;

            total_duration += r.meta.duration_ms;
            if r.meta.duration_ms < min_dur {
                min_dur = r.meta.duration_ms;
            }
            if r.meta.duration_ms > max_dur {
                max_dur = r.meta.duration_ms;
            }

            if let Some(t) = r.usage.input_tokens {
                total_input += t;
            }
            if let Some(t) = r.usage.output_tokens {
                total_output += t;
            }

            if r.outcome == Outcome::Complete {
                complete_count += 1;
            }
        }

        let total = receipts.len();
        Self {
            total,
            by_outcome,
            by_backend,
            avg_duration_ms: Some(total_duration as f64 / total as f64),
            min_duration_ms: Some(min_dur),
            max_duration_ms: Some(max_dur),
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            success_rate: Some(complete_count as f64 / total as f64),
        }
    }
}
