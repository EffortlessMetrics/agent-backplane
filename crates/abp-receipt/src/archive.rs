// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-memory receipt archive with rich querying by work-order ID, backend,
//! and time range.

use abp_core::Receipt;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Search criteria for [`ReceiptArchive::search`].
#[derive(Debug, Clone, Default)]
pub struct ArchiveQuery {
    /// Filter by the work-order that produced the receipt.
    pub work_order_id: Option<Uuid>,
    /// Filter by backend identifier.
    pub backend_id: Option<String>,
    /// Only include receipts started at or after this time.
    pub after: Option<DateTime<Utc>>,
    /// Only include receipts started at or before this time.
    pub before: Option<DateTime<Utc>>,
}

/// Errors from archive operations.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    /// A receipt with this run ID already exists.
    #[error("duplicate receipt id: {0}")]
    DuplicateId(Uuid),
}

/// In-memory receipt archive keyed by run ID.
///
/// Unlike [`crate::store::InMemoryReceiptStore`], the archive exposes
/// `store` / `retrieve` / `search` semantics with work-order-level
/// querying.
///
/// # Examples
///
/// ```
/// use abp_receipt::archive::{ReceiptArchive, ArchiveQuery};
/// use abp_receipt::{ReceiptBuilder, Outcome};
///
/// let mut archive = ReceiptArchive::new();
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let id = r.meta.run_id;
/// archive.store(r).unwrap();
///
/// assert!(archive.retrieve(id).is_some());
/// let all = archive.search(&ArchiveQuery::default());
/// assert_eq!(all.len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReceiptArchive {
    receipts: BTreeMap<Uuid, Receipt>,
}

impl ReceiptArchive {
    /// Create an empty archive.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Persist a receipt in the archive.
    ///
    /// # Errors
    ///
    /// Returns [`ArchiveError::DuplicateId`] if a receipt with the same
    /// run ID already exists.
    pub fn store(&mut self, receipt: Receipt) -> Result<Uuid, ArchiveError> {
        let id = receipt.meta.run_id;
        if self.receipts.contains_key(&id) {
            return Err(ArchiveError::DuplicateId(id));
        }
        self.receipts.insert(id, receipt);
        Ok(id)
    }

    /// Retrieve a receipt by its run ID.
    #[must_use]
    pub fn retrieve(&self, id: Uuid) -> Option<&Receipt> {
        self.receipts.get(&id)
    }

    /// Search the archive using the provided query criteria.
    ///
    /// An empty [`ArchiveQuery`] returns all stored receipts.
    #[must_use]
    pub fn search(&self, query: &ArchiveQuery) -> Vec<&Receipt> {
        self.receipts
            .values()
            .filter(|r| {
                if let Some(woid) = query.work_order_id {
                    if r.meta.work_order_id != woid {
                        return false;
                    }
                }
                if let Some(ref bid) = query.backend_id {
                    if r.backend.id != *bid {
                        return false;
                    }
                }
                if let Some(after) = query.after {
                    if r.meta.started_at < after {
                        return false;
                    }
                }
                if let Some(before) = query.before {
                    if r.meta.started_at > before {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Returns the number of archived receipts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Returns `true` if the archive is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }
}
