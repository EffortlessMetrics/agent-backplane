// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ordered receipt chain with integrity verification, tamper detection,
//! gap detection, and chain-level statistics.

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use abp_core::{Outcome, Receipt};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    /// A parent hash reference does not match the preceding receipt's hash.
    ParentMismatch {
        /// Index of the receipt with the mismatched parent.
        index: usize,
    },
    /// A sequence number is not contiguous.
    SequenceGap {
        /// The expected sequence number.
        expected: u64,
        /// The actual sequence number found.
        actual: u64,
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
            Self::ParentMismatch { index } => {
                write!(f, "parent hash mismatch at chain index {index}")
            }
            Self::SequenceGap { expected, actual } => {
                write!(f, "sequence gap: expected {expected}, found {actual}")
            }
        }
    }
}

impl std::error::Error for ChainError {}

// ── TamperEvidence ─────────────────────────────────────────────────

/// Describes the kind of tampering detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TamperKind {
    /// The stored hash does not match the recomputed hash.
    HashMismatch {
        /// The stored hash value.
        stored: String,
        /// The recomputed hash value.
        computed: String,
    },
    /// The parent hash pointer is broken.
    ParentLinkBroken {
        /// Expected parent hash (hash of previous receipt).
        expected: Option<String>,
        /// Actual parent hash recorded in the chain.
        actual: Option<String>,
    },
}

impl fmt::Display for TamperKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashMismatch { stored, computed } => {
                write!(f, "hash mismatch: stored={stored}, computed={computed}")
            }
            Self::ParentLinkBroken { expected, actual } => {
                write!(
                    f,
                    "parent link broken: expected={expected:?}, actual={actual:?}"
                )
            }
        }
    }
}

/// Evidence of tampering at a specific position in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TamperEvidence {
    /// Index of the tampered receipt in the chain.
    pub index: usize,
    /// Sequence number of the tampered entry.
    pub sequence: u64,
    /// What kind of tampering was detected.
    pub kind: TamperKind,
}

impl fmt::Display for TamperEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[index={}, seq={}] {}",
            self.index, self.sequence, self.kind
        )
    }
}

// ── ChainGap ───────────────────────────────────────────────────────

/// A gap in the chain's sequence numbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainGap {
    /// The expected sequence number.
    pub expected: u64,
    /// The actual sequence number found.
    pub actual: u64,
    /// Index in the receipts vector where the gap was detected.
    pub after_index: usize,
}

impl fmt::Display for ChainGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gap after index {}: expected seq {}, found {}",
            self.after_index, self.expected, self.actual
        )
    }
}

// ── ChainSummary ───────────────────────────────────────────────────

/// Aggregated statistics across a receipt chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSummary {
    /// Total number of receipts in the chain.
    pub total_receipts: usize,
    /// Number of receipts with [`Outcome::Complete`].
    pub complete_count: usize,
    /// Number of receipts with [`Outcome::Failed`].
    pub failed_count: usize,
    /// Number of receipts with [`Outcome::Partial`].
    pub partial_count: usize,
    /// Sum of `duration_ms` across all receipts.
    pub total_duration_ms: u64,
    /// Sum of normalized input tokens (where available).
    pub total_input_tokens: u64,
    /// Sum of normalized output tokens (where available).
    pub total_output_tokens: u64,
    /// Distinct backend IDs seen in the chain.
    pub backends: Vec<String>,
    /// Earliest `started_at` in the chain.
    pub first_started_at: Option<DateTime<Utc>>,
    /// Latest `finished_at` in the chain.
    pub last_finished_at: Option<DateTime<Utc>>,
    /// Whether all receipt hashes are valid.
    pub all_hashes_valid: bool,
    /// Number of gaps detected in the sequence.
    pub gap_count: usize,
}

// ── ReceiptChain ───────────────────────────────────────────────────

/// An ordered chain of [`Receipt`]s with integrity verification,
/// tamper detection, gap detection, and chain-level statistics.
///
/// Each receipt pushed into the chain is validated for hash integrity
/// and uniqueness. The chain maintains insertion order and tracks
/// sequence numbers and parent hash linkage.
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
    sequences: Vec<u64>,
    parent_hashes: Vec<Option<String>>,
    next_sequence: u64,
}

impl ReceiptChain {
    /// Create an empty receipt chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and append a receipt to the chain.
    ///
    /// Automatically assigns the next sequence number and records
    /// the parent hash (the hash of the previous receipt, if any).
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

        // Record parent hash (hash of previous receipt).
        let parent_hash = self.receipts.last().and_then(|r| r.receipt_sha256.clone());

        self.seen_ids.insert(id);
        self.sequences.push(self.next_sequence);
        self.parent_hashes.push(parent_hash);
        self.next_sequence += 1;
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

