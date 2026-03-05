// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt export and import in JSON and JSONL formats.

use abp_core::Receipt;

use crate::Result;
use crate::error::StoreError;

/// Export receipts as a single JSON array.
pub fn export_json(receipts: &[Receipt]) -> Result<String> {
    serde_json::to_string_pretty(receipts).map_err(StoreError::from)
}

/// Export receipts in JSON-lines format (one JSON object per line).
pub fn export_jsonl(receipts: &[Receipt]) -> Result<String> {
    let mut buf = String::new();
    for r in receipts {
        let line = serde_json::to_string(r)?;
        buf.push_str(&line);
        buf.push('\n');
    }
    Ok(buf)
}

/// Import receipts from a JSON array string.
pub fn import_json(data: &str) -> Result<Vec<Receipt>> {
    serde_json::from_str(data).map_err(StoreError::from)
}

/// Import receipts from a JSONL string (one JSON object per line).
pub fn import_jsonl(data: &str) -> Result<Vec<Receipt>> {
    let mut receipts = Vec::new();
    for (i, line) in data.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let receipt: Receipt =
            serde_json::from_str(line).map_err(|e| StoreError::Other(format!("line {i}: {e}")))?;
        receipts.push(receipt);
    }
    Ok(receipts)
}
