// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared command implementations for the ABP CLI.
//!
//! These functions are library-level so they can be tested without
//! spawning the binary.

use abp_core::{Receipt, WorkOrder, receipt_hash};
use anyhow::{Context, Result};
use schemars::schema_for;
use std::path::Path;

/// Schema types that can be printed by the `schema` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    /// JSON schema for [`WorkOrder`].
    WorkOrder,
    /// JSON schema for [`Receipt`].
    Receipt,
    /// JSON schema for [`BackplaneConfig`](crate::config::BackplaneConfig).
    Config,
}

/// Return the JSON schema string for the given kind.
pub fn schema_json(kind: SchemaKind) -> Result<String> {
    let value = match kind {
        SchemaKind::WorkOrder => serde_json::to_value(schema_for!(WorkOrder))?,
        SchemaKind::Receipt => serde_json::to_value(schema_for!(Receipt))?,
        SchemaKind::Config => serde_json::to_value(schema_for!(crate::config::BackplaneConfig))?,
    };
    serde_json::to_string_pretty(&value).context("serialize schema")
}

/// Validate a JSON file against the [`WorkOrder`] schema.
///
/// Returns `Ok(())` if the file is valid, or an error describing every
/// validation failure found.
pub fn validate_work_order_file(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read work order file '{}'", path.display()))?;

    // First, ensure it's valid JSON.
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse JSON from '{}'", path.display()))?;

    // Try to deserialize into the actual type for a more specific error.
    serde_json::from_value::<WorkOrder>(value)
        .with_context(|| format!("validate work order from '{}'", path.display()))?;

    Ok(())
}

/// Inspect a receipt file: deserialize it and verify its hash.
///
/// Returns `(receipt, hash_valid)` where `hash_valid` is `true` when the
/// stored `receipt_sha256` matches the recomputed hash.
pub fn inspect_receipt_file(path: &Path) -> Result<(Receipt, bool)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read receipt file '{}'", path.display()))?;

    let receipt: Receipt = serde_json::from_str(&content)
        .with_context(|| format!("parse receipt from '{}'", path.display()))?;

    let valid = match &receipt.receipt_sha256 {
        Some(stored) => {
            let computed = receipt_hash(&receipt).context("compute receipt hash")?;
            *stored == computed
        }
        None => false,
    };

    Ok((receipt, valid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_work_order_is_valid_json() {
        let s = schema_json(SchemaKind::WorkOrder).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v.get("properties").is_some() || v.get("$defs").is_some());
    }

    #[test]
    fn schema_receipt_is_valid_json() {
        let s = schema_json(SchemaKind::Receipt).unwrap();
        let _: serde_json::Value = serde_json::from_str(&s).unwrap();
    }

    #[test]
    fn schema_config_is_valid_json() {
        let s = schema_json(SchemaKind::Config).unwrap();
        let _: serde_json::Value = serde_json::from_str(&s).unwrap();
    }

    #[test]
    fn validate_work_order_rejects_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(validate_work_order_file(&path).is_err());
    }

    #[test]
    fn validate_work_order_rejects_wrong_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrong.json");
        std::fs::write(&path, r#"{"foo": "bar"}"#).unwrap();
        assert!(validate_work_order_file(&path).is_err());
    }

    #[test]
    fn validate_work_order_accepts_valid() {
        let wo = abp_core::WorkOrderBuilder::new("test task").build();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.json");
        std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
        validate_work_order_file(&path).unwrap();
    }

    #[test]
    fn inspect_receipt_valid_hash() {
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

        let (r, valid) = inspect_receipt_file(&path).unwrap();
        assert!(valid, "hash should be valid");
        assert_eq!(r.receipt_sha256, receipt.receipt_sha256);
    }

    #[test]
    fn inspect_receipt_invalid_hash() {
        let mut receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .with_hash()
            .unwrap();

        // Tamper with the hash.
        receipt.receipt_sha256 = Some("0000000000000000".into());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

        let (_r, valid) = inspect_receipt_file(&path).unwrap();
        assert!(!valid, "tampered hash should be invalid");
    }

    #[test]
    fn inspect_receipt_no_hash() {
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .build();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();

        let (_r, valid) = inspect_receipt_file(&path).unwrap();
        assert!(!valid, "missing hash should be invalid");
    }
}