    /// Comprehensive chain verification including hash integrity,
    /// chronological ordering, and parent hash linkage.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered (hash, ordering, or parent link).
    pub fn verify_chain(&self) -> Result<(), ChainError> {
        if self.receipts.is_empty() {
            return Err(ChainError::EmptyChain);
        }
        for (i, receipt) in self.receipts.iter().enumerate() {
            verify_receipt_hash(receipt, i)?;
            if i > 0 {
                if receipt.meta.started_at < self.receipts[i - 1].meta.started_at {
                    return Err(ChainError::BrokenLink { index: i });
                }
                // Verify parent hash linkage.
                let expected_parent = self.receipts[i - 1].receipt_sha256.as_deref();
                let recorded_parent = self.parent_hashes[i].as_deref();
                if expected_parent != recorded_parent {
                    return Err(ChainError::ParentMismatch { index: i });
                }
            }
        }
        // Check sequence contiguity.
        for i in 1..self.sequences.len() {
            if self.sequences[i] != self.sequences[i - 1] + 1 {
                return Err(ChainError::SequenceGap {
                    expected: self.sequences[i - 1] + 1,
                    actual: self.sequences[i],
                });
            }
        }
        Ok(())
    }

    /// Detect all tampering in the chain.
    ///
    /// Unlike [`verify_chain`](Self::verify_chain) which stops at the first
    /// error, this scans the entire chain and collects all evidence.
    #[must_use]
    pub fn detect_tampering(&self) -> Vec<TamperEvidence> {
        let mut evidence = Vec::new();
        for (i, receipt) in self.receipts.iter().enumerate() {
            let seq = self.sequences.get(i).copied().unwrap_or(i as u64);

            // Check hash integrity.
            if let Some(ref stored) = receipt.receipt_sha256 {
                match crate::compute_hash(receipt) {
                    Ok(recomputed) if *stored != recomputed => {
                        evidence.push(TamperEvidence {
                            index: i,
                            sequence: seq,
                            kind: TamperKind::HashMismatch {
                                stored: stored.clone(),
                                computed: recomputed,
                            },
                        });
                    }
                    Err(_) => {
                        evidence.push(TamperEvidence {
                            index: i,
                            sequence: seq,
                            kind: TamperKind::HashMismatch {
                                stored: stored.clone(),
                                computed: "<computation failed>".into(),
                            },
                        });
                    }
                    _ => {}
                }
            }

            // Check parent linkage.
            if i > 0 {
                let expected_parent = self.receipts[i - 1].receipt_sha256.clone();
                let recorded_parent = self.parent_hashes.get(i).cloned().flatten();
                if expected_parent != recorded_parent {
                    evidence.push(TamperEvidence {
                        index: i,
                        sequence: seq,
                        kind: TamperKind::ParentLinkBroken {
                            expected: expected_parent,
                            actual: recorded_parent,
                        },
                    });
                }
            }
        }
        evidence
    }

    /// Find gaps in the sequence numbering.
    ///
    /// Returns an empty vector for a contiguous chain.
    #[must_use]
    pub fn find_gaps(&self) -> Vec<ChainGap> {
        let mut gaps = Vec::new();
        for i in 1..self.sequences.len() {
            let expected = self.sequences[i - 1] + 1;
            let actual = self.sequences[i];
            if actual != expected {
                gaps.push(ChainGap {
                    expected,
                    actual,
                    after_index: i - 1,
                });
            }
        }
        gaps
    }

    /// Compute aggregate statistics across the chain.
    #[must_use]
    pub fn chain_summary(&self) -> ChainSummary {
        let mut complete_count = 0usize;
        let mut failed_count = 0usize;
        let mut partial_count = 0usize;
        let mut total_duration_ms = 0u64;
        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;
        let mut backends = BTreeSet::new();
        let mut first_started_at: Option<DateTime<Utc>> = None;
        let mut last_finished_at: Option<DateTime<Utc>> = None;
        let mut all_hashes_valid = true;

        for receipt in &self.receipts {
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

            backends.insert(receipt.backend.id.clone());

            match first_started_at {
                None => first_started_at = Some(receipt.meta.started_at),
                Some(ref cur) if receipt.meta.started_at < *cur => {
                    first_started_at = Some(receipt.meta.started_at);
                }
                _ => {}
            }
            match last_finished_at {
                None => last_finished_at = Some(receipt.meta.finished_at),
                Some(ref cur) if receipt.meta.finished_at > *cur => {
                    last_finished_at = Some(receipt.meta.finished_at);
                }
                _ => {}
            }

            if let Some(ref stored) = receipt.receipt_sha256 {
                match crate::compute_hash(receipt) {
                    Ok(recomputed) if *stored != recomputed => {
                        all_hashes_valid = false;
                    }
                    Err(_) => all_hashes_valid = false,
                    _ => {}
                }
            }
        }

        ChainSummary {
            total_receipts: self.receipts.len(),
            complete_count,
            failed_count,
            partial_count,
            total_duration_ms,
            total_input_tokens,
            total_output_tokens,
            backends: backends.into_iter().collect(),
            first_started_at,
            last_finished_at,
            all_hashes_valid,
            gap_count: self.find_gaps().len(),
        }
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

    /// Returns a receipt by chain index.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Receipt> {
        self.receipts.get(index)
    }

