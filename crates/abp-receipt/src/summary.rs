// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt aggregation summary — success rate, average duration, total
//! tokens, error distribution, and most common backends.

use abp_core::{AgentEventKind, Outcome, Receipt};
use std::collections::BTreeMap;

/// Aggregated statistics across a collection of receipts.
///
/// # Examples
///
/// ```
/// use abp_receipt::summary::AggregateSummary;
/// use abp_receipt::{ReceiptBuilder, Outcome};
///
/// let receipts = vec![
///     ReceiptBuilder::new("a").outcome(Outcome::Complete).usage_tokens(50, 100).build(),
///     ReceiptBuilder::new("b").outcome(Outcome::Failed).error("boom").build(),
/// ];
/// let summary = AggregateSummary::from_receipts(&receipts);
/// assert_eq!(summary.total_receipts, 2);
/// assert!((summary.success_rate - 0.5).abs() < f64::EPSILON);
/// ```
#[derive(Debug, Clone)]
pub struct AggregateSummary {
    /// Total receipts considered.
    pub total_receipts: usize,
    /// Fraction of receipts with [`Outcome::Complete`] (0.0–1.0).
    pub success_rate: f64,
    /// Average duration in milliseconds, or 0.0 if empty.
    pub avg_duration_ms: f64,
    /// Sum of input + output tokens across all receipts.
    pub total_tokens: u64,
    /// Total input tokens.
    pub total_input_tokens: u64,
    /// Total output tokens.
    pub total_output_tokens: u64,
    /// Counts of error messages seen across all traces.
    pub error_distribution: BTreeMap<String, usize>,
    /// Per-backend receipt counts, sorted by backend ID.
    pub backend_distribution: BTreeMap<String, usize>,
    /// The backend ID with the most receipts, if any.
    pub most_common_backend: Option<String>,
    /// Total duration across all receipts.
    pub total_duration_ms: u64,
    /// Number of complete receipts.
    pub complete_count: usize,
    /// Number of failed receipts.
    pub failed_count: usize,
    /// Number of partial receipts.
    pub partial_count: usize,
}

impl AggregateSummary {
    /// Compute an aggregate summary from a slice of receipts.
    #[must_use]
    pub fn from_receipts(receipts: &[Receipt]) -> Self {
        let mut complete = 0usize;
        let mut failed = 0usize;
        let mut partial = 0usize;
        let mut total_duration_ms = 0u64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut error_dist: BTreeMap<String, usize> = BTreeMap::new();
        let mut backend_dist: BTreeMap<String, usize> = BTreeMap::new();

        for r in receipts {
            match r.outcome {
                Outcome::Complete => complete += 1,
                Outcome::Failed => failed += 1,
                Outcome::Partial => partial += 1,
            }

            total_duration_ms += r.meta.duration_ms;

            if let Some(t) = r.usage.input_tokens {
                total_input += t;
            }
            if let Some(t) = r.usage.output_tokens {
                total_output += t;
            }

            for event in &r.trace {
                if let AgentEventKind::Error { ref message, .. } = event.kind {
                    *error_dist.entry(message.clone()).or_insert(0) += 1;
                }
            }

            *backend_dist.entry(r.backend.id.clone()).or_insert(0) += 1;
        }

        let n = receipts.len();
        let success_rate = if n == 0 {
            0.0
        } else {
            complete as f64 / n as f64
        };
        let avg_duration_ms = if n == 0 {
            0.0
        } else {
            total_duration_ms as f64 / n as f64
        };

        let most_common_backend = backend_dist
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(id, _)| id.clone());

        Self {
            total_receipts: n,
            success_rate,
            avg_duration_ms,
            total_tokens: total_input + total_output,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            error_distribution: error_dist,
            backend_distribution: backend_dist,
            most_common_backend,
            total_duration_ms,
            complete_count: complete,
            failed_count: failed,
            partial_count: partial,
        }
    }
}
