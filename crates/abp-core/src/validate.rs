// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]

//! Receipt validation utilities.

use std::fmt;

use crate::{CONTRACT_VERSION, Receipt, receipt_hash};

/// An individual validation failure found in a [`Receipt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A required field is missing or empty.
    MissingField {
        /// Name of the missing field.
        field: &'static str,
    },
    /// The stored hash does not match the recomputed hash.
    InvalidHash {
        /// The recomputed hash value.
        expected: String,
        /// The stored hash value.
        actual: String,
    },
    /// The `backend.id` field is empty.
    EmptyBackendId,
    /// The outcome or another field has an invalid value.
    InvalidOutcome {
        /// Description of why the outcome is invalid.
        reason: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "missing required field: {field}"),
            Self::InvalidHash { expected, actual } => {
                write!(f, "hash mismatch: expected {expected}, got {actual}")
            }
            Self::EmptyBackendId => write!(f, "backend.id must not be empty"),
            Self::InvalidOutcome { reason } => write!(f, "invalid outcome: {reason}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validate a [`Receipt`] for completeness and consistency.
///
/// Returns `Ok(())` when the receipt passes all checks, or `Err(errors)` with
/// every problem found (errors are accumulated, not short-circuited).
///
/// # Examples
///
/// ```
/// use abp_core::{ReceiptBuilder, Outcome};
/// use abp_core::validate::validate_receipt;
///
/// // A valid receipt with a correct hash passes validation.
/// let receipt = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// assert!(validate_receipt(&receipt).is_ok());
/// ```
///
/// ```
/// use abp_core::{ReceiptBuilder, Outcome};
/// use abp_core::validate::{validate_receipt, ValidationError};
///
/// // An empty backend id is rejected.
/// let bad = ReceiptBuilder::new("")
///     .outcome(Outcome::Complete)
///     .build();
/// let errs = validate_receipt(&bad).unwrap_err();
/// assert!(errs.iter().any(|e| matches!(e, ValidationError::EmptyBackendId)));
/// ```
///
/// # Errors
///
/// Returns a `Vec<ValidationError>` listing every problem found in the receipt.
pub fn validate_receipt(receipt: &Receipt) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    // 1. backend.id must be non-empty
    if receipt.backend.id.is_empty() {
        errors.push(ValidationError::EmptyBackendId);
    }

    // 2. contract_version must match CONTRACT_VERSION
    if receipt.meta.contract_version != CONTRACT_VERSION {
        errors.push(ValidationError::InvalidOutcome {
            reason: format!(
                "contract_version mismatch: expected \"{CONTRACT_VERSION}\", got \"{}\"",
                receipt.meta.contract_version
            ),
        });
    }

    // 3. started_at <= finished_at
    if receipt.meta.started_at > receipt.meta.finished_at {
        errors.push(ValidationError::InvalidOutcome {
            reason: "started_at is after finished_at".into(),
        });
    }

    // 4. If receipt_sha256 is present, verify it matches recomputed hash
    if let Some(ref stored) = receipt.receipt_sha256 {
        match receipt_hash(receipt) {
            Ok(recomputed) => {
                if *stored != recomputed {
                    errors.push(ValidationError::InvalidHash {
                        expected: recomputed,
                        actual: stored.clone(),
                    });
                }
            }
            Err(e) => {
                errors.push(ValidationError::InvalidOutcome {
                    reason: format!("failed to recompute hash: {e}"),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_variants() {
        let e = ValidationError::EmptyBackendId;
        assert_eq!(e.to_string(), "backend.id must not be empty");

        let e = ValidationError::MissingField { field: "foo" };
        assert!(e.to_string().contains("foo"));
    }
}
