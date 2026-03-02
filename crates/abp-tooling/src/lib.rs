// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Shared tool-definition models used across ABP dialect SDK crates.

use serde::{Deserialize, Serialize};

/// A vendor-agnostic tool definition used as the ABP canonical form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalToolDef {
    /// Tool name.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
}
