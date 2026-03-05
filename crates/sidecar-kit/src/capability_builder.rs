// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fluent capability advertisement builder for sidecar authors.
//!
//! [`CapabilityBuilder`] provides a higher-level API than the raw
//! [`CapabilitySet`](crate::capabilities::CapabilitySet) — it bundles the
//! sidecar identity, dialect, and capability declarations into a single
//! fluent chain, producing the complete `hello` payload.
//!
//! # Example
//! ```
//! use sidecar_kit::capability_builder::CapabilityBuilder;
//!
//! let (backend, caps) = CapabilityBuilder::new("my-sidecar")
//!     .dialect("openai")
//!     .supports_streaming()
//!     .supports_tools()
//!     .build();
//!
//! assert_eq!(backend["id"], "my-sidecar");
//! assert_eq!(caps["streaming"], "native");
//! ```

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Fluent builder for constructing sidecar capability advertisements.
///
/// Produces a `(backend, capabilities)` pair suitable for use in a
/// [`Frame::Hello`](crate::Frame::Hello).
#[derive(Debug, Clone)]
pub struct CapabilityBuilder {
    name: String,
    dialect: Option<String>,
    version: Option<String>,
    capabilities: BTreeMap<String, Value>,
    metadata: BTreeMap<String, Value>,
}

impl CapabilityBuilder {
    /// Start building capabilities for a sidecar with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            dialect: None,
            version: None,
            capabilities: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    /// Set the dialect this sidecar speaks (e.g. `"openai"`, `"anthropic"`).
    #[must_use]
    pub fn dialect(mut self, dialect: impl Into<String>) -> Self {
        self.dialect = Some(dialect.into());
        self
    }

    /// Set the sidecar version string.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Declare native streaming support.
    #[must_use]
    pub fn supports_streaming(mut self) -> Self {
        self.capabilities
            .insert("streaming".into(), json!("native"));
        self
    }

    /// Declare native tool-use support.
    #[must_use]
    pub fn supports_tools(mut self) -> Self {
        self.capabilities.insert("tool_use".into(), json!("native"));
        self
    }

    /// Declare native file-read support.
    #[must_use]
    pub fn supports_file_read(mut self) -> Self {
        self.capabilities
            .insert("tool_read".into(), json!("native"));
        self
    }

    /// Declare native file-write support.
    #[must_use]
    pub fn supports_file_write(mut self) -> Self {
        self.capabilities
            .insert("tool_write".into(), json!("native"));
        self
    }

    /// Declare native file-edit support.
    #[must_use]
    pub fn supports_file_edit(mut self) -> Self {
        self.capabilities
            .insert("tool_edit".into(), json!("native"));
        self
    }

    /// Declare native bash/command execution support.
    #[must_use]
    pub fn supports_bash(mut self) -> Self {
        self.capabilities
            .insert("tool_bash".into(), json!("native"));
        self
    }

    /// Declare a capability as native.
    #[must_use]
    pub fn native(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.insert(capability.into(), json!("native"));
        self
    }

    /// Declare a capability as emulated.
    #[must_use]
    pub fn emulated(mut self, capability: impl Into<String>) -> Self {
        self.capabilities
            .insert(capability.into(), json!("emulated"));
        self
    }

    /// Declare a capability as unsupported.
    #[must_use]
    pub fn unsupported(mut self, capability: impl Into<String>) -> Self {
        self.capabilities
            .insert(capability.into(), json!("unsupported"));
        self
    }

    /// Declare a capability as restricted with a reason.
    #[must_use]
    pub fn restricted(mut self, capability: impl Into<String>, reason: impl Into<String>) -> Self {
        self.capabilities.insert(
            capability.into(),
            json!({"restricted": {"reason": reason.into()}}),
        );
        self
    }

