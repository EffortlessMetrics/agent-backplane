// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Capability negotiation between work-order requirements and backend manifests.
//!
//! This crate provides functions to compare a [`CapabilityManifest`] against
//! [`CapabilityRequirements`] and produce structured negotiation results,
//! per-capability support checks, and human-readable compatibility reports.
//!
//! It also provides a [`CapabilityRegistry`] that stores manifests for known
//! dialects/backends and pre-populated manifests for common models.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirements, SupportLevel as CoreSupportLevel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a single capability would be fulfilled after negotiation.
///
/// # Examples
///
/// ```
/// use abp_capability::SupportLevel;
///
/// let level = SupportLevel::Native;
/// assert!(matches!(level, SupportLevel::Native));
///
/// let emulated = SupportLevel::Emulated { method: "polyfill".into() };
/// assert!(matches!(emulated, SupportLevel::Emulated { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum SupportLevel {
    /// The backend supports this capability natively.
    Native,
    /// The capability can be provided through an adapter/polyfill.
    Emulated {
        /// Human-readable description of the emulation method.
        method: String,
    },
    /// The capability is available but restricted by policy or environment.
    Restricted {
        /// Human-readable explanation of the restriction.
        reason: String,
    },
    /// The capability is not available.
    Unsupported {
        /// Human-readable explanation of why it is unsupported.
        reason: String,
    },
}

/// Outcome of negotiating a full set of requirements against a manifest.
///
/// # Examples
///
/// ```
/// use abp_capability::NegotiationResult;
/// use abp_core::Capability;
///
/// let result = NegotiationResult {
///     native: vec![Capability::Streaming],
///     emulated: vec![Capability::ToolRead],
///     unsupported: vec![],
/// };
/// assert!(result.is_compatible());
/// assert_eq!(result.total(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationResult {
    /// Capabilities the manifest supports natively.
    pub native: Vec<Capability>,
    /// Capabilities the manifest can provide via emulation or with restrictions.
    pub emulated: Vec<Capability>,
    /// Capabilities the manifest cannot provide.
    pub unsupported: Vec<Capability>,
}

impl NegotiationResult {
    /// Returns `true` when every required capability is native or emulated.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.unsupported.is_empty()
    }

    /// Total number of capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.native.len() + self.emulated.len() + self.unsupported.len()
    }
}

/// Human-readable summary of a negotiation outcome.
///
/// # Examples
///
/// ```
/// use abp_capability::{generate_report, NegotiationResult};
/// use abp_core::Capability;
///
/// let result = NegotiationResult {
///     native: vec![Capability::Streaming],
///     emulated: vec![Capability::ToolRead],
///     unsupported: vec![],
/// };
/// let report = generate_report(&result);
/// assert!(report.compatible);
/// assert!(report.summary.contains("fully compatible"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    /// Whether all required capabilities can be satisfied.
    pub compatible: bool,
    /// Number of natively supported capabilities.
    pub native_count: usize,
    /// Number of capabilities requiring emulation.
    pub emulated_count: usize,
    /// Number of unsupported capabilities.
    pub unsupported_count: usize,
    /// One-line human-readable summary.
    pub summary: String,
    /// Per-capability details (capability name → support level).
    pub details: Vec<(String, SupportLevel)>,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Stores [`CapabilityManifest`]s for known dialects/backends.
///
/// Use [`CapabilityRegistry::with_defaults`] to get a registry pre-populated
/// with manifests for common models (OpenAI GPT-4o, Claude 3.5 Sonnet,
/// Gemini 1.5 Pro).
///
/// # Examples
///
/// ```
/// use abp_capability::CapabilityRegistry;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut reg = CapabilityRegistry::new();
/// let mut m = BTreeMap::new();
/// m.insert(Capability::Streaming, CoreSupportLevel::Native);
/// reg.register("my-backend", m);
/// assert!(reg.get("my-backend").is_some());
/// ```
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    manifests: BTreeMap<String, CapabilityManifest>,
}

