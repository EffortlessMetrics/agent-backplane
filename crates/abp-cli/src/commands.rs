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

/// The detected type of a validated JSON file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatedType {
    /// The file is a valid [`WorkOrder`].
    WorkOrder,
    /// The file is a valid [`Receipt`].
    Receipt,
}

/// Validate a JSON file, auto-detecting whether it is a [`WorkOrder`] or [`Receipt`].
///
/// Returns the detected type on success.
pub fn validate_file(path: &Path) -> Result<ValidatedType> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read file '{}'", path.display()))?;

    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse JSON from '{}'", path.display()))?;

    // Try WorkOrder first, then Receipt.
    if serde_json::from_value::<WorkOrder>(value.clone()).is_ok() {
        return Ok(ValidatedType::WorkOrder);
    }
    if serde_json::from_value::<Receipt>(value.clone()).is_ok() {
        return Ok(ValidatedType::Receipt);
    }

    anyhow::bail!(
        "file '{}' is not a valid WorkOrder or Receipt",
        path.display()
    )
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

/// Verify a receipt file's hash integrity.
///
/// Returns `(receipt, hash_valid)`.
pub fn verify_receipt_file(path: &Path) -> Result<(Receipt, bool)> {
    inspect_receipt_file(path)
}

/// Load and validate a configuration file.
///
/// Returns a list of human-readable diagnostic messages (errors and warnings).
pub fn config_check(path: Option<&Path>) -> Result<Vec<String>> {
    let mut diagnostics = Vec::new();

    let config = match crate::config::load_config(path) {
        Ok(c) => c,
        Err(e) => {
            diagnostics.push(format!("error: {e}"));
            return Ok(diagnostics);
        }
    };

    match crate::config::validate_config(&config) {
        Ok(()) => {
            diagnostics.push("config: ok".into());
        }
        Err(errors) => {
            for e in &errors {
                diagnostics.push(format!("error: {e}"));
            }
        }
    }

    Ok(diagnostics)
}

/// Diff two receipt files, returning a human-readable summary of differences.
pub fn receipt_diff(path1: &Path, path2: &Path) -> Result<String> {
    let content1 = std::fs::read_to_string(path1)
        .with_context(|| format!("read receipt file '{}'", path1.display()))?;
    let content2 = std::fs::read_to_string(path2)
        .with_context(|| format!("read receipt file '{}'", path2.display()))?;

    let r1: Receipt = serde_json::from_str(&content1)
        .with_context(|| format!("parse receipt from '{}'", path1.display()))?;
    let r2: Receipt = serde_json::from_str(&content2)
        .with_context(|| format!("parse receipt from '{}'", path2.display()))?;

    let mut diffs = Vec::new();

    if r1.outcome != r2.outcome {
        diffs.push(format!("outcome: {:?} -> {:?}", r1.outcome, r2.outcome));
    }
    if r1.backend.id != r2.backend.id {
        diffs.push(format!("backend: {} -> {}", r1.backend.id, r2.backend.id));
    }
    if r1.meta.run_id != r2.meta.run_id {
        diffs.push(format!("run_id: {} -> {}", r1.meta.run_id, r2.meta.run_id));
    }
    if r1.meta.contract_version != r2.meta.contract_version {
        diffs.push(format!(
            "contract_version: {} -> {}",
            r1.meta.contract_version, r2.meta.contract_version
        ));
    }
    if r1.meta.duration_ms != r2.meta.duration_ms {
        diffs.push(format!(
            "duration_ms: {} -> {}",
            r1.meta.duration_ms, r2.meta.duration_ms
        ));
    }
    if r1.mode != r2.mode {
        diffs.push(format!("mode: {:?} -> {:?}", r1.mode, r2.mode));
    }
    if r1.trace.len() != r2.trace.len() {
        diffs.push(format!(
            "trace_events: {} -> {}",
            r1.trace.len(),
            r2.trace.len()
        ));
    }
    if r1.artifacts.len() != r2.artifacts.len() {
        diffs.push(format!(
            "artifacts: {} -> {}",
            r1.artifacts.len(),
            r2.artifacts.len()
        ));
    }
    if r1.receipt_sha256 != r2.receipt_sha256 {
        diffs.push(format!(
            "receipt_sha256: {} -> {}",
            r1.receipt_sha256.as_deref().unwrap_or("<none>"),
            r2.receipt_sha256.as_deref().unwrap_or("<none>"),
        ));
    }

    if diffs.is_empty() {
        Ok("no differences".to_string())
    } else {
        Ok(diffs.join("\n"))
    }
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

    #[test]
    fn validate_file_detects_work_order() {
        let wo = abp_core::WorkOrderBuilder::new("test").build();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wo.json");
        std::fs::write(&path, serde_json::to_string_pretty(&wo).unwrap()).unwrap();
        assert_eq!(validate_file(&path).unwrap(), ValidatedType::WorkOrder);
    }

    #[test]
    fn validate_file_detects_receipt() {
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .build();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
        assert_eq!(validate_file(&path).unwrap(), ValidatedType::Receipt);
    }

    #[test]
    fn validate_file_rejects_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unknown.json");
        std::fs::write(&path, r#"{"foo": "bar"}"#).unwrap();
        assert!(validate_file(&path).is_err());
    }

    #[test]
    fn config_check_defaults_ok() {
        let diags = config_check(None).unwrap();
        assert!(diags.iter().any(|d| d.contains("ok")));
    }

    #[test]
    fn config_check_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not valid [toml =").unwrap();
        let diags = config_check(Some(&path)).unwrap();
        assert!(diags.iter().any(|d| d.starts_with("error:")));
    }

    #[test]
    fn receipt_diff_no_differences() {
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .with_hash()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("r1.json");
        let p2 = dir.path().join("r2.json");
        let json = serde_json::to_string_pretty(&receipt).unwrap();
        std::fs::write(&p1, &json).unwrap();
        std::fs::write(&p2, &json).unwrap();
        let diff = receipt_diff(&p1, &p2).unwrap();
        assert_eq!(diff, "no differences");
    }

    #[test]
    fn receipt_diff_shows_changes() {
        let r1 = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .with_hash()
            .unwrap();
        let r2 = abp_core::ReceiptBuilder::new("other")
            .outcome(abp_core::Outcome::Failed)
            .with_hash()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("r1.json");
        let p2 = dir.path().join("r2.json");
        std::fs::write(&p1, serde_json::to_string_pretty(&r1).unwrap()).unwrap();
        std::fs::write(&p2, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
        let diff = receipt_diff(&p1, &p2).unwrap();
        assert!(diff.contains("outcome"));
        assert!(diff.contains("backend"));
    }

    #[test]
    fn verify_receipt_delegates_to_inspect() {
        let receipt = abp_core::ReceiptBuilder::new("mock")
            .outcome(abp_core::Outcome::Complete)
            .with_hash()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
        let (_, valid) = verify_receipt_file(&path).unwrap();
        assert!(valid);
    }
}
