// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical receipt representation for deterministic hashing and comparison.

use abp_core::{ContractError, Receipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A stripped-down receipt that contains only the fields relevant for
/// deterministic hashing. Optional metadata is omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalReceipt {
    /// Run identifier.
    pub run_id: String,
    /// Work-order identifier.
    pub work_order_id: String,
    /// Contract version string.
    pub contract_version: String,
    /// ISO-8601 start timestamp.
    pub started_at: String,
    /// ISO-8601 finish timestamp.
    pub finished_at: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Backend identifier.
    pub backend_id: String,
    /// Execution mode (serialized).
    pub mode: String,
    /// Outcome (serialized).
    pub outcome: String,
    /// Number of trace events.
    pub trace_count: usize,
    /// Number of artifacts.
    pub artifact_count: usize,
    /// Input tokens (if reported).
    pub input_tokens: Option<u64>,
    /// Output tokens (if reported).
    pub output_tokens: Option<u64>,
}

impl CanonicalReceipt {
    /// Build a canonical receipt from a full [`Receipt`], stripping optional fields.
    #[must_use]
    pub fn from_receipt(receipt: &Receipt) -> Self {
        Self {
            run_id: receipt.meta.run_id.to_string(),
            work_order_id: receipt.meta.work_order_id.to_string(),
            contract_version: receipt.meta.contract_version.clone(),
            started_at: receipt.meta.started_at.to_rfc3339(),
            finished_at: receipt.meta.finished_at.to_rfc3339(),
            duration_ms: receipt.meta.duration_ms,
            backend_id: receipt.backend.id.clone(),
            mode: serde_json::to_string(&receipt.mode).unwrap_or_default(),
            outcome: format!("{:?}", receipt.outcome),
            trace_count: receipt.trace.len(),
            artifact_count: receipt.artifacts.len(),
            input_tokens: receipt.usage.input_tokens,
            output_tokens: receipt.usage.output_tokens,
        }
    }
}

/// Produce deterministic JSON bytes from a canonical receipt.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn canonicalize(receipt: &Receipt) -> Result<Vec<u8>, ContractError> {
    let canonical = CanonicalReceipt::from_receipt(receipt);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(bytes)
}

/// Compute the hex-encoded SHA-256 hash of the canonical form.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn canonical_hash(receipt: &Receipt) -> Result<String, ContractError> {
    let bytes = canonicalize(receipt)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify that a receipt's canonical hash matches an expected value.
///
/// Returns `true` if the recomputed canonical hash equals `expected_hash`.
#[must_use]
pub fn verify(receipt: &Receipt, expected_hash: &str) -> bool {
    match canonical_hash(receipt) {
        Ok(h) => h == expected_hash,
        Err(_) => false,
    }
}

/// A single field difference between two canonical receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalDiff {
    /// Field name.
    pub field: String,
    /// Value from receipt A.
    pub left: String,
    /// Value from receipt B.
    pub right: String,
}

/// Compare the canonical forms of two receipts and return differing fields.
#[must_use]
pub fn diff_canonical(a: &Receipt, b: &Receipt) -> Vec<CanonicalDiff> {
    let ca = CanonicalReceipt::from_receipt(a);
    let cb = CanonicalReceipt::from_receipt(b);
    let mut diffs = Vec::new();

    macro_rules! cmp_field {
        ($field:ident) => {
            let left = format!("{:?}", ca.$field);
            let right = format!("{:?}", cb.$field);
            if left != right {
                diffs.push(CanonicalDiff {
                    field: stringify!($field).to_string(),
                    left,
                    right,
                });
            }
        };
    }

    cmp_field!(run_id);
    cmp_field!(work_order_id);
    cmp_field!(contract_version);
    cmp_field!(started_at);
    cmp_field!(finished_at);
    cmp_field!(duration_ms);
    cmp_field!(backend_id);
    cmp_field!(mode);
    cmp_field!(outcome);
    cmp_field!(trace_count);
    cmp_field!(artifact_count);
    cmp_field!(input_tokens);
    cmp_field!(output_tokens);

    diffs
}