impl CapabilityRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry pre-populated with manifests for common models.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register("openai/gpt-4o", openai_gpt4o_manifest());
        reg.register("anthropic/claude-3.5-sonnet", claude_35_sonnet_manifest());
        reg.register("google/gemini-1.5-pro", gemini_15_pro_manifest());
        reg
    }

    /// Register a manifest under the given name (dialect/model identifier).
    pub fn register(&mut self, name: &str, manifest: CapabilityManifest) {
        self.manifests.insert(name.to_owned(), manifest);
    }

    /// Look up a manifest by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CapabilityManifest> {
        self.manifests.get(name)
    }

    /// Return all registered names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.manifests.keys().map(String::as_str).collect()
    }

    /// Negotiate the given requirements against a named manifest.
    ///
    /// Returns `None` if the name is not registered.
    #[must_use]
    pub fn negotiate_by_name(
        &self,
        name: &str,
        required: &[Capability],
    ) -> Option<NegotiationResult> {
        self.manifests
            .get(name)
            .map(|m| negotiate_capabilities(required, m))
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Classify a single capability against a manifest.
///
/// Returns [`SupportLevel::Native`] for `CoreSupportLevel::Native`,
/// [`SupportLevel::Emulated`] for `CoreSupportLevel::Emulated`,
/// [`SupportLevel::Restricted`] for `CoreSupportLevel::Restricted`, and
/// [`SupportLevel::Unsupported`] for everything else (including capabilities
/// absent from the manifest).
///
/// # Examples
///
/// ```
/// use abp_capability::{check_capability, SupportLevel};
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// assert_eq!(check_capability(&manifest, &Capability::Streaming), SupportLevel::Native);
/// ```
#[must_use]
pub fn check_capability(manifest: &CapabilityManifest, cap: &Capability) -> SupportLevel {
    match manifest.get(cap) {
        Some(CoreSupportLevel::Native) => SupportLevel::Native,
        Some(CoreSupportLevel::Emulated) => SupportLevel::Emulated {
            method: "adapter".into(),
        },
        Some(CoreSupportLevel::Restricted { reason }) => SupportLevel::Restricted {
            reason: reason.clone(),
        },
        Some(CoreSupportLevel::Unsupported) => SupportLevel::Unsupported {
            reason: "explicitly marked unsupported".into(),
        },
        None => SupportLevel::Unsupported {
            reason: "not declared in manifest".into(),
        },
    }
}

/// Negotiate a set of required capabilities against a manifest (simple form).
///
/// This is the primary negotiation entry point. Each required capability is
/// classified via [`check_capability`] and placed into the appropriate bucket.
/// Capabilities with [`SupportLevel::Restricted`] are placed in the `emulated`
/// bucket since they can still be served.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate_capabilities;
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let result = negotiate_capabilities(
///     &[Capability::Streaming, Capability::ToolUse],
///     &manifest,
/// );
/// assert_eq!(result.native, vec![Capability::Streaming]);
/// assert_eq!(result.unsupported, vec![Capability::ToolUse]);
/// assert!(!result.is_compatible());
/// ```
#[must_use]
pub fn negotiate_capabilities(
    required: &[Capability],
    manifest: &CapabilityManifest,
) -> NegotiationResult {
    let mut native = Vec::new();
    let mut emulated = Vec::new();
    let mut unsupported = Vec::new();

    for cap in required {
        match check_capability(manifest, cap) {
            SupportLevel::Native => native.push(cap.clone()),
            SupportLevel::Emulated { .. } | SupportLevel::Restricted { .. } => {
                emulated.push(cap.clone());
            }
            SupportLevel::Unsupported { .. } => unsupported.push(cap.clone()),
        }
    }

    NegotiationResult {
        native,
        emulated,
        unsupported,
    }
}

/// Negotiate all required capabilities from [`CapabilityRequirements`] against
/// a manifest.
///
/// This preserves backward compatibility with the structured requirements type
/// from `abp-core`.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate;
/// use abp_core::{
///     Capability, CapabilityManifest, CapabilityRequirements,
///     CapabilityRequirement, MinSupport, SupportLevel as CoreSupportLevel,
/// };
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let reqs = CapabilityRequirements {
///     required: vec![CapabilityRequirement {
///         capability: Capability::Streaming,
///         min_support: MinSupport::Native,
///     }],
/// };
///
/// let result = negotiate(&manifest, &reqs);
/// assert!(result.is_compatible());
/// assert_eq!(result.native, vec![Capability::Streaming]);
/// ```
#[must_use]
pub fn negotiate(
    manifest: &CapabilityManifest,
    requirements: &CapabilityRequirements,
) -> NegotiationResult {
    let caps: Vec<Capability> = requirements.required.iter().map(|r| r.capability.clone()).collect();
    negotiate_capabilities(&caps, manifest)
}

/// Produce a human-readable [`CompatibilityReport`] from a negotiation result.
///
/// # Examples
///
/// ```
/// use abp_capability::{generate_report, NegotiationResult};
/// use abp_core::Capability;
///
/// let result = NegotiationResult {
///     native: vec![Capability::Streaming],
///     emulated: vec![],
///     unsupported: vec![],
/// };
/// let report = generate_report(&result);
/// assert!(report.compatible);
/// assert_eq!(report.native_count, 1);
/// ```
#[must_use]
pub fn generate_report(result: &NegotiationResult) -> CompatibilityReport {
    let compatible = result.is_compatible();

    let mut details: Vec<(String, SupportLevel)> = Vec::new();
    for cap in &result.native {
        details.push((format!("{cap:?}"), SupportLevel::Native));
    }
    for cap in &result.emulated {
        details.push((
            format!("{cap:?}"),
            SupportLevel::Emulated {
                method: "adapter".into(),
            },
        ));
    }
    for cap in &result.unsupported {
        details.push((
            format!("{cap:?}"),
            SupportLevel::Unsupported {
                reason: "not available".into(),
            },
        ));
    }

    let verdict = if compatible {
        "fully compatible"
    } else {
        "incompatible"
    };

    let summary = format!(
        "{} native, {} emulated, {} unsupported — {verdict}",
        result.native.len(),
        result.emulated.len(),
        result.unsupported.len(),
    );

    CompatibilityReport {
        compatible,
        native_count: result.native.len(),
        emulated_count: result.emulated.len(),
        unsupported_count: result.unsupported.len(),
        summary,
        details,
    }
}

// ---------------------------------------------------------------------------
// Pre-populated manifests
// ---------------------------------------------------------------------------

/// Capability manifest for **OpenAI GPT-4o**.
#[must_use]
pub fn openai_gpt4o_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::Audio, CoreSupportLevel::Native),
        (Capability::CodeExecution, CoreSupportLevel::Emulated),
        (Capability::StructuredOutputJsonSchema, CoreSupportLevel::Native),
        (Capability::JsonMode, CoreSupportLevel::Native),
        (Capability::SystemMessage, CoreSupportLevel::Native),
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::StopSequences, CoreSupportLevel::Native),
        (Capability::Logprobs, CoreSupportLevel::Native),
        (Capability::SeedDeterminism, CoreSupportLevel::Native),
        (Capability::FrequencyPenalty, CoreSupportLevel::Native),
        (Capability::PresencePenalty, CoreSupportLevel::Native),
        (Capability::ImageGeneration, CoreSupportLevel::Emulated),
        (Capability::Embeddings, CoreSupportLevel::Emulated),
        (Capability::BatchMode, CoreSupportLevel::Native),
        (Capability::TopK, CoreSupportLevel::Unsupported),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
        (Capability::CacheControl, CoreSupportLevel::Unsupported),
        (Capability::PdfInput, CoreSupportLevel::Unsupported),
    ])
}

