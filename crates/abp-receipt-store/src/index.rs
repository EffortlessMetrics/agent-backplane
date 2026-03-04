// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory index for fast receipt lookup by backend, outcome, and time range.

use std::collections::{BTreeMap, HashMap, HashSet};

use abp_core::{Outcome, Receipt};
use chrono::{DateTime, Utc};

/// In-memory index that accelerates receipt queries by backend, outcome,
/// and time range without requiring a full table scan.
#[derive(Debug, Clone, Default)]
pub struct ReceiptIndex {
    /// backend_id → set of run_id strings
    by_backend: HashMap<String, HashSet<String>>,
    /// outcome (as string) → set of run_id strings
    by_outcome: BTreeMap<String, HashSet<String>>,
    /// started_at → run_id (BTreeMap for range queries)
    by_time: BTreeMap<DateTime<Utc>, Vec<String>>,
}

impl ReceiptIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a receipt to the index.
    pub fn insert(&mut self, receipt: &Receipt) {
        let id = receipt.meta.run_id.to_string();
        self.by_backend
            .entry(receipt.backend.id.clone())
            .or_default()
            .insert(id.clone());
        let outcome_key = format!("{:?}", receipt.outcome);
        self.by_outcome
            .entry(outcome_key)
            .or_default()
            .insert(id.clone());
        self.by_time
            .entry(receipt.meta.started_at)
            .or_default()
            .push(id);
    }

    /// Remove a receipt from the index.
    pub fn remove(&mut self, receipt: &Receipt) {
        let id = receipt.meta.run_id.to_string();
        if let Some(set) = self.by_backend.get_mut(&receipt.backend.id) {
            set.remove(&id);
            if set.is_empty() {
                self.by_backend.remove(&receipt.backend.id);
            }
        }
        let outcome_key = format!("{:?}", receipt.outcome);
        if let Some(set) = self.by_outcome.get_mut(&outcome_key) {
            set.remove(&id);
            if set.is_empty() {
                self.by_outcome.remove(&outcome_key);
            }
        }
        if let Some(ids) = self.by_time.get_mut(&receipt.meta.started_at) {
            ids.retain(|i| i != &id);
            if ids.is_empty() {
                self.by_time.remove(&receipt.meta.started_at);
            }
        }
    }

    /// Query receipt IDs matching a given backend.
    #[must_use]
    pub fn by_backend(&self, backend: &str) -> HashSet<String> {
        self.by_backend.get(backend).cloned().unwrap_or_default()
    }

    /// Query receipt IDs matching a given outcome.
    #[must_use]
    pub fn by_outcome(&self, outcome: &Outcome) -> HashSet<String> {
        let key = format!("{outcome:?}");
        self.by_outcome.get(&key).cloned().unwrap_or_default()
    }

    /// Query receipt IDs whose `started_at` falls within `[from, to]` inclusive.
    #[must_use]
    pub fn by_time_range(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> HashSet<String> {
        let mut result = HashSet::new();
        for (_ts, ids) in self.by_time.range(from..=to) {
            for id in ids {
                result.insert(id.clone());
            }
        }
        result
    }

    /// Total number of unique receipt IDs tracked across all backend entries.
    #[must_use]
    pub fn len(&self) -> usize {
        let mut all = HashSet::new();
        for ids in self.by_backend.values() {
            for id in ids {
                all.insert(id.clone());
            }
        }
        all.len()
    }

    /// Returns `true` if the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_backend.is_empty()
    }
}
