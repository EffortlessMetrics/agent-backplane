// SPDX-License-Identifier: MIT OR Apache-2.0

//! Standalone receipt statistics computation for individual receipts and batches.

use abp_core::{AgentEventKind, Outcome, Receipt};
use std::collections::BTreeMap;

/// Statistics derived from a single [`Receipt`].
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::stats::ReceiptStats;
///
/// let r = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .usage_tokens(100, 200)
///     .build();
/// let stats = ReceiptStats::from_receipt(&r);
/// assert_eq!(stats.total_tokens(), Some(300));
/// ```
#[derive(Debug, Clone)]
pub struct ReceiptStats {
    /// Duration of the run in milliseconds.
    pub duration_ms: u64,
    /// Input tokens (if reported).
    pub input_tokens: Option<u64>,
    /// Output tokens (if reported).
    pub output_tokens: Option<u64>,
    /// Cache read tokens (if reported).
    pub cache_read_tokens: Option<u64>,
    /// Cache write tokens (if reported).
    pub cache_write_tokens: Option<u64>,
    /// Number of trace events.
    pub event_count: usize,
    /// Number of error events.
    pub error_count: usize,
    /// Number of tool-use events.
    pub tool_use_count: usize,
    /// Outcome of the run.
    pub outcome: Outcome,
}

impl ReceiptStats {
    /// Compute statistics from a receipt.
    #[must_use]
    pub fn from_receipt(receipt: &Receipt) -> Self {
        let mut error_count = 0usize;
        let mut tool_use_count = 0usize;
        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::Error { .. } => error_count += 1,
                AgentEventKind::ToolCall { .. } => tool_use_count += 1,
                _ => {}
            }
        }

        Self {
            duration_ms: receipt.meta.duration_ms,
            input_tokens: receipt.usage.input_tokens,
            output_tokens: receipt.usage.output_tokens,
            cache_read_tokens: receipt.usage.cache_read_tokens,
            cache_write_tokens: receipt.usage.cache_write_tokens,
            event_count: receipt.trace.len(),
            error_count,
            tool_use_count,
            outcome: receipt.outcome.clone(),
        }
    }

    /// Total tokens (input + output), or `None` if neither is reported.
    #[must_use]
    pub fn total_tokens(&self) -> Option<u64> {
        match (self.input_tokens, self.output_tokens) {
            (Some(i), Some(o)) => Some(i + o),
            (Some(i), None) => Some(i),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        }
    }

    /// Tokens per millisecond throughput, or `None` if tokens or duration unavailable.
    #[must_use]
    pub fn tokens_per_ms(&self) -> Option<f64> {
        if self.duration_ms == 0 {
            return None;
        }
        self.total_tokens()
            .map(|t| t as f64 / self.duration_ms as f64)
    }
}

/// Aggregate statistics across a batch of [`Receipt`]s.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::stats::BatchStats;
///
/// let receipts = vec![
///     ReceiptBuilder::new("a").outcome(Outcome::Complete).usage_tokens(50, 100).build(),
///     ReceiptBuilder::new("b").outcome(Outcome::Failed).usage_tokens(30, 60).build(),
/// ];
/// let stats = BatchStats::from_receipts(&receipts);
/// assert_eq!(stats.total_receipts, 2);
/// assert_eq!(stats.complete_count, 1);
/// assert_eq!(stats.failed_count, 1);
/// assert_eq!(stats.total_input_tokens, 80);
/// ```
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Total receipts in the batch.
    pub total_receipts: usize,
    /// Number of complete receipts.
    pub complete_count: usize,
    /// Number of failed receipts.
    pub failed_count: usize,
    /// Number of partial receipts.
    pub partial_count: usize,
    /// Sum of all durations in milliseconds.
    pub total_duration_ms: u64,
    /// Sum of input tokens across all receipts.
    pub total_input_tokens: u64,
    /// Sum of output tokens across all receipts.
    pub total_output_tokens: u64,
    /// Total trace events across all receipts.
    pub total_events: usize,
    /// Total tool-use events across all receipts.
    pub total_tool_uses: usize,
    /// Total error events across all receipts.
    pub total_errors: usize,
    /// Per-backend receipt counts.
    pub backend_counts: BTreeMap<String, usize>,
    /// Success rate as a fraction (0.0–1.0), or `None` if batch is empty.
    pub success_rate: Option<f64>,
}

impl BatchStats {
    /// Compute aggregate statistics from a slice of receipts.
    #[must_use]
    pub fn from_receipts(receipts: &[Receipt]) -> Self {
        let mut complete_count = 0usize;
        let mut failed_count = 0usize;
        let mut partial_count = 0usize;
        let mut total_duration_ms = 0u64;
        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;
        let mut total_events = 0usize;
        let mut total_tool_uses = 0usize;
        let mut total_errors = 0usize;
        let mut backend_counts: BTreeMap<String, usize> = BTreeMap::new();

        for receipt in receipts {
            match receipt.outcome {
                Outcome::Complete => complete_count += 1,
                Outcome::Failed => failed_count += 1,
                Outcome::Partial => partial_count += 1,
            }

            total_duration_ms += receipt.meta.duration_ms;

            if let Some(t) = receipt.usage.input_tokens {
                total_input_tokens += t;
            }
            if let Some(t) = receipt.usage.output_tokens {
                total_output_tokens += t;
            }

            let stats = ReceiptStats::from_receipt(receipt);
            total_events += stats.event_count;
            total_tool_uses += stats.tool_use_count;
            total_errors += stats.error_count;

            *backend_counts
                .entry(receipt.backend.id.clone())
                .or_insert(0) += 1;
        }

        let success_rate = if receipts.is_empty() {
            None
        } else {
            Some(complete_count as f64 / receipts.len() as f64)
        };

        Self {
            total_receipts: receipts.len(),
            complete_count,
            failed_count,
            partial_count,
            total_duration_ms,
            total_input_tokens,
            total_output_tokens,
            total_events,
            total_tool_uses,
            total_errors,
            backend_counts,
            success_rate,
        }
    }

    /// Average duration per receipt in milliseconds, or `None` if empty.
    #[must_use]
    pub fn avg_duration_ms(&self) -> Option<f64> {
        if self.total_receipts == 0 {
            None
        } else {
            Some(self.total_duration_ms as f64 / self.total_receipts as f64)
        }
    }

    /// Total tokens (input + output) across the batch.
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }
}