/// Capability manifest for **Anthropic Claude 3.5 Sonnet**.
#[must_use]
pub fn claude_35_sonnet_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::PdfInput, CoreSupportLevel::Native),
        (Capability::CodeExecution, CoreSupportLevel::Emulated),
        (Capability::StructuredOutputJsonSchema, CoreSupportLevel::Emulated),
        (Capability::JsonMode, CoreSupportLevel::Emulated),
        (Capability::SystemMessage, CoreSupportLevel::Native),
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::TopK, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::StopSequences, CoreSupportLevel::Native),
        (Capability::ExtendedThinking, CoreSupportLevel::Native),
        (Capability::CacheControl, CoreSupportLevel::Native),
        (Capability::BatchMode, CoreSupportLevel::Native),
        (Capability::Audio, CoreSupportLevel::Unsupported),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (Capability::SeedDeterminism, CoreSupportLevel::Unsupported),
        (Capability::FrequencyPenalty, CoreSupportLevel::Unsupported),
        (Capability::PresencePenalty, CoreSupportLevel::Unsupported),
        (Capability::ImageGeneration, CoreSupportLevel::Unsupported),
        (Capability::Embeddings, CoreSupportLevel::Emulated),
    ])
}

/// Capability manifest for **Google Gemini 1.5 Pro**.
#[must_use]
pub fn gemini_15_pro_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::Audio, CoreSupportLevel::Native),
        (Capability::PdfInput, CoreSupportLevel::Native),
        (Capability::CodeExecution, CoreSupportLevel::Native),
        (Capability::StructuredOutputJsonSchema, CoreSupportLevel::Native),
        (Capability::JsonMode, CoreSupportLevel::Native),
        (Capability::SystemMessage, CoreSupportLevel::Native),
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::TopK, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::StopSequences, CoreSupportLevel::Native),
        (Capability::Embeddings, CoreSupportLevel::Emulated),
        (Capability::BatchMode, CoreSupportLevel::Unsupported),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (Capability::SeedDeterminism, CoreSupportLevel::Unsupported),
        (Capability::FrequencyPenalty, CoreSupportLevel::Unsupported),
        (Capability::PresencePenalty, CoreSupportLevel::Unsupported),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
        (Capability::CacheControl, CoreSupportLevel::Native),
        (Capability::ImageGeneration, CoreSupportLevel::Emulated),
    ])
}

