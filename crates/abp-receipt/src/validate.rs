// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt validation with structured error reporting.

use abp_core::{CONTRACT_VERSION, Receipt};
use std::fmt;

/// A single validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Which field or aspect failed validation.
    pub field: String,
    /// Human-readable description of the failure.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Validates a [`Receipt`] for structural correctness and integrity.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, ReceiptValidator, Outcome};
///
/// let v = ReceiptValidator::new();
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).with_hash().unwrap();
/// assert!(v.validate(&r).is_ok());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReceiptValidator {
    _priv: (),
}

impl ReceiptValidator {
    /// Create a new validator with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate a receipt, returning all detected errors.
    ///
    /// Returns `Ok(())` when the receipt passes all checks, or
    /// `Err(errors)` with every failure found.
    ///
    /// # Checks performed
    ///
    /// - `contract_version` matches [`CONTRACT_VERSION`]
    /// - `receipt_sha256`, if present, matches the recomputed hash
    /// - `backend.id` is non-empty
    /// - `finished_at >= started_at`
    /// - `duration_ms` is consistent with the timestamps
    pub fn validate(&self, receipt: &Receipt) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        // Contract version
        if receipt.meta.contract_version != CONTRACT_VERSION {
            errors.push(ValidationError {
                field: "meta.contract_version".into(),
                message: format!(
                    "expected \"{CONTRACT_VERSION}\", got \"{}\"",
                    receipt.meta.contract_version
                ),
            });
        }

        // Hash integrity
        if let Some(ref stored) = receipt.receipt_sha256 {
            match crate::compute_hash(receipt) {
                Ok(recomputed) if *stored != recomputed => {
                    errors.push(ValidationError {
                        field: "receipt_sha256".into(),
                        message: "stored hash does not match recomputed hash".into(),
                    });
                }
                Err(e) => {
                    errors.push(ValidationError {
                        field: "receipt_sha256".into(),
                        message: format!("failed to recompute hash: {e}"),
                    });
                }
                _ => {}
            }
        }

        // Backend ID required
        if receipt.backend.id.is_empty() {
            errors.push(ValidationError {
                field: "backend.id".into(),
                message: "backend id must not be empty".into(),
            });
        }

        // Timestamp ordering
        if receipt.meta.finished_at < receipt.meta.started_at {
            errors.push(ValidationError {
                field: "meta.finished_at".into(),
                message: "finished_at is before started_at".into(),
            });
        }

        // Duration consistency
        let expected_ms = (receipt.meta.finished_at - receipt.meta.started_at)
            .num_milliseconds()
            .max(0) as u64;
        if receipt.meta.duration_ms != expected_ms {
            errors.push(ValidationError {
                field: "meta.duration_ms".into(),
                message: format!(
                    "duration_ms is {}, expected {} based on timestamps",
                    receipt.meta.duration_ms, expected_ms
                ),
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
