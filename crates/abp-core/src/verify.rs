// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive receipt and chain verification.
//!
//! This module provides two levels of verification:
//!
//! 1. **Individual receipt verification** via `ReceiptVerifier` — checks hash
//!    integrity, contract version, timestamps, trace ordering, etc.
//! 2. **Chain verification** via `verify_chain()` — validates an ordered
//!    `ReceiptChain` with parent→child relationships for multi-step workflows.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AgentEventKind, CONTRACT_VERSION, Receipt, receipt_hash};

/// Result of a single verification check.
#[derive(Debug, Clone)]
pub struct VerificationCheck {
    /// Human-readable name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Detail message explaining the result.
    pub detail: String,
}

/// Aggregated verification result for a single receipt.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// The run ID of the verified receipt.
    pub receipt_id: String,
    /// Individual check results.
    pub checks: Vec<VerificationCheck>,
    /// `true` only when every check passed.
    pub passed: bool,
}

/// Aggregated verification result for an ordered chain of receipts.
#[derive(Debug, Clone)]
pub struct ChainVerificationReport {
    /// Number of receipts in the chain.
    pub receipt_count: usize,
    /// `true` only when all individual and chain-level checks pass.
    pub all_valid: bool,
    /// Per-receipt verification reports.
    pub individual_reports: Vec<VerificationReport>,
    /// Chain-level checks (ordering, duplicates, version consistency).
    pub chain_checks: Vec<VerificationCheck>,
}

/// Verifies individual [`Receipt`]s for integrity and completeness.
#[derive(Debug, Clone, Default)]
pub struct ReceiptVerifier {
    _private: (),
}

impl ReceiptVerifier {
    /// Create a new verifier.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run all verification checks against a single receipt.
    #[must_use]
    pub fn verify(&self, receipt: &Receipt) -> VerificationReport {
        let checks = vec![
            self.check_hash_integrity(receipt),
            self.check_contract_version(receipt),
            self.check_work_order_id(receipt),
            self.check_run_id(receipt),
            self.check_outcome(receipt),
            self.check_backend(receipt),
            self.check_timestamps(receipt),
            self.check_trace_order(receipt),
            self.check_trace_duplicate_ids(receipt),
        ];

        let passed = checks.iter().all(|c| c.passed);
        VerificationReport {
            receipt_id: receipt.meta.run_id.to_string(),
            checks,
            passed,
        }
    }

