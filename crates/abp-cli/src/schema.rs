// SPDX-License-Identifier: MIT OR Apache-2.0
//! Schema subcommand implementation.
//!
//! Generates JSON schemas for ABP contract types and optionally writes them
//! to a file.

#![allow(dead_code, unused_imports)]

use crate::commands::SchemaKind;
use anyhow::{Context, Result};
use std::path::Path;

/// Generate the JSON schema string for the given [`SchemaKind`].
///
/// This delegates to [`crate::commands::schema_json`].
pub fn generate_schema(kind: SchemaKind) -> Result<String> {
    crate::commands::schema_json(kind)
}

/// Write a JSON schema to a file at `path`, creating parent directories as needed.
pub fn write_schema_to_file(kind: SchemaKind, path: &Path) -> Result<()> {
    let json = generate_schema(kind)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create schema output directory {}", parent.display()))?;
    }
    std::fs::write(path, &json)
        .with_context(|| format!("write schema to {}", path.display()))?;
    Ok(())
}

/// Print a JSON schema to stdout, or write it to a file if `output` is given.
pub fn output_schema(kind: SchemaKind, output: Option<&Path>) -> Result<()> {
    match output {
        Some(path) => {
            write_schema_to_file(kind, path)?;
            eprintln!("schema written to {}", path.display());
        }
        None => {
            let json = generate_schema(kind)?;
            println!("{json}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_work_order_schema() {
        let json = generate_schema(SchemaKind::WorkOrder).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn generate_receipt_schema() {
        let json = generate_schema(SchemaKind::Receipt).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn generate_config_schema() {
        let json = generate_schema(SchemaKind::Config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn write_schema_to_file_creates_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schemas").join("work_order.json");
        write_schema_to_file(SchemaKind::WorkOrder, &path).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn all_schema_kinds_produce_valid_json() {
        for kind in &[SchemaKind::WorkOrder, SchemaKind::Receipt, SchemaKind::Config] {
            let json = generate_schema(*kind).unwrap();
            let v: serde_json::Value = serde_json::from_str(&json).unwrap();
            // Every JSON schema should have at least a "type" or "$defs" or "properties" key.
            assert!(
                v.get("type").is_some()
                    || v.get("$defs").is_some()
                    || v.get("properties").is_some(),
                "schema for {kind:?} missing expected top-level keys"
            );
        }
    }
}