    /// Returns the sequence number for the given chain index.
    #[must_use]
    pub fn sequence_at(&self, index: usize) -> Option<u64> {
        self.sequences.get(index).copied()
    }

    /// Returns the parent hash for the given chain index.
    #[must_use]
    pub fn parent_hash_at(&self, index: usize) -> Option<&str> {
        self.parent_hashes.get(index).and_then(|h| h.as_deref())
    }

    /// Returns an iterator over the receipts in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Receipt> {
        self.receipts.iter()
    }

    /// Returns the receipts as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[Receipt] {
        &self.receipts
    }
}

impl<'a> IntoIterator for &'a ReceiptChain {
    type Item = &'a Receipt;
    type IntoIter = std::slice::Iter<'a, Receipt>;

    fn into_iter(self) -> Self::IntoIter {
        self.receipts.iter()
    }
}

impl Serialize for ReceiptChain {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.receipts.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReceiptChain {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let receipts = Vec::<Receipt>::deserialize(deserializer)?;
        let mut chain = Self::new();
        for r in receipts {
            chain
                .push(r)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        }
        Ok(chain)
    }
}

// ── ChainBuilder ───────────────────────────────────────────────────

/// Fluent builder for constructing [`ReceiptChain`]s with parent hash
/// linkage and automatic sequence numbering.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ChainBuilder, ReceiptBuilder, Outcome};
///
/// let chain = ChainBuilder::new()
///     .append(
///         ReceiptBuilder::new("mock")
///             .outcome(Outcome::Complete)
///             .with_hash()
///             .unwrap(),
///     )
///     .unwrap()
///     .build();
/// assert_eq!(chain.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ChainBuilder {
    chain: ReceiptChain,
    validate: bool,
}

impl ChainBuilder {
    /// Create a new chain builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chain: ReceiptChain::new(),
            validate: true,
        }
    }

    /// Disable validation on append (useful for constructing test chains).
    #[must_use]
    pub fn skip_validation(mut self) -> Self {
        self.validate = false;
        self
    }

    /// Append a receipt to the chain.
    ///
    /// Validates on append by default (unless [`skip_validation`](Self::skip_validation) was called).
    ///
    /// # Errors
    ///
    /// Returns [`ChainError`] if validation fails.
    pub fn append(mut self, receipt: Receipt) -> Result<Self, ChainError> {
        if self.validate {
            self.chain.push(receipt)?;
        } else {
            self.push_unchecked(receipt);
        }
        Ok(self)
    }

    /// Append a receipt with an explicit sequence number.
    ///
    /// This allows constructing chains with gaps for testing gap detection.
    ///
    /// # Errors
    ///
    /// Returns [`ChainError`] if hash validation fails.
    pub fn append_with_sequence(
        mut self,
        receipt: Receipt,
        sequence: u64,
    ) -> Result<Self, ChainError> {
        let id = receipt.meta.run_id;
        if self.validate && self.chain.seen_ids.contains(&id) {
            return Err(ChainError::DuplicateId { id });
        }
        if self.validate {
            verify_receipt_hash(&receipt, self.chain.receipts.len())?;
        }

        let parent_hash = self
            .chain
            .receipts
            .last()
            .and_then(|r| r.receipt_sha256.clone());

        self.chain.seen_ids.insert(id);
        self.chain.sequences.push(sequence);
        self.chain.parent_hashes.push(parent_hash);
        self.chain.next_sequence = sequence + 1;
        self.chain.receipts.push(receipt);
        Ok(self)
    }

    /// Consume the builder and return the constructed chain.
    #[must_use]
    pub fn build(self) -> ReceiptChain {
        self.chain
    }

    /// Push without any validation (used by `skip_validation` mode).
    fn push_unchecked(&mut self, receipt: Receipt) {
        let parent_hash = self
            .chain
            .receipts
            .last()
            .and_then(|r| r.receipt_sha256.clone());

        self.chain.seen_ids.insert(receipt.meta.run_id);
        self.chain.sequences.push(self.chain.next_sequence);
        self.chain.parent_hashes.push(parent_hash);
        self.chain.next_sequence += 1;
        self.chain.receipts.push(receipt);
    }
}

impl Default for ChainBuilder {
    fn default() -> Self {
        Self::new()
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
