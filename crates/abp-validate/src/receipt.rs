// SPDX-License-Identifier: MIT OR Apache-2.0
//! Receipt validation.

use abp_core::{receipt_hash, Outcome, Receipt};

use crate::{ValidationErrorKind, ValidationErrors, Validator};

/// Validates [`Receipt`] fields.
#[derive(Debug, Default)]
pub struct ReceiptValidator;

impl Validator<Receipt> for ReceiptValidator {
    fn validate(&self, receipt: &Receipt) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        // backend id must be non-empty
        if receipt.backend.id.trim().is_empty() {
            errs.add(
                "backend.id",
                ValidationErrorKind::Required,
                "backend id must not be empty",
            );
        }

        // contract_version must be non-empty
        if receipt.meta.contract_version.trim().is_empty() {
            errs.add(
                "meta.contract_version",
                ValidationErrorKind::Required,
                "contract_version must not be empty",
            );
        }

        // contract_version format check
        if !receipt.meta.contract_version.is_empty()
            && !receipt.meta.contract_version.starts_with("abp/v")
        {
            errs.add(
                "meta.contract_version",
                ValidationErrorKind::InvalidFormat,
                "contract_version must start with 'abp/v'",
            );
        }

        // finished_at must be >= started_at
        if receipt.meta.finished_at < receipt.meta.started_at {
            errs.add(
                "meta.finished_at",
                ValidationErrorKind::OutOfRange,
                "finished_at must not be before started_at",
            );
        }

        // receipt_sha256 correctness when present
        if let Some(ref hash) = receipt.receipt_sha256 {
            if hash.len() != 64 {
                errs.add(
                    "receipt_sha256",
                    ValidationErrorKind::InvalidFormat,
                    "receipt_sha256 must be a 64-character hex string",
                );
            } else if let Ok(expected) = receipt_hash(receipt) {
                if *hash != expected {
                    errs.add(
                        "receipt_sha256",
                        ValidationErrorKind::InvalidReference,
                        "receipt_sha256 does not match computed hash",
                    );
                }
            }
        }

        // cross-field: failed outcome should not claim harness_ok
        if receipt.outcome == Outcome::Failed && receipt.verification.harness_ok {
            errs.add(
                "verification.harness_ok",
                ValidationErrorKind::InvalidReference,
                "harness_ok must not be true when outcome is failed",
            );
        }

        errs.into_result()
    }
}
