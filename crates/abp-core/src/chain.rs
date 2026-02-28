// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]

//! Receipt chain verification and integrity checking.

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use uuid::Uuid;

use crate::{receipt_hash, Outcome, Receipt};

/// Error type for receipt chain operations.
#[derive(Debug, Clone)]
pub enum ChainError {
    /// A receipt's stored hash does not match the recomputed hash.
    InvalidHash { index: usize },
    /// The chain is empty when a non-empty chain was expected.
    EmptyChain,
    /// A receipt with a duplicate run ID was encountered.
    DuplicateId { id: Uuid },
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHash { index } => {
                write!(f, "invalid hash at chain index {index}")
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
#[derive(Debug, Clone, Default)]
pub struct ReceiptChain {
    receipts: Vec<Receipt>,
    seen_ids: HashSet<Uuid>,
}

impl ReceiptChain {
    /// Create an empty receipt chain.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_core::chain::ReceiptChain;
    /// let chain = ReceiptChain::new();
    /// assert!(chain.is_empty());
    /// assert_eq!(chain.len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a receipt's hash and append it to the chain.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_core::chain::ReceiptChain;
    /// # use abp_core::{ReceiptBuilder, Outcome};
    /// let mut chain = ReceiptChain::new();
    ///
    /// let receipt = ReceiptBuilder::new("mock")
    ///     .outcome(Outcome::Complete)
    ///     .with_hash()
    ///     .unwrap();
    /// chain.push(receipt).unwrap();
    /// assert_eq!(chain.len(), 1);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`ChainError::InvalidHash`] if the stored hash doesn't match the
    /// recomputed hash, or [`ChainError::DuplicateId`] if the run ID is already
    /// in the chain.
    pub fn push(&mut self, receipt: Receipt) -> Result<(), ChainError> {
        let id = receipt.meta.run_id;
        if self.seen_ids.contains(&id) {
            return Err(ChainError::DuplicateId { id });
        }
        verify_receipt_hash(&receipt, self.receipts.len())?;
        self.seen_ids.insert(id);
        self.receipts.push(receipt);
        Ok(())
    }

    /// Verify every receipt hash in the chain.
    ///
    /// # Errors
    ///
    /// Returns [`ChainError::EmptyChain`] if the chain is empty, or
    /// [`ChainError::InvalidHash`] for the first receipt whose hash is invalid.
    pub fn verify(&self) -> Result<(), ChainError> {
        if self.receipts.is_empty() {
            return Err(ChainError::EmptyChain);
        }
        for (i, receipt) in self.receipts.iter().enumerate() {
            verify_receipt_hash(receipt, i)?;
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

    /// Returns an iterator over the receipts in the chain.
    pub fn iter(&self) -> std::slice::Iter<'_, Receipt> {
        self.receipts.iter()
    }

    /// Returns the last receipt in the chain, if any.
    #[must_use]
    pub fn last(&self) -> Option<&Receipt> {
        self.receipts.last()
    }

    /// Find a receipt by its run ID.
    #[must_use]
    pub fn find_by_id(&self, id: &Uuid) -> Option<&Receipt> {
        self.receipts.iter().find(|r| r.meta.run_id == *id)
    }

    /// Find all receipts produced by a given backend.
    #[must_use]
    pub fn find_by_backend(&self, backend: &str) -> Vec<&Receipt> {
        self.receipts
            .iter()
            .filter(|r| r.backend.id == backend)
            .collect()
    }

    /// Sum of event counts across all receipts in the chain.
    #[must_use]
    pub fn total_events(&self) -> usize {
        self.receipts.iter().map(|r| r.trace.len()).sum()
    }

    /// Percentage of receipts with [`Outcome::Complete`] outcome (0.0–1.0).
    ///
    /// Returns 0.0 for an empty chain.
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.receipts.is_empty() {
            return 0.0;
        }
        let successes = self
            .receipts
            .iter()
            .filter(|r| r.outcome == Outcome::Complete)
            .count();
        successes as f64 / self.receipts.len() as f64
    }

    /// Returns the minimum and maximum durations across all receipts.
    ///
    /// Returns `None` if the chain is empty.
    #[must_use]
    pub fn duration_range(&self) -> Option<(Duration, Duration)> {
        if self.receipts.is_empty() {
            return None;
        }
        let mut min = u64::MAX;
        let mut max = 0u64;
        for r in &self.receipts {
            let ms = r.meta.duration_ms;
            if ms < min {
                min = ms;
            }
            if ms > max {
                max = ms;
            }
        }
        Some((
            Duration::from_millis(min),
            Duration::from_millis(max),
        ))
    }
}

impl<'a> IntoIterator for &'a ReceiptChain {
    type Item = &'a Receipt;
    type IntoIter = std::slice::Iter<'a, Receipt>;

    fn into_iter(self) -> Self::IntoIter {
        self.receipts.iter()
    }
}

/// Verify that a single receipt's stored hash matches the recomputed hash.
fn verify_receipt_hash(receipt: &Receipt, index: usize) -> Result<(), ChainError> {
    if let Some(ref stored) = receipt.receipt_sha256 {
        let recomputed =
            receipt_hash(receipt).map_err(|_| ChainError::InvalidHash { index })?;
        if *stored != recomputed {
            return Err(ChainError::InvalidHash { index });
        }
    }
    Ok(())
}
