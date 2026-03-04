// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! This crate extracts receipt-focused logic from `abp-core` into a dedicated
//! microcrate. It provides canonical JSON serialization, SHA-256 hashing,
//! chain verification, a fluent receipt builder, field-level diffing,
//! structured validation, pluggable storage, and serialization helpers.

mod builder;
mod chain;
mod diff;
/// Serialization in JSON and compact binary formats.
pub mod serde_formats;
/// Pluggable receipt storage with an in-memory implementation.
pub mod store;
mod validate;
/// Receipt verification and batch auditing utilities.
pub mod verify;

pub use builder::ReceiptBuilder;
pub use chain::{ChainError, ReceiptChain};
pub use diff::{FieldDiff, ReceiptDiff, diff_receipts};
pub use validate::{ReceiptValidator, ValidationError};
pub use verify::{AuditIssue, AuditReport, ReceiptAuditor, VerificationResult, verify_receipt};

// Re-export core receipt types so consumers can depend on abp-receipt alone.
pub use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, ContractError, ExecutionMode,
    Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport,
};

use sha2::{Digest, Sha256};

/// Produce the canonical JSON representation of a receipt.
///
/// The `receipt_sha256` field is forced to `null` before serialization so
/// that the output is independent of any previously stored hash.
///
/// Keys are sorted (serde_json uses `BTreeMap` internally) and numbers
/// are serialized consistently, making the output deterministic.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if the receipt cannot be serialized.
///
/// # Examples
///
/// ```
/// use abp_receipt::{canonicalize, ReceiptBuilder, Outcome};
///
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let json1 = canonicalize(&r).unwrap();
/// let json2 = canonicalize(&r).unwrap();
/// assert_eq!(json1, json2);
/// ```
pub fn canonicalize(receipt: &Receipt) -> Result<String, ContractError> {
    let mut v = serde_json::to_value(receipt)?;
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    Ok(serde_json::to_string(&v)?)
}

/// Compute the hex-encoded SHA-256 hash of the canonical receipt form.
///
/// This calls [`canonicalize`] internally and hashes the resulting bytes.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if the receipt cannot be serialized.
///
/// # Examples
///
/// ```
/// use abp_receipt::{compute_hash, ReceiptBuilder, Outcome};
///
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let h = compute_hash(&r).unwrap();
/// assert_eq!(h.len(), 64); // SHA-256 hex
/// ```
pub fn compute_hash(receipt: &Receipt) -> Result<String, ContractError> {
    let json = canonicalize(receipt)?;
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify that a receipt's stored `receipt_sha256` matches the recomputed hash.
///
/// Returns `true` if:
/// - The stored hash matches the recomputed hash, **or**
/// - There is no stored hash (`receipt_sha256` is `None`).
///
/// Returns `false` if:
/// - The stored hash does not match the recomputed hash, **or**
/// - Serialization fails during hash computation.
///
/// # Examples
///
/// ```
/// use abp_receipt::{verify_hash, compute_hash, ReceiptBuilder, Outcome};
///
/// let mut r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// r.receipt_sha256 = Some(compute_hash(&r).unwrap());
/// assert!(verify_hash(&r));
///
/// r.receipt_sha256 = Some("tampered".into());
/// assert!(!verify_hash(&r));
/// ```
pub fn verify_hash(receipt: &Receipt) -> bool {
    match &receipt.receipt_sha256 {
        None => true,
        Some(stored) => match compute_hash(receipt) {
            Ok(recomputed) => *stored == recomputed,
            Err(_) => false,
        },
    }
}

#[cfg(test)]
mod tests;
