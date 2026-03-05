// SPDX-License-Identifier: MIT OR Apache-2.0
//! Capability declaration helpers for sidecar authors.
//!
//! Sidecars advertise their capabilities in the `hello` handshake.
//! This module provides a fluent [`CapabilitySet`] builder that produces
//! the JSON value expected by the ABP protocol without requiring `abp-core`.

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Fluent builder for declaring sidecar capabilities.
///
/// Produces a JSON object mapping capability names to support levels,
/// matching the shape of `abp-core::CapabilityManifest`.
///
/// # Example
/// ```
/// use sidecar_kit::capabilities::CapabilitySet;
///
/// let caps = CapabilitySet::new()
///     .native("streaming")
///     .native("tool_use")
///     .emulated("structured_output_json_schema")
///     .unsupported("image_input")
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    entries: BTreeMap<String, Value>,
}

impl CapabilitySet {
    /// Create an empty capability set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a capability as natively supported.
    #[must_use]
    pub fn native(mut self, name: &str) -> Self {
        self.entries.insert(name.to_string(), json!("native"));
        self
    }

    /// Declare a capability as supported via emulation/polyfill.
    #[must_use]
    pub fn emulated(mut self, name: &str) -> Self {
        self.entries.insert(name.to_string(), json!("emulated"));
        self
    }

    /// Declare a capability as unsupported.
    #[must_use]
    pub fn unsupported(mut self, name: &str) -> Self {
        self.entries.insert(name.to_string(), json!("unsupported"));
        self
    }

    /// Declare a capability as restricted with a reason.
    #[must_use]
    pub fn restricted(mut self, name: &str, reason: &str) -> Self {
        self.entries.insert(
            name.to_string(),
            json!({ "restricted": { "reason": reason } }),
        );
        self
    }

    /// Add a capability with an arbitrary support level value.
    #[must_use]
    pub fn with(mut self, name: &str, level: Value) -> Self {
        self.entries.insert(name.to_string(), level);
        self
    }

    /// Returns `true` if no capabilities have been declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of declared capabilities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check whether a capability has been declared.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Consume the builder and produce a JSON value.
    #[must_use]
    pub fn build(self) -> Value {
        serde_json::to_value(self.entries).unwrap_or(json!({}))
    }
}

/// Shorthand: create a minimal capability set for a streaming tool-use sidecar.
#[must_use]
pub fn default_streaming_capabilities() -> CapabilitySet {
    CapabilitySet::new().native("streaming").native("tool_use")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_builds_empty_object() {
        let v = CapabilitySet::new().build();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn native_capability() {
        let v = CapabilitySet::new().native("streaming").build();
        assert_eq!(v["streaming"], json!("native"));
    }

    #[test]
    fn emulated_capability() {
        let v = CapabilitySet::new().emulated("vision").build();
        assert_eq!(v["vision"], json!("emulated"));
    }

    #[test]
    fn restricted_capability() {
        let v = CapabilitySet::new()
            .restricted("tool_bash", "sandbox only")
            .build();
        assert_eq!(
            v["tool_bash"],
            json!({ "restricted": { "reason": "sandbox only" } })
        );
    }

    #[test]
    fn default_streaming_has_expected_keys() {
        let caps = default_streaming_capabilities();
        assert!(caps.contains("streaming"));
        assert!(caps.contains("tool_use"));
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn btree_ordering_is_deterministic() {
        let v = CapabilitySet::new()
            .native("z_cap")
            .native("a_cap")
            .native("m_cap")
            .build();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, vec!["a_cap", "m_cap", "z_cap"]);
    }
}
