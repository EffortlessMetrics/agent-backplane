// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt retention policies — max count, max age, max total size.

use std::time::Duration;

use abp_core::Receipt;
use chrono::Utc;

/// Retention policy for receipt pruning.
///
/// Any field set to `None` means that criterion is not enforced.
/// When multiple criteria are set, receipts are pruned if *any* criterion
/// is exceeded (union, not intersection).
#[derive(Debug, Clone, Default)]
pub struct ReceiptRetention {
    /// Maximum number of receipts to keep. Oldest are pruned first.
    pub max_count: Option<usize>,
    /// Maximum age of a receipt. Receipts older than this are pruned.
    pub max_age: Option<Duration>,
    /// Maximum total serialized size in bytes. Oldest are pruned first.
    pub max_total_bytes: Option<u64>,
}

/// Result of applying a retention policy.
#[derive(Debug, Clone)]
pub struct RetentionResult {
    /// Number of receipts kept.
    pub kept: usize,
    /// Number of receipts pruned.
    pub pruned: usize,
    /// IDs of the pruned receipts.
    pub pruned_ids: Vec<String>,
}

impl ReceiptRetention {
    /// Create a retention policy with no limits.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Create a retention policy with a maximum receipt count.
    #[must_use]
    pub fn with_max_count(mut self, count: usize) -> Self {
        self.max_count = Some(count);
        self
    }

    /// Create a retention policy with a maximum age.
    #[must_use]
    pub fn with_max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// Create a retention policy with a maximum total serialized size.
    #[must_use]
    pub fn with_max_total_bytes(mut self, bytes: u64) -> Self {
        self.max_total_bytes = Some(bytes);
        self
    }

    /// Determine which receipts to keep and which to prune.
    ///
    /// Receipts should be sorted oldest-first (by `started_at`).
    /// Returns the kept receipts and a [`RetentionResult`] describing the pruning.
    #[must_use]
    pub fn apply(&self, mut receipts: Vec<Receipt>) -> (Vec<Receipt>, RetentionResult) {
        receipts.sort_by_key(|r| r.meta.started_at);

        let mut pruned_ids = Vec::new();

        // Phase 1: prune by age.
        if let Some(max_age) = self.max_age {
            let cutoff = Utc::now() - chrono::Duration::from_std(max_age).unwrap_or_default();
            let mut keep = Vec::new();
            for r in receipts {
                if r.meta.started_at < cutoff {
                    pruned_ids.push(r.meta.run_id.to_string());
                } else {
                    keep.push(r);
                }
            }
            receipts = keep;
        }

        // Phase 2: prune by total size (oldest first).
        if let Some(max_bytes) = self.max_total_bytes {
            let mut total: u64 = 0;
            let mut keep = Vec::new();
            // Walk newest-first so we keep newest within budget.
            for r in receipts.into_iter().rev() {
                let size = serde_json::to_string(&r)
                    .map(|s| s.len() as u64)
                    .unwrap_or(0);
                if total + size <= max_bytes {
                    total += size;
                    keep.push(r);
                } else {
                    pruned_ids.push(r.meta.run_id.to_string());
                }
            }
            keep.reverse();
            receipts = keep;
        }

        // Phase 3: prune by count (keep newest).
        if let Some(max_count) = self.max_count {
            if receipts.len() > max_count {
                let to_prune = receipts.len() - max_count;
                for r in receipts.drain(..to_prune) {
                    pruned_ids.push(r.meta.run_id.to_string());
                }
            }
        }

        let kept = receipts.len();
        let pruned = pruned_ids.len();
        (
            receipts,
            RetentionResult {
                kept,
                pruned,
                pruned_ids,
            },
        )
    }
}
