// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pluggable receipt storage with an in-memory reference implementation.

use abp_core::{Outcome, Receipt};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Unique identifier for a stored receipt (wraps the run ID).
pub type ReceiptId = Uuid;

/// Lightweight summary of a stored receipt for listing.
#[derive(Debug, Clone)]
pub struct ReceiptSummary {
    /// The run ID.
    pub id: ReceiptId,
    /// Backend that produced this receipt.
    pub backend_id: String,
    /// High-level outcome.
    pub outcome: Outcome,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run finished.
    pub finished_at: DateTime<Utc>,
}

impl From<&Receipt> for ReceiptSummary {
    fn from(r: &Receipt) -> Self {
        Self {
            id: r.meta.run_id,
            backend_id: r.backend.id.clone(),
            outcome: r.outcome.clone(),
            started_at: r.meta.started_at,
            finished_at: r.meta.finished_at,
        }
    }
}

/// Filter criteria for listing receipts.
#[derive(Debug, Clone, Default)]
pub struct ReceiptFilter {
    /// Only return receipts from this backend.
    pub backend_id: Option<String>,
    /// Only return receipts with this outcome.
    pub outcome: Option<Outcome>,
    /// Only return receipts started at or after this time.
    pub after: Option<DateTime<Utc>>,
    /// Only return receipts started at or before this time.
    pub before: Option<DateTime<Utc>>,
}

/// Errors from receipt store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A receipt with this ID already exists.
    #[error("duplicate receipt id: {0}")]
    DuplicateId(ReceiptId),

    /// Generic storage failure.
    #[error("store error: {0}")]
    Other(String),
}

/// Trait for pluggable receipt storage backends.
pub trait ReceiptStore {
    /// Persist a receipt and return its ID.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::DuplicateId`] if the receipt's run ID already
    /// exists in the store.
    fn store(&mut self, receipt: Receipt) -> Result<ReceiptId, StoreError>;

    /// Retrieve a receipt by run ID.
    fn get(&self, id: ReceiptId) -> Result<Option<&Receipt>, StoreError>;

    /// List receipt summaries matching the given filter.
    fn list(&self, filter: &ReceiptFilter) -> Result<Vec<ReceiptSummary>, StoreError>;
}

/// In-memory receipt store backed by a [`BTreeMap`].
///
/// Receipts are ordered by run ID for deterministic iteration.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::store::{InMemoryReceiptStore, ReceiptStore, ReceiptFilter};
///
/// let mut store = InMemoryReceiptStore::new();
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).with_hash().unwrap();
/// let id = r.meta.run_id;
/// store.store(r).unwrap();
///
/// assert!(store.get(id).unwrap().is_some());
/// let all = store.list(&ReceiptFilter::default()).unwrap();
/// assert_eq!(all.len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct InMemoryReceiptStore {
    receipts: BTreeMap<ReceiptId, Receipt>,
}

impl InMemoryReceiptStore {
    /// Create an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of stored receipts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Returns `true` if the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }
}

impl ReceiptStore for InMemoryReceiptStore {
    fn store(&mut self, receipt: Receipt) -> Result<ReceiptId, StoreError> {
        let id = receipt.meta.run_id;
        if self.receipts.contains_key(&id) {
            return Err(StoreError::DuplicateId(id));
        }
        self.receipts.insert(id, receipt);
        Ok(id)
    }

    fn get(&self, id: ReceiptId) -> Result<Option<&Receipt>, StoreError> {
        Ok(self.receipts.get(&id))
    }

    fn list(&self, filter: &ReceiptFilter) -> Result<Vec<ReceiptSummary>, StoreError> {
        let summaries = self
            .receipts
            .values()
            .filter(|r| {
                if let Some(ref bid) = filter.backend_id {
                    if r.backend.id != *bid {
                        return false;
                    }
                }
                if let Some(ref out) = filter.outcome {
                    if r.outcome != *out {
                        return false;
                    }
                }
                if let Some(after) = filter.after {
                    if r.meta.started_at < after {
                        return false;
                    }
                }
                if let Some(before) = filter.before {
                    if r.meta.started_at > before {
                        return false;
                    }
                }
                true
            })
            .map(ReceiptSummary::from)
            .collect();
        Ok(summaries)
    }
}