    fn check_hash_integrity(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "hash_integrity".to_string();
        match &receipt.receipt_sha256 {
            None => VerificationCheck {
                name,
                passed: true,
                detail: "no hash present; skipped".into(),
            },
            Some(stored) => match receipt_hash(receipt) {
                Ok(recomputed) if *stored == recomputed => VerificationCheck {
                    name,
                    passed: true,
                    detail: "hash matches".into(),
                },
                Ok(recomputed) => VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("expected {recomputed}, got {stored}"),
                },
                Err(e) => VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("failed to recompute hash: {e}"),
                },
            },
        }
    }

    fn check_contract_version(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "contract_version".to_string();
        let ver = &receipt.meta.contract_version;
        if ver.is_empty() {
            return VerificationCheck {
                name,
                passed: false,
                detail: "contract version is empty".into(),
            };
        }
        // Expect format "abp/vX.Y"
        let valid_format = ver.starts_with("abp/v")
            && ver[5..].contains('.')
            && ver[5..].split('.').all(|p| !p.is_empty());
        if !valid_format {
            return VerificationCheck {
                name,
                passed: false,
                detail: format!("invalid format: \"{ver}\""),
            };
        }
        if *ver != CONTRACT_VERSION {
            return VerificationCheck {
                name,
                passed: true,
                detail: format!(
                    "valid format but differs from current ({CONTRACT_VERSION}): \"{ver}\""
                ),
            };
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("matches current contract version ({CONTRACT_VERSION})"),
        }
    }

    fn check_work_order_id(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "work_order_id".to_string();
        if receipt.meta.work_order_id == Uuid::nil() {
            VerificationCheck {
                name,
                passed: false,
                detail: "work order ID is nil UUID".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("valid UUID: {}", receipt.meta.work_order_id),
            }
        }
    }

    fn check_run_id(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "run_id".to_string();
        if receipt.meta.run_id == Uuid::nil() {
            VerificationCheck {
                name,
                passed: false,
                detail: "run ID is nil UUID".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("valid UUID: {}", receipt.meta.run_id),
            }
        }
    }

    fn check_outcome(&self, receipt: &Receipt) -> VerificationCheck {
        // Outcome is an enum so it's always a recognized variant if deserialized.
        VerificationCheck {
            name: "outcome".to_string(),
            passed: true,
            detail: format!("recognized variant: {:?}", receipt.outcome),
        }
    }

    fn check_backend(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "backend".to_string();
        if receipt.backend.id.is_empty() {
            VerificationCheck {
                name,
                passed: false,
                detail: "backend ID is empty".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("backend present: \"{}\"", receipt.backend.id),
            }
        }
    }

    fn check_timestamps(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "timestamps".to_string();
        if receipt.meta.started_at > receipt.meta.finished_at {
            VerificationCheck {
                name,
                passed: false,
                detail: format!(
                    "started_at ({}) is after finished_at ({})",
                    receipt.meta.started_at, receipt.meta.finished_at
                ),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: "started_at <= finished_at".into(),
            }
        }
    }

    fn check_trace_order(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "trace_order".to_string();
        if receipt.trace.len() < 2 {
            return VerificationCheck {
                name,
                passed: true,
                detail: "fewer than 2 trace events; ordering trivially valid".into(),
            };
        }
        for i in 1..receipt.trace.len() {
            if receipt.trace[i].ts < receipt.trace[i - 1].ts {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "event {} timestamp ({}) precedes event {} ({})",
                        i,
                        receipt.trace[i].ts,
                        i - 1,
                        receipt.trace[i - 1].ts,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} trace events in order", receipt.trace.len()),
        }
    }

    fn check_trace_duplicate_ids(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "trace_no_duplicate_ids".to_string();
        let mut seen = HashSet::new();
        for event in &receipt.trace {
            let id = match &event.kind {
                AgentEventKind::ToolCall { tool_use_id, .. } => tool_use_id.as_deref(),
                AgentEventKind::ToolResult { tool_use_id, .. } => tool_use_id.as_deref(),
                _ => None,
            };
            if let Some(id) = id
                && !seen.insert(id.to_string())
            {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("duplicate tool_use_id: \"{id}\""),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("no duplicate IDs among {} events", receipt.trace.len()),
        }
    }
}

/// Verifies an ordered chain of [`Receipt`]s for consistency.
pub struct ChainVerifier;

impl ChainVerifier {
    /// Verify an ordered slice of receipts as a chain.
    #[must_use]
    pub fn verify_chain(chain: &[Receipt]) -> ChainVerificationReport {
        let verifier = ReceiptVerifier::new();
        let individual_reports: Vec<VerificationReport> =
            chain.iter().map(|r| verifier.verify(r)).collect();

        let chain_checks = vec![
            Self::check_chain_order(chain),
            Self::check_no_duplicate_run_ids(chain),
            Self::check_consistent_version(chain),
        ];

        let all_individual_pass = individual_reports.iter().all(|r| r.passed);
        let all_chain_pass = chain_checks.iter().all(|c| c.passed);

        ChainVerificationReport {
            receipt_count: chain.len(),
            all_valid: all_individual_pass && all_chain_pass,
            individual_reports,
            chain_checks,
        }
    }

    fn check_chain_order(chain: &[Receipt]) -> VerificationCheck {
        let name = "chain_order".to_string();
        if chain.len() < 2 {
            return VerificationCheck {
                name,
                passed: true,
                detail: "fewer than 2 receipts; ordering trivially valid".into(),
            };
        }
        for i in 1..chain.len() {
            if chain[i].meta.started_at < chain[i - 1].meta.started_at {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "receipt {} started_at ({}) precedes receipt {} ({})",
                        i,
                        chain[i].meta.started_at,
                        i - 1,
                        chain[i - 1].meta.started_at,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} receipts in chronological order", chain.len()),
        }
    }

    fn check_no_duplicate_run_ids(chain: &[Receipt]) -> VerificationCheck {
        let name = "no_duplicate_run_ids".to_string();
        let mut seen = HashSet::new();
        for receipt in chain {
            let id = receipt.meta.run_id;
            if !seen.insert(id) {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("duplicate run ID: {id}"),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} unique run IDs", seen.len()),
        }
    }