    /// Add arbitrary metadata to the backend descriptor.
    #[must_use]
    pub fn meta(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Consume the builder and produce `(backend, capabilities)`.
    ///
    /// - `backend`: JSON object with `id`, optional `dialect`, `version`, and metadata.
    /// - `capabilities`: JSON object mapping capability names to support levels.
    #[must_use]
    pub fn build(self) -> (Value, Value) {
        let mut backend = serde_json::Map::new();
        backend.insert("id".into(), json!(self.name));
        if let Some(d) = &self.dialect {
            backend.insert("dialect".into(), json!(d));
        }
        if let Some(v) = &self.version {
            backend.insert("version".into(), json!(v));
        }
        for (k, v) in &self.metadata {
            backend.insert(k.clone(), v.clone());
        }

        let caps = serde_json::to_value(&self.capabilities).unwrap_or(json!({}));
        (Value::Object(backend), caps)
    }

    /// Build a complete [`Frame::Hello`](crate::Frame::Hello) frame.
    #[must_use]
    pub fn build_hello(self) -> crate::Frame {
        let (backend, caps) = self.build();
        crate::Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend,
            capabilities: caps,
            mode: Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frame;

    #[test]
    fn basic_build() {
        let (backend, caps) = CapabilityBuilder::new("my-sidecar").build();
        assert_eq!(backend["id"], "my-sidecar");
        assert!(caps.is_object());
    }

    #[test]
    fn with_dialect() {
        let (backend, _) = CapabilityBuilder::new("sc").dialect("openai").build();
        assert_eq!(backend["dialect"], "openai");
    }

    #[test]
    fn with_version() {
        let (backend, _) = CapabilityBuilder::new("sc").version("1.2.3").build();
        assert_eq!(backend["version"], "1.2.3");
    }

    #[test]
    fn supports_streaming() {
        let (_, caps) = CapabilityBuilder::new("sc").supports_streaming().build();
        assert_eq!(caps["streaming"], "native");
    }

    #[test]
    fn supports_tools() {
        let (_, caps) = CapabilityBuilder::new("sc").supports_tools().build();
        assert_eq!(caps["tool_use"], "native");
    }

    #[test]
    fn supports_file_ops() {
        let (_, caps) = CapabilityBuilder::new("sc")
            .supports_file_read()
            .supports_file_write()
            .supports_file_edit()
            .build();
        assert_eq!(caps["tool_read"], "native");
        assert_eq!(caps["tool_write"], "native");
        assert_eq!(caps["tool_edit"], "native");
    }

    #[test]
    fn supports_bash() {
        let (_, caps) = CapabilityBuilder::new("sc").supports_bash().build();
        assert_eq!(caps["tool_bash"], "native");
    }

    #[test]
    fn custom_native() {
        let (_, caps) = CapabilityBuilder::new("sc").native("custom_cap").build();
        assert_eq!(caps["custom_cap"], "native");
    }

    #[test]
    fn custom_emulated() {
        let (_, caps) = CapabilityBuilder::new("sc").emulated("vision").build();
        assert_eq!(caps["vision"], "emulated");
    }

    #[test]
    fn custom_unsupported() {
        let (_, caps) = CapabilityBuilder::new("sc")
            .unsupported("image_input")
            .build();
        assert_eq!(caps["image_input"], "unsupported");
    }

    #[test]
    fn restricted_with_reason() {
        let (_, caps) = CapabilityBuilder::new("sc")
            .restricted("tool_bash", "sandbox only")
            .build();
        assert_eq!(caps["tool_bash"]["restricted"]["reason"], "sandbox only");
    }

    #[test]
    fn metadata() {
        let (backend, _) = CapabilityBuilder::new("sc")
            .meta("runtime", json!("node"))
            .build();
        assert_eq!(backend["runtime"], "node");
    }

    #[test]
    fn full_chain() {
        let (backend, caps) = CapabilityBuilder::new("my-sidecar")
            .dialect("openai")
            .version("0.1.0")
            .supports_streaming()
            .supports_tools()
            .supports_file_read()
            .supports_bash()
            .emulated("vision")
            .restricted("web_search", "rate limited")
            .meta("runtime", json!("node"))
            .build();

        assert_eq!(backend["id"], "my-sidecar");
        assert_eq!(backend["dialect"], "openai");
        assert_eq!(backend["version"], "0.1.0");
        assert_eq!(backend["runtime"], "node");
        assert_eq!(caps["streaming"], "native");
        assert_eq!(caps["tool_use"], "native");
        assert_eq!(caps["tool_read"], "native");
        assert_eq!(caps["tool_bash"], "native");
        assert_eq!(caps["vision"], "emulated");
    }

    #[test]
    fn build_hello_frame() {
        let frame = CapabilityBuilder::new("my-sidecar")
            .supports_streaming()
            .build_hello();
        match frame {
            Frame::Hello {
                contract_version,
                backend,
                capabilities,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend["id"], "my-sidecar");
                assert_eq!(capabilities["streaming"], "native");
            }
            _ => panic!("expected Hello frame"),
        }
    }

    #[test]
    fn capabilities_are_deterministically_ordered() {
        let (_, caps) = CapabilityBuilder::new("sc")
            .native("z_cap")
            .native("a_cap")
            .native("m_cap")
            .build();
        let keys: Vec<&str> = caps
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(keys, vec!["a_cap", "m_cap", "z_cap"]);
    }

    #[test]
    fn empty_capabilities() {
        let (_, caps) = CapabilityBuilder::new("sc").build();
        assert_eq!(caps, json!({}));
    }
}