// ===========================================================================
// Tests
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        Capability, CapabilityRequirement, CapabilityRequirements, MinSupport,
        SupportLevel as CoreSupportLevel,
    };
    use std::collections::BTreeMap;

    // ---- helpers ----------------------------------------------------------

    fn manifest_from(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    fn require(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
        CapabilityRequirements {
            required: caps
                .iter()
                .map(|(c, m)| CapabilityRequirement {
                    capability: c.clone(),
                    min_support: m.clone(),
                })
                .collect(),
        }
    }

    fn require_native(caps: &[Capability]) -> CapabilityRequirements {
        require(
            &caps
                .iter()
                .map(|c| (c.clone(), MinSupport::Native))
                .collect::<Vec<_>>(),
        )
    }

    // ---- negotiate (legacy CapabilityRequirements) -----------------------

    #[test]
    fn negotiate_all_native() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native.len(), 2);
        assert!(res.emulated.is_empty());
        assert!(res.unsupported.is_empty());
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_all_emulated() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.native.is_empty());
        assert_eq!(res.emulated.len(), 2);
        assert!(res.unsupported.is_empty());
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_all_unsupported() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.native.is_empty());
        assert!(res.emulated.is_empty());
        assert_eq!(res.unsupported.len(), 2);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_mixed() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let r = require_native(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert_eq!(res.emulated, vec![Capability::ToolRead]);
        assert_eq!(res.unsupported, vec![Capability::ToolWrite]);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_some_emulated_some_unsupported() {
        let m = manifest_from(&[
            (Capability::ToolBash, CoreSupportLevel::Emulated),
            (Capability::ToolEdit, CoreSupportLevel::Unsupported),
        ]);
        let r = require_native(&[Capability::ToolBash, Capability::ToolEdit]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulated, vec![Capability::ToolBash]);
        assert_eq!(res.unsupported, vec![Capability::ToolEdit]);
        assert!(!res.is_compatible());
    }

    // ---- negotiate: empty inputs ------------------------------------------

    #[test]
    fn negotiate_empty_manifest() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported, vec![Capability::Streaming]);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_empty_requirements() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = CapabilityRequirements::default();
        let res = negotiate(&m, &r);
        assert!(res.native.is_empty());
        assert!(res.emulated.is_empty());
        assert!(res.unsupported.is_empty());
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_both_empty() {
        let res = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
        assert!(res.is_compatible());
        assert_eq!(res.total(), 0);
    }

    // ---- negotiate: restricted handling -----------------------------------

    #[test]
    fn negotiate_restricted_treated_as_emulated() {
        let m = manifest_from(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let r = require_native(&[Capability::ToolBash]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulated, vec![Capability::ToolBash]);
        assert!(res.is_compatible());
    }

    // ---- negotiate: explicit unsupported in manifest ----------------------

    #[test]
    fn negotiate_explicit_unsupported_in_manifest() {
        let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        let r = require_native(&[Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported, vec![Capability::Logprobs]);
    }

    // ---- negotiate: order preservation ------------------------------------

    #[test]
    fn negotiate_preserves_requirement_order() {
        let m = manifest_from(&[
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let r = require_native(&[
            Capability::ToolRead,
            Capability::Streaming,
            Capability::ToolWrite,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(
            res.native,
            vec![
                Capability::ToolRead,
                Capability::Streaming,
                Capability::ToolWrite
            ]
        );
    }

    // ---- negotiate: duplicates --------------------------------------------

    #[test]
    fn negotiate_duplicate_requirements() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = require_native(&[Capability::Streaming, Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native.len(), 2);
    }

    #[test]
    fn negotiate_duplicate_mixed_outcomes() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Logprobs, Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported.len(), 2);
    }

    // ---- negotiate_capabilities (simple &[Capability] form) ---------------

    #[test]
    fn negotiate_capabilities_all_native() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let res = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
        assert_eq!(res.native.len(), 2);
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_capabilities_mixed() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Emulated),
        ]);
        let res = negotiate_capabilities(
            &[Capability::Streaming, Capability::Vision, Capability::Audio],
            &m,
        );
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert_eq!(res.emulated, vec![Capability::Vision]);
        assert_eq!(res.unsupported, vec![Capability::Audio]);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_capabilities_empty_required() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let res = negotiate_capabilities(&[], &m);
        assert!(res.is_compatible());
        assert_eq!(res.total(), 0);
    }

    #[test]
    fn negotiate_capabilities_fail_fast_check() {
        // Unsupported capabilities land in unsupported vec so callers can fail fast
        let m: CapabilityManifest = BTreeMap::new();
        let res = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported.len(), 1);
    }

    // ---- check_capability -------------------------------------------------

    #[test]
    fn check_capability_native() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Native
        );
    }

    #[test]
    fn check_capability_emulated() {
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        assert!(matches!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Emulated { .. }
        ));
    }

    #[test]
    fn check_capability_unsupported_explicit() {
        let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        assert!(matches!(
            check_capability(&m, &Capability::Logprobs),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn check_capability_missing() {
        let m: CapabilityManifest = BTreeMap::new();
        assert!(matches!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn check_capability_restricted() {
        let m = manifest_from(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "policy".into(),
            },
        )]);
        let level = check_capability(&m, &Capability::ToolBash);
        assert!(matches!(level, SupportLevel::Restricted { .. }));
        if let SupportLevel::Restricted { reason } = level {
            assert!(reason.contains("policy"));
        }
    }

    #[test]
    fn check_capability_all_variants() {
        let cases: Vec<(CoreSupportLevel, bool)> = vec![
            (CoreSupportLevel::Native, true),
            (CoreSupportLevel::Emulated, true),
            (CoreSupportLevel::Unsupported, false),
            (CoreSupportLevel::Restricted { reason: "x".into() }, true),
        ];
        for (core_level, should_satisfy) in cases {
            let m = manifest_from(&[(Capability::Streaming, core_level)]);
            let level = check_capability(&m, &Capability::Streaming);
            let satisfied = !matches!(level, SupportLevel::Unsupported { .. });
            assert_eq!(satisfied, should_satisfy);
        }
    }

    // ---- generate_report --------------------------------------------------

    #[test]
    fn report_fully_compatible() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolRead],
            emulated: vec![Capability::ToolWrite],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 2);
        assert_eq!(report.emulated_count, 1);
        assert_eq!(report.unsupported_count, 0);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn report_incompatible() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert!(report.summary.contains("incompatible"));
    }

    #[test]
    fn report_empty_result() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 0);
        assert_eq!(report.emulated_count, 0);
        assert_eq!(report.unsupported_count, 0);
    }

    #[test]
    fn report_counts_match_result() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead, Capability::ToolWrite],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&result);
        assert_eq!(report.native_count, result.native.len());
        assert_eq!(report.emulated_count, result.emulated.len());
        assert_eq!(report.unsupported_count, result.unsupported.len());
    }

    #[test]
    fn report_details_length() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&result);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_summary_counts() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolUse],
            emulated: vec![Capability::ToolBash],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.summary.contains("2 native"));
        assert!(report.summary.contains("1 emulated"));
        assert!(report.summary.contains("0 unsupported"));
    }

    #[test]
    fn report_all_emulated_still_compatible() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![Capability::Streaming, Capability::ToolRead],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.compatible);
        assert!(report.summary.contains("fully compatible"));
    }

    // ---- NegotiationResult helpers ----------------------------------------

    #[test]
    fn negotiation_result_total() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn negotiation_result_is_compatible_true() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        assert!(result.is_compatible());
    }

    #[test]
    fn negotiation_result_is_compatible_false() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![Capability::Streaming],
        };
        assert!(!result.is_compatible());
    }

    // ---- edge cases -------------------------------------------------------

    #[test]
    fn negotiate_large_manifest_small_requirements() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::ToolEdit, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
            (Capability::ToolGlob, CoreSupportLevel::Native),
        ]);
        let r = require_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert!(res.is_compatible());
        assert_eq!(res.total(), 1);
    }

    #[test]
    fn negotiate_single_native() {
        let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        let r = require_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::ToolUse]);
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_single_emulated() {
        let m = manifest_from(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let r = require_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulated, vec![Capability::ToolUse]);
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_single_unsupported() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported, vec![Capability::ToolUse]);
        assert!(!res.is_compatible());
    }

    // ---- serde round-trip -------------------------------------------------

    #[test]
    fn support_level_serde_roundtrip() {
        let levels = vec![
            SupportLevel::Native,
            SupportLevel::Emulated {
                method: "polyfill".into(),
            },
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
            SupportLevel::Unsupported {
                reason: "not available".into(),
            },
        ];
        for level in &levels {
            let json = serde_json::to_string(level).unwrap();
            let back: SupportLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, level);
        }
    }

    #[test]
    fn negotiation_result_serde_roundtrip() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        let json = serde_json::to_string(&report).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }

    // ---- CapabilityRegistry -----------------------------------------------

    #[test]
    fn registry_new_is_empty() {
        let reg = CapabilityRegistry::new();
        assert!(reg.names().is_empty());
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = CapabilityRegistry::new();
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("test-backend", m);
        assert!(reg.get("test-backend").is_some());
        assert!(reg.get("test-backend").unwrap().contains_key(&Capability::Streaming));
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn registry_names() {
        let mut reg = CapabilityRegistry::new();
        reg.register("a", BTreeMap::new());
        reg.register("b", BTreeMap::new());
        let mut names = reg.names();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn registry_overwrite() {
        let mut reg = CapabilityRegistry::new();
        let m1 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let m2 = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        reg.register("x", m1);
        reg.register("x", m2);
        // After overwrite the manifest should reflect the second registration
        let got = reg.get("x").unwrap();
        assert!(matches!(got.get(&Capability::Streaming), Some(CoreSupportLevel::Emulated)));
    }

    #[test]
    fn registry_negotiate_by_name() {
        let mut reg = CapabilityRegistry::new();
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("b", m);
        let res = reg.negotiate_by_name("b", &[Capability::Streaming]);
        assert!(res.is_some());
        assert!(res.unwrap().is_compatible());
    }

    #[test]
    fn registry_negotiate_by_name_missing() {
        let reg = CapabilityRegistry::new();
        assert!(reg.negotiate_by_name("nope", &[Capability::Streaming]).is_none());
    }

    #[test]
    fn registry_with_defaults_has_three_backends() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.names().len(), 3);
        assert!(reg.get("openai/gpt-4o").is_some());
        assert!(reg.get("anthropic/claude-3.5-sonnet").is_some());
        assert!(reg.get("google/gemini-1.5-pro").is_some());
    }

    // ---- Pre-populated manifest tests ------------------------------------

    #[test]
    fn openai_gpt4o_streaming_native() {
        let m = openai_gpt4o_manifest();
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Native
        );
    }

    #[test]
    fn openai_gpt4o_extended_thinking_unsupported() {
        let m = openai_gpt4o_manifest();
        assert!(matches!(
            check_capability(&m, &Capability::ExtendedThinking),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn claude_35_sonnet_extended_thinking_native() {
        let m = claude_35_sonnet_manifest();
        assert_eq!(
            check_capability(&m, &Capability::ExtendedThinking),
            SupportLevel::Native
        );
    }

    #[test]
    fn claude_35_sonnet_logprobs_unsupported() {
        let m = claude_35_sonnet_manifest();
        assert!(matches!(
            check_capability(&m, &Capability::Logprobs),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn gemini_15_pro_code_execution_native() {
        let m = gemini_15_pro_manifest();
        assert_eq!(
            check_capability(&m, &Capability::CodeExecution),
            SupportLevel::Native
        );
    }

    #[test]
    fn gemini_15_pro_vision_native() {
        let m = gemini_15_pro_manifest();
        assert_eq!(
            check_capability(&m, &Capability::Vision),
            SupportLevel::Native
        );
    }

    #[test]
    fn cross_model_negotiation_streaming_and_vision() {
        let required = &[Capability::Streaming, Capability::Vision];
        let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
        let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
        let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
        // All three support streaming + vision
        assert!(openai.is_compatible());
        assert!(claude.is_compatible());
        assert!(gemini.is_compatible());
    }

    #[test]
    fn cross_model_negotiation_extended_thinking() {
        let required = &[Capability::ExtendedThinking];
        let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
        let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
        let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
        // Only Claude supports extended thinking
        assert!(!openai.is_compatible());
        assert!(claude.is_compatible());
        assert!(!gemini.is_compatible());
    }

    #[test]
    fn cross_model_negotiation_audio() {
        let required = &[Capability::Audio];
        let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
        let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
        let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
        assert!(openai.is_compatible());
        assert!(!claude.is_compatible());
        assert!(gemini.is_compatible());
    }

    // ---- New capability variant tests ------------------------------------

    #[test]
    fn negotiate_new_capability_variants() {
        let m = manifest_from(&[
            (Capability::FunctionCalling, CoreSupportLevel::Native),
            (Capability::JsonMode, CoreSupportLevel::Emulated),
            (Capability::Temperature, CoreSupportLevel::Native),
            (Capability::TopK, CoreSupportLevel::Native),
            (Capability::CacheControl, CoreSupportLevel::Native),
        ]);
        let res = negotiate_capabilities(
            &[
                Capability::FunctionCalling,
                Capability::JsonMode,
                Capability::Temperature,
                Capability::TopK,
                Capability::CacheControl,
                Capability::Embeddings,
            ],
            &m,
        );
        assert_eq!(res.native.len(), 4);
        assert_eq!(res.emulated.len(), 1);
        assert_eq!(res.unsupported.len(), 1);
        assert!(!res.is_compatible());
    }
}