    fn check_consistent_version(chain: &[Receipt]) -> VerificationCheck {
        let name = "consistent_contract_version".to_string();
        if chain.is_empty() {
            return VerificationCheck {
                name,
                passed: true,
                detail: "empty chain".into(),
            };
        }
        let first = &chain[0].meta.contract_version;
        for (i, receipt) in chain.iter().enumerate().skip(1) {
            if receipt.meta.contract_version != *first {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "receipt {} has version \"{}\" but receipt 0 has \"{first}\"",
                        i, receipt.meta.contract_version,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("all receipts use version \"{first}\""),
        }
    }
}

// ── Chain verification with parent→child relationships ─────────────

/// Errors discovered during [`ReceiptChain`] verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChainError {
    /// A receipt's stored hash does not match the recomputed hash.
    BrokenHash {
        /// Position of the receipt in the chain.
        index: usize,
        /// Run ID of the receipt with the broken hash.
        run_id: Uuid,
    },
    /// A receipt references a parent run ID not present in the chain.
    MissingParent {
        /// Position of the receipt in the chain.
        index: usize,
        /// The missing parent run ID.
        parent_id: Uuid,
    },
    /// Receipts are not in chronological order by `started_at`.
    OutOfOrder {
        /// Position of the out-of-order receipt.
        index: usize,
    },
    /// Two or more receipts share the same run ID.
    DuplicateId {
        /// The duplicated run ID.
        id: Uuid,
    },
    /// Contract versions differ between receipts in the chain.
    ContractVersionMismatch {
        /// Position of the mismatched receipt.
        index: usize,
        /// The expected contract version (from the first receipt).
        expected: String,
        /// The actual contract version found.
        actual: String,
    },
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BrokenHash { index, run_id } => {
                write!(f, "broken hash at index {index} (run_id={run_id})")
            }
            Self::MissingParent { index, parent_id } => {
                write!(f, "missing parent {parent_id} referenced at index {index}")
            }
            Self::OutOfOrder { index } => {
                write!(f, "receipt at index {index} is out of chronological order")
            }
            Self::DuplicateId { id } => write!(f, "duplicate run ID: {id}"),
            Self::ContractVersionMismatch {
                index,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "contract version mismatch at index {index}: expected \"{expected}\", got \"{actual}\""
                )
            }
        }
    }
}

impl std::error::Error for ChainError {}

/// A receipt entry in a [`ReceiptChain`] with an optional parent link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    /// The receipt for this step in the workflow.
    pub receipt: Receipt,
    /// Run ID of the parent receipt, if this is a child step.
    pub parent_id: Option<Uuid>,
}

/// An ordered sequence of receipts with parent→child relationships.
///
/// Each entry pairs a [`Receipt`] with an optional parent run ID,
/// forming a DAG of multi-step agent workflows.
///
/// Use [`ChainBuilder`] for ergonomic construction.
///
/// # Examples
///
/// ```
/// use abp_core::verify::{ChainBuilder, verify_chain};
/// use abp_core::{ReceiptBuilder, Outcome};
///
/// let r1 = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// let parent_id = r1.meta.run_id;
///
/// let r2 = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
///
/// let chain = ChainBuilder::new()
///     .push(r1)
///     .push_child(r2, parent_id)
///     .build();
///
/// let result = verify_chain(&chain);
/// assert!(result.valid);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReceiptChain {
    entries: Vec<ChainEntry>,
}

impl ReceiptChain {
    /// Create an empty receipt chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of entries in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the chain contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a slice of all entries.
    #[must_use]
    pub fn entries(&self) -> &[ChainEntry] {
        &self.entries
    }

    /// Returns an iterator over the chain entries.
    pub fn iter(&self) -> std::slice::Iter<'_, ChainEntry> {
        self.entries.iter()
    }
}

