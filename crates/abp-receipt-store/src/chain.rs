// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt chain validation.

use abp_core::Receipt;

/// Result of validating a receipt chain.
#[derive(Debug, Clone)]
pub struct ChainValidation {
    /// Whether the entire chain is valid.
    pub valid: bool,
    /// Number of receipts in the chain.
    pub receipt_count: usize,
    /// Errors found during validation.
    pub errors: Vec<ChainValidationError>,
}

/// A single error found during chain validation.
#[derive(Debug, Clone)]
pub struct ChainValidationError {
    /// Index of the receipt with the error.
    pub index: usize,
    /// Description of the error.
    pub message: String,
}

impl std::fmt::Display for ChainValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {}", self.index, self.message)
    }
}

/// Validate a chain of receipts for integrity and ordering.
///
/// Checks:
/// - No duplicate run IDs
/// - Hash integrity (if `receipt_sha256` is set)
/// - Chronological ordering (`started_at` non-decreasing)
///
/// Returns a [`ChainValidation`] describing the result.
pub fn validate_chain(receipts: &[Receipt]) -> ChainValidation {
    let mut errors = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for (i, receipt) in receipts.iter().enumerate() {
        let id = receipt.meta.run_id;

        // Duplicate ID check.
        if !seen_ids.insert(id) {
            errors.push(ChainValidationError {
                index: i,
                message: format!("duplicate run_id: {id}"),
            });
        }

        // Hash integrity check.
        if let Some(ref stored_hash) = receipt.receipt_sha256 {
            match abp_core::receipt_hash(receipt) {
                Ok(recomputed) => {
                    if *stored_hash != recomputed {
                        errors.push(ChainValidationError {
                            index: i,
                            message: format!(
                                "hash mismatch: stored={stored_hash}, computed={recomputed}"
                            ),
                        });
                    }
                }
                Err(e) => {
                    errors.push(ChainValidationError {
                        index: i,
                        message: format!("failed to compute hash: {e}"),
                    });
                }
            }
        }

        // Chronological ordering check.
        if i > 0 && receipt.meta.started_at < receipts[i - 1].meta.started_at {
            errors.push(ChainValidationError {
                index: i,
                message: "receipt started_at is earlier than previous receipt".to_string(),
            });
        }
    }

    ChainValidation {
        valid: errors.is_empty(),
        receipt_count: receipts.len(),
        errors,
    }
}
