// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ordered receipt chain with integrity verification.

use std::collections::HashSet;
use std::fmt;

use abp_core::Receipt;
use uuid::Uuid;

/// Errors from receipt chain operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// A receipt's stored hash does not match the recomputed hash.
    HashMismatch {
        /// Index of the receipt with the mismatched hash.
        index: usize,
    },
    /// A receipt references a predecessor not present earlier in the chain.
    BrokenLink {
        /// Index of the receipt with the broken link.
        index: usize,
    },
    /// The chain is empty when a non-empty chain was expected.
    EmptyChain,
    /// A receipt with a duplicate run ID was encountered.
    DuplicateId {
        /// The duplicate run ID.
        id: Uuid,
    },
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashMismatch { index } => {
                write!(f, "hash mismatch at chain index {index}")
            }
            Self::BrokenLink { index } => {
                write!(f, "broken link at chain index {index}")
            }
            Self::EmptyChain => write!(f, "chain is empty"),
            Self::DuplicateId { id } => {
                write!(f, "duplicate receipt id: {id}")
            }
        }
    }
}

impl std::error::Error for ChainError {}

/// An ordered chain of [`Receipt`]s with integrity verification.
///
/// Each receipt pushed into the chain is validated for hash integrity
/// and uniqueness. The chain maintains insertion order.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptChain, ReceiptBuilder, Outcome};
///
/// let mut chain = ReceiptChain::new();
/// let r = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// chain.push(r).unwrap();
/// assert_eq!(chain.len(), 1);
/// assert!(chain.verify().is_ok());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReceiptChain {
    receipts: Vec<Receipt>,
    seen_ids: HashSet<Uuid>,
}

impl ReceiptChain {
    /// Create an empty receipt chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and append a receipt to the chain.
    ///
    /// # Errors
    ///
    /// - [`ChainError::HashMismatch`] if the stored hash doesn't match.
    /// - [`ChainError::DuplicateId`] if the run ID already exists.
    /// - [`ChainError::BrokenLink`] if chronological ordering is violated.
    pub fn push(&mut self, receipt: Receipt) -> Result<(), ChainError> {
        let id = receipt.meta.run_id;
        if self.seen_ids.contains(&id) {
            return Err(ChainError::DuplicateId { id });
        }

        // Verify hash integrity.
        verify_receipt_hash(&receipt, self.receipts.len())?;

        // Check chronological ordering against the previous entry.
        if let Some(last) = self.receipts.last()
            && receipt.meta.started_at < last.meta.started_at
        {
            return Err(ChainError::BrokenLink {
                index: self.receipts.len(),
            });
        }

        self.seen_ids.insert(id);
        self.receipts.push(receipt);
        Ok(())
    }

    /// Verify all receipt hashes and ordering in the chain.
    ///
    /// # Errors
    ///
    /// - [`ChainError::EmptyChain`] if the chain is empty.
    /// - [`ChainError::HashMismatch`] for the first broken hash.
    /// - [`ChainError::BrokenLink`] for the first ordering violation.
    pub fn verify(&self) -> Result<(), ChainError> {
        if self.receipts.is_empty() {
            return Err(ChainError::EmptyChain);
        }
        for (i, receipt) in self.receipts.iter().enumerate() {
            verify_receipt_hash(receipt, i)?;
            if i > 0 && receipt.meta.started_at < self.receipts[i - 1].meta.started_at {
                return Err(ChainError::BrokenLink { index: i });
            }
        }
        Ok(())
    }

    /// Returns the number of receipts in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    /// Returns `true` if the chain contains no receipts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    /// Returns the last (most recent) receipt, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&Receipt> {
        self.receipts.last()
    }

    /// Returns an iterator over the receipts in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Receipt> {
        self.receipts.iter()
    }
}

impl<'a> IntoIterator for &'a ReceiptChain {
    type Item = &'a Receipt;
    type IntoIter = std::slice::Iter<'a, Receipt>;

    fn into_iter(self) -> Self::IntoIter {
        self.receipts.iter()
    }
}

/// Verify a single receipt's hash integrity.
fn verify_receipt_hash(receipt: &Receipt, index: usize) -> Result<(), ChainError> {
    if let Some(ref stored) = receipt.receipt_sha256 {
        let recomputed =
            crate::compute_hash(receipt).map_err(|_| ChainError::HashMismatch { index })?;
        if *stored != recomputed {
            return Err(ChainError::HashMismatch { index });
        }
    }
    Ok(())
}