impl<'a> IntoIterator for &'a ReceiptChain {
    type Item = &'a ChainEntry;
    type IntoIter = std::slice::Iter<'a, ChainEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// Builder for constructing a [`ReceiptChain`] incrementally.
///
/// # Examples
///
/// ```
/// use abp_core::verify::ChainBuilder;
/// use abp_core::{ReceiptBuilder, Outcome};
///
/// let chain = ChainBuilder::new()
///     .push(ReceiptBuilder::new("mock").outcome(Outcome::Complete).build())
///     .build();
/// assert_eq!(chain.len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct ChainBuilder {
    entries: Vec<ChainEntry>,
}

impl ChainBuilder {
    /// Create a new empty chain builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a receipt with no parent link.
    #[must_use]
    pub fn push(mut self, receipt: Receipt) -> Self {
        self.entries.push(ChainEntry {
            receipt,
            parent_id: None,
        });
        self
    }

    /// Append a receipt as a child of the given parent run ID.
    #[must_use]
    pub fn push_child(mut self, receipt: Receipt, parent_id: Uuid) -> Self {
        self.entries.push(ChainEntry {
            receipt,
            parent_id: Some(parent_id),
        });
        self
    }

    /// Consume the builder and produce a [`ReceiptChain`].
    #[must_use]
    pub fn build(self) -> ReceiptChain {
        ReceiptChain {
            entries: self.entries,
        }
    }
}

/// Aggregated result of verifying a [`ReceiptChain`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerification {
    /// `true` when the chain has no errors.
    pub valid: bool,
    /// All errors found during verification.
    pub errors: Vec<ChainError>,
    /// Number of receipts in the chain.
    pub chain_length: usize,
    /// Total number of trace events across all receipts.
    pub total_events: usize,
    /// Sum of `duration_ms` across all receipts.
    pub total_duration_ms: u64,
}

/// Verify a [`ReceiptChain`] for integrity and consistency.
///
/// Checks performed:
/// - No duplicate run IDs
/// - Chronological ordering by `started_at`
/// - Hash integrity for each receipt that carries a hash
/// - All parent references resolve to receipts in the chain
/// - Consistent contract version across all receipts
///
/// Returns a [`ChainVerification`] with aggregated statistics.
#[must_use]
pub fn verify_chain(chain: &ReceiptChain) -> ChainVerification {
    let mut errors = Vec::new();
    let chain_length = chain.len();
    let mut total_events: usize = 0;
    let mut total_duration_ms: u64 = 0;

    // First pass: collect IDs, accumulate stats, detect duplicates.
    let mut seen_ids = HashSet::new();
    for entry in &chain.entries {
        let id = entry.receipt.meta.run_id;
        total_events += entry.receipt.trace.len();
        total_duration_ms = total_duration_ms.saturating_add(entry.receipt.meta.duration_ms);

        if !seen_ids.insert(id) {
            errors.push(ChainError::DuplicateId { id });
        }
    }

    // Second pass: ordering, hashes, parents, version consistency.
    let expected_version = chain
        .entries
        .first()
        .map(|e| &e.receipt.meta.contract_version);

    for (i, entry) in chain.entries.iter().enumerate() {
        // Chronological order.
        if i > 0 && entry.receipt.meta.started_at < chain.entries[i - 1].receipt.meta.started_at {
            errors.push(ChainError::OutOfOrder { index: i });
        }

        // Hash integrity.
        if let Some(ref stored) = entry.receipt.receipt_sha256 {
            let broken = match receipt_hash(&entry.receipt) {
                Ok(recomputed) => *stored != recomputed,
                Err(_) => true,
            };
            if broken {
                errors.push(ChainError::BrokenHash {
                    index: i,
                    run_id: entry.receipt.meta.run_id,
                });
            }
        }

        // Parent reference.
        if let Some(pid) = entry.parent_id
            && !seen_ids.contains(&pid)
        {
            errors.push(ChainError::MissingParent {
                index: i,
                parent_id: pid,
            });
        }

        // Contract version consistency.
        if let Some(expected) = expected_version
            && entry.receipt.meta.contract_version != *expected
        {
            errors.push(ChainError::ContractVersionMismatch {
                index: i,
                expected: expected.clone(),
                actual: entry.receipt.meta.contract_version.clone(),
            });
        }
    }

    let valid = errors.is_empty();
    ChainVerification {
        valid,
        errors,
        chain_length,
        total_events,
        total_duration_ms,
    }
}
