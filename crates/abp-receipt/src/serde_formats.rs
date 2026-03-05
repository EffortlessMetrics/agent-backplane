// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt serialization in JSON and compact binary formats.

use abp_core::{ContractError, Receipt};

/// Serialize a receipt to pretty-printed JSON.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn to_json(receipt: &Receipt) -> Result<String, ContractError> {
    Ok(serde_json::to_string_pretty(receipt)?)
}

/// Deserialize a receipt from a JSON string.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if deserialization fails.
pub fn from_json(json: &str) -> Result<Receipt, ContractError> {
    Ok(serde_json::from_str(json)?)
}

/// Serialize a receipt to compact JSON bytes (no whitespace).
///
/// This is the "compact binary" format — canonical JSON encoded as UTF-8
/// bytes. It uses the same serde pipeline so no additional dependencies
/// are needed.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if serialization fails.
pub fn to_bytes(receipt: &Receipt) -> Result<Vec<u8>, ContractError> {
    Ok(serde_json::to_vec(receipt)?)
}

/// Deserialize a receipt from compact JSON bytes.
///
/// # Errors
///
/// Returns [`ContractError::Json`] if deserialization fails.
pub fn from_bytes(bytes: &[u8]) -> Result<Receipt, ContractError> {
    Ok(serde_json::from_slice(bytes)?)
}
