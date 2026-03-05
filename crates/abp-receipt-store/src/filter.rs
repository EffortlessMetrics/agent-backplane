// SPDX-License-Identifier: MIT OR Apache-2.0

//! Filter criteria for listing receipts.

use abp_core::Outcome;
use chrono::{DateTime, Utc};

/// Filter criteria for querying receipts.
#[derive(Debug, Clone, Default)]
pub struct ReceiptFilter {
    /// Only return receipts with this outcome.
    pub outcome: Option<Outcome>,
    /// Only return receipts from this backend.
    pub backend: Option<String>,
    /// Only return receipts whose `started_at` falls within this range (inclusive).
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Only return receipts for this work order ID.
    pub work_order_id: Option<String>,
    /// Case-insensitive substring search across backend ID and contract version.
    pub text_search: Option<String>,
    /// Minimum duration in milliseconds.
    pub min_duration_ms: Option<u64>,
    /// Maximum duration in milliseconds.
    pub max_duration_ms: Option<u64>,
    /// Maximum number of results to return.
    pub limit: Option<usize>,
    /// Number of results to skip (for pagination).
    pub offset: Option<usize>,
}

impl ReceiptFilter {
    /// Returns `true` if the receipt matches all active filter criteria
    /// (ignoring limit/offset, which are applied at the collection level).
    pub(crate) fn matches(&self, receipt: &abp_core::Receipt) -> bool {
        if let Some(ref outcome) = self.outcome {
            if receipt.outcome != *outcome {
                return false;
            }
        }
        if let Some(ref backend) = self.backend {
            if receipt.backend.id != *backend {
                return false;
            }
        }
        if let Some((start, end)) = self.time_range {
            if receipt.meta.started_at < start || receipt.meta.started_at > end {
                return false;
            }
        }
        if let Some(ref woid) = self.work_order_id {
            if receipt.meta.work_order_id.to_string() != *woid {
                return false;
            }
        }
        if let Some(ref needle) = self.text_search {
            let needle_lower = needle.to_lowercase();
            let haystack = [
                receipt.backend.id.as_str(),
                receipt.meta.contract_version.as_str(),
                &format!("{:?}", receipt.outcome),
            ];
            if !haystack
                .iter()
                .any(|s| s.to_lowercase().contains(&needle_lower))
            {
                return false;
            }
        }
        if let Some(min) = self.min_duration_ms {
            if receipt.meta.duration_ms < min {
                return false;
            }
        }
        if let Some(max) = self.max_duration_ms {
            if receipt.meta.duration_ms > max {
                return false;
            }
        }
        true
    }

    /// Apply limit and offset to a vec of receipts.
    pub(crate) fn paginate<T>(&self, items: Vec<T>) -> Vec<T> {
        let offset = self.offset.unwrap_or(0);
        let iter = items.into_iter().skip(offset);
        match self.limit {
            Some(limit) => iter.take(limit).collect(),
            None => iter.collect(),
        }
    }
}
