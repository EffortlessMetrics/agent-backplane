// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
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

pub mod negotiate;

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirements, SupportLevel as CoreSupportLevel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a capability can be emulated when not natively supported.
///
/// # Examples
///
/// ```
/// use abp_capability::EmulationStrategy;
///
/// let strategy = EmulationStrategy::ClientSide;
/// assert_eq!(format!("{strategy}"), "client-side emulation");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulationStrategy {
    /// Emulated in ABP before sending to the backend.
    ClientSide,
    /// Degraded server-side behavior.
    ServerFallback,
    /// Best-effort approximation with possible fidelity loss.
    Approximate,
}

impl EmulationStrategy {
    /// Returns `true` if this strategy may have fidelity loss.
    #[must_use]
    pub fn has_fidelity_loss(&self) -> bool {
        matches!(self, Self::Approximate)
    }
}

impl fmt::Display for EmulationStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientSide => write!(f, "client-side emulation"),
            Self::ServerFallback => write!(f, "server fallback"),
            Self::Approximate => write!(f, "approximate"),
        }
    }
}

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

impl fmt::Display for SupportLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Emulated { method } => write!(f, "emulated ({method})"),
            Self::Restricted { reason } => write!(f, "restricted ({reason})"),
            Self::Unsupported { reason } => write!(f, "unsupported ({reason})"),
        }
    }
}

/// Outcome of negotiating a full set of requirements against a manifest.
///
/// # Examples
///
/// ```
/// use abp_capability::{NegotiationResult, EmulationStrategy};
/// use abp_core::Capability;
///
/// let result = NegotiationResult {
///     native: vec![Capability::Streaming],
///     emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
///     unsupported: vec![],
/// };
/// assert!(result.is_viable());
/// assert_eq!(result.total(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationResult {
    /// Capabilities the manifest supports natively.
    pub native: Vec<Capability>,
    /// Capabilities that can be emulated, with strategy.
    pub emulated: Vec<(Capability, EmulationStrategy)>,
    /// Capabilities that cannot be fulfilled, with reason.
    pub unsupported: Vec<(Capability, String)>,
}

impl NegotiationResult {
    /// Returns `true` when no required capabilities are unsupported.
    #[must_use]
    pub fn is_viable(&self) -> bool {
        self.unsupported.is_empty()
    }

    /// Alias for [`is_viable`](Self::is_viable) for backward compatibility.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.is_viable()
    }

    /// Total number of capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.native.len() + self.emulated.len() + self.unsupported.len()
    }

    /// Returns emulated capabilities that may have fidelity loss.
    #[must_use]
    pub fn warnings(&self) -> Vec<&(Capability, EmulationStrategy)> {
        self.emulated
            .iter()
            .filter(|(_, strategy)| strategy.has_fidelity_loss())
            .collect()
    }

    /// Extract just the emulated capability names.
    #[must_use]
    pub fn emulated_caps(&self) -> Vec<Capability> {
        self.emulated.iter().map(|(c, _)| c.clone()).collect()
    }

    /// Extract just the unsupported capability names.
    #[must_use]
    pub fn unsupported_caps(&self) -> Vec<Capability> {
        self.unsupported.iter().map(|(c, _)| c.clone()).collect()
    }

    /// Construct from simple capability lists (backward-compatible helper).
    #[must_use]
    pub fn from_simple(
        native: Vec<Capability>,
        emulated: Vec<Capability>,
        unsupported: Vec<Capability>,
    ) -> Self {
        Self {
            native,
            emulated: emulated
                .into_iter()
                .map(|c| (c, EmulationStrategy::ClientSide))
                .collect(),
            unsupported: unsupported
                .into_iter()
                .map(|c| (c, "not available".to_string()))
                .collect(),
        }
    }
}

impl fmt::Display for NegotiationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.is_viable() {
            "viable"
        } else {
            "not viable"
        };
        write!(
            f,
            "{} native, {} emulated, {} unsupported \u{2014} {verdict}",
            self.native.len(),
            self.emulated.len(),
            self.unsupported.len(),
        )
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
/// let result = NegotiationResult::from_simple(
///     vec![Capability::Streaming],
///     vec![Capability::ToolRead],
///     vec![],
/// );
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
    /// Per-capability details (capability name, support level).
    pub details: Vec<(String, SupportLevel)>,
}

impl fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary)
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Stores [`CapabilityManifest`]s for known dialects/backends.
///
/// Use [`CapabilityRegistry::with_defaults`] to get a registry pre-populated
/// with manifests for all six supported dialects (OpenAI, Claude, Gemini,
/// Kimi, Codex, Copilot).
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

    /// Create a registry pre-populated with manifests for all six dialects.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register("openai/gpt-4o", openai_gpt4o_manifest());
        reg.register("anthropic/claude-3.5-sonnet", claude_35_sonnet_manifest());
        reg.register("google/gemini-1.5-pro", gemini_15_pro_manifest());
        reg.register("moonshot/kimi", kimi_manifest());
        reg.register("openai/codex", codex_manifest());
        reg.register("github/copilot", copilot_manifest());
        reg
    }

    /// Register a manifest under the given name (dialect/model identifier).
    pub fn register(&mut self, name: &str, manifest: CapabilityManifest) {
        self.manifests.insert(name.to_owned(), manifest);
    }

    /// Remove a manifest by name. Returns `true` if it existed.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.manifests.remove(name).is_some()
    }

    /// Look up a manifest by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&CapabilityManifest> {
        self.manifests.get(name)
    }

    /// Returns `true` if a manifest with the given name exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.manifests.contains_key(name)
    }

    /// Return all registered names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.manifests.keys().map(String::as_str).collect()
    }

    /// Return the number of registered manifests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Returns `true` if no manifests are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Query a specific capability across all registered backends.
    #[must_use]
    pub fn query_capability(&self, cap: &Capability) -> Vec<(&str, SupportLevel)> {
        self.manifests
            .iter()
            .map(|(name, manifest)| (name.as_str(), check_capability(manifest, cap)))
            .collect()
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

    /// Compare source and target backends to find capability gaps.
    ///
    /// Returns a [`NegotiationResult`] showing which source capabilities the
    /// target can provide natively, which need emulation, and which are
    /// unsupported. Returns `None` if either name is not registered.
    #[must_use]
    pub fn compare(&self, source: &str, target: &str) -> Option<NegotiationResult> {
        let source_manifest = self.manifests.get(source)?;
        let target_manifest = self.manifests.get(target)?;

        // All capabilities from source that are not Unsupported are "required"
        let required: Vec<Capability> = source_manifest
            .iter()
            .filter(|(_, level)| !matches!(level, CoreSupportLevel::Unsupported))
            .map(|(cap, _)| cap.clone())
            .collect();

        Some(negotiate_capabilities(&required, target_manifest))
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Determine the default emulation strategy for a capability.
#[must_use]
pub fn default_emulation_strategy(cap: &Capability) -> EmulationStrategy {
    match cap {
        // Client-side polyfills
        Capability::StructuredOutputJsonSchema
        | Capability::JsonMode
        | Capability::PdfInput
        | Capability::CodeExecution
        | Capability::ToolRead
        | Capability::ToolWrite
        | Capability::ToolEdit
        | Capability::ToolBash
        | Capability::ToolGlob
        | Capability::ToolGrep
        | Capability::ToolWebSearch
        | Capability::ToolWebFetch
        | Capability::ToolAskUser
        | Capability::HooksPreToolUse
        | Capability::HooksPostToolUse
        | Capability::Checkpointing => EmulationStrategy::ClientSide,

        // Server can provide degraded version
        Capability::FunctionCalling
        | Capability::ToolUse
        | Capability::ExtendedThinking
        | Capability::BatchMode
        | Capability::SessionResume
        | Capability::SessionFork
        | Capability::McpClient
        | Capability::McpServer
        | Capability::SystemMessage => EmulationStrategy::ServerFallback,

        // Best-effort approximation
        Capability::Vision
        | Capability::ImageInput
        | Capability::Audio
        | Capability::ImageGeneration
        | Capability::Embeddings
        | Capability::CacheControl
        | Capability::Logprobs
        | Capability::SeedDeterminism
        | Capability::Streaming
        | Capability::StopSequences
        | Capability::Temperature
        | Capability::TopP
        | Capability::TopK
        | Capability::MaxTokens
        | Capability::FrequencyPenalty
        | Capability::PresencePenalty => EmulationStrategy::Approximate,
    }
}

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

/// Negotiate a set of required capabilities against a manifest.
///
/// This is the primary negotiation entry point. Each required capability is
/// classified via [`check_capability`] and placed into the appropriate bucket.
/// Emulated capabilities include an [`EmulationStrategy`]. Unsupported
/// capabilities include a reason string.
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
/// assert_eq!(result.unsupported_caps(), vec![Capability::ToolUse]);
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
                emulated.push((cap.clone(), default_emulation_strategy(cap)));
            }
            SupportLevel::Unsupported { reason } => {
                unsupported.push((cap.clone(), reason));
            }
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
    let caps: Vec<Capability> = requirements
        .required
        .iter()
        .map(|r| r.capability.clone())
        .collect();
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
/// let result = NegotiationResult::from_simple(
///     vec![Capability::Streaming],
///     vec![],
///     vec![],
/// );
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
    for (cap, _strategy) in &result.emulated {
        details.push((
            format!("{cap:?}"),
            SupportLevel::Emulated {
                method: "adapter".into(),
            },
        ));
    }
    for (cap, reason) in &result.unsupported {
        details.push((
            format!("{cap:?}"),
            SupportLevel::Unsupported {
                reason: reason.clone(),
            },
        ));
    }

    let verdict = if compatible {
        "fully compatible"
    } else {
        "incompatible"
    };

    let summary = format!(
        "{} native, {} emulated, {} unsupported \u{2014} {verdict}",
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
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Native,
        ),
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
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Emulated,
        ),
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
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Native,
        ),
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

/// Capability manifest for **Moonshot Kimi**.
#[must_use]
pub fn kimi_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::ImageInput, CoreSupportLevel::Native),
        (Capability::SystemMessage, CoreSupportLevel::Native),
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::StopSequences, CoreSupportLevel::Native),
        (Capability::JsonMode, CoreSupportLevel::Native),
        (Capability::FrequencyPenalty, CoreSupportLevel::Native),
        (Capability::PresencePenalty, CoreSupportLevel::Native),
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Emulated,
        ),
        (Capability::Embeddings, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
        (Capability::PdfInput, CoreSupportLevel::Unsupported),
        (Capability::CodeExecution, CoreSupportLevel::Unsupported),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (Capability::SeedDeterminism, CoreSupportLevel::Unsupported),
        (Capability::TopK, CoreSupportLevel::Unsupported),
        (Capability::CacheControl, CoreSupportLevel::Unsupported),
        (Capability::BatchMode, CoreSupportLevel::Unsupported),
        (Capability::ImageGeneration, CoreSupportLevel::Unsupported),
    ])
}

/// Capability manifest for **OpenAI Codex**.
#[must_use]
pub fn codex_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::ToolEdit, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Native),
        (Capability::ToolGlob, CoreSupportLevel::Native),
        (Capability::ToolGrep, CoreSupportLevel::Native),
        (Capability::CodeExecution, CoreSupportLevel::Native),
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Native,
        ),
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
        (Capability::BatchMode, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::ImageInput, CoreSupportLevel::Emulated),
        (Capability::Embeddings, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
        (Capability::PdfInput, CoreSupportLevel::Unsupported),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
        (Capability::TopK, CoreSupportLevel::Unsupported),
        (Capability::CacheControl, CoreSupportLevel::Unsupported),
        (Capability::ImageGeneration, CoreSupportLevel::Unsupported),
    ])
}

/// Capability manifest for **GitHub Copilot**.
#[must_use]
pub fn copilot_manifest() -> CapabilityManifest {
    BTreeMap::from([
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
        (Capability::FunctionCalling, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::ToolEdit, CoreSupportLevel::Native),
        (Capability::ToolBash, CoreSupportLevel::Native),
        (Capability::ToolGlob, CoreSupportLevel::Native),
        (Capability::ToolGrep, CoreSupportLevel::Native),
        (Capability::ToolWebSearch, CoreSupportLevel::Native),
        (Capability::ToolWebFetch, CoreSupportLevel::Native),
        (Capability::ToolAskUser, CoreSupportLevel::Native),
        (Capability::SystemMessage, CoreSupportLevel::Native),
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::StopSequences, CoreSupportLevel::Native),
        (Capability::CodeExecution, CoreSupportLevel::Emulated),
        (
            Capability::StructuredOutputJsonSchema,
            CoreSupportLevel::Emulated,
        ),
        (Capability::JsonMode, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::ImageInput, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
        (Capability::PdfInput, CoreSupportLevel::Unsupported),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (Capability::SeedDeterminism, CoreSupportLevel::Unsupported),
        (Capability::TopK, CoreSupportLevel::Unsupported),
        (Capability::FrequencyPenalty, CoreSupportLevel::Unsupported),
        (Capability::PresencePenalty, CoreSupportLevel::Unsupported),
        (Capability::CacheControl, CoreSupportLevel::Unsupported),
        (Capability::BatchMode, CoreSupportLevel::Unsupported),
        (Capability::Embeddings, CoreSupportLevel::Unsupported),
        (Capability::ImageGeneration, CoreSupportLevel::Unsupported),
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

    // ---- EmulationStrategy ------------------------------------------------

    #[test]
    fn emulation_strategy_display() {
        assert_eq!(
            format!("{}", EmulationStrategy::ClientSide),
            "client-side emulation"
        );
        assert_eq!(
            format!("{}", EmulationStrategy::ServerFallback),
            "server fallback"
        );
        assert_eq!(format!("{}", EmulationStrategy::Approximate), "approximate");
    }

    #[test]
    fn emulation_strategy_fidelity_loss() {
        assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
        assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
        assert!(EmulationStrategy::Approximate.has_fidelity_loss());
    }

    #[test]
    fn emulation_strategy_serde_roundtrip() {
        let strategies = vec![
            EmulationStrategy::ClientSide,
            EmulationStrategy::ServerFallback,
            EmulationStrategy::Approximate,
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, s);
        }
    }

    // ---- SupportLevel Display ---------------------------------------------

    #[test]
    fn support_level_display() {
        assert_eq!(format!("{}", SupportLevel::Native), "native");
        assert_eq!(
            format!(
                "{}",
                SupportLevel::Emulated {
                    method: "polyfill".into()
                }
            ),
            "emulated (polyfill)"
        );
        assert_eq!(
            format!(
                "{}",
                SupportLevel::Restricted {
                    reason: "sandbox".into()
                }
            ),
            "restricted (sandbox)"
        );
        assert_eq!(
            format!(
                "{}",
                SupportLevel::Unsupported {
                    reason: "N/A".into()
                }
            ),
            "unsupported (N/A)"
        );
    }

    // ---- NegotiationResult: is_viable & warnings --------------------------

    #[test]
    fn negotiation_result_is_viable_true() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![],
        };
        assert!(result.is_viable());
        assert!(result.is_compatible());
    }

    #[test]
    fn negotiation_result_is_viable_false() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![(Capability::Streaming, "missing".into())],
        };
        assert!(!result.is_viable());
    }

    #[test]
    fn negotiation_result_warnings_empty() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![],
        };
        assert!(result.warnings().is_empty());
    }

    #[test]
    fn negotiation_result_warnings_approximate() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![
                (Capability::ToolRead, EmulationStrategy::ClientSide),
                (Capability::Vision, EmulationStrategy::Approximate),
                (Capability::Audio, EmulationStrategy::Approximate),
            ],
            unsupported: vec![],
        };
        let w = result.warnings();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].0, Capability::Vision);
        assert_eq!(w[1].0, Capability::Audio);
    }

    #[test]
    fn negotiation_result_display() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![(Capability::Logprobs, "N/A".into())],
        };
        let s = format!("{result}");
        assert!(s.contains("1 native"));
        assert!(s.contains("1 emulated"));
        assert!(s.contains("1 unsupported"));
        assert!(s.contains("not viable"));
    }

    #[test]
    fn negotiation_result_display_viable() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let s = format!("{result}");
        assert!(s.contains("viable"));
        assert!(!s.contains("not viable"));
    }

    // ---- NegotiationResult helpers ----------------------------------------

    #[test]
    fn negotiation_result_emulated_caps() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![
                (Capability::ToolRead, EmulationStrategy::ClientSide),
                (Capability::Vision, EmulationStrategy::Approximate),
            ],
            unsupported: vec![],
        };
        assert_eq!(
            result.emulated_caps(),
            vec![Capability::ToolRead, Capability::Vision]
        );
    }

    #[test]
    fn negotiation_result_unsupported_caps() {
        let result = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![
                (Capability::Logprobs, "no API".into()),
                (Capability::Audio, "not supported".into()),
            ],
        };
        assert_eq!(
            result.unsupported_caps(),
            vec![Capability::Logprobs, Capability::Audio]
        );
    }

    #[test]
    fn negotiation_result_from_simple() {
        let result = NegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Logprobs],
        );
        assert_eq!(result.native, vec![Capability::Streaming]);
        assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
        assert_eq!(result.unsupported_caps(), vec![Capability::Logprobs]);
        assert_eq!(result.total(), 3);
    }

    // ---- CompatibilityReport Display --------------------------------------

    #[test]
    fn compatibility_report_display() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let report = generate_report(&result);
        let s = format!("{report}");
        assert!(s.contains("fully compatible"));
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
        assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolWrite]);
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
        assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolEdit]);
        assert!(!res.is_compatible());
    }

    // ---- negotiate: empty inputs ------------------------------------------

    #[test]
    fn negotiate_empty_manifest() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
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
        assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
        assert!(res.is_compatible());
    }

    // ---- negotiate: explicit unsupported in manifest ----------------------

    #[test]
    fn negotiate_explicit_unsupported_in_manifest() {
        let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        let r = require_native(&[Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported_caps(), vec![Capability::Logprobs]);
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
        assert_eq!(res.emulated_caps(), vec![Capability::Vision]);
        assert_eq!(res.unsupported_caps(), vec![Capability::Audio]);
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
        let m: CapabilityManifest = BTreeMap::new();
        let res = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported.len(), 1);
    }

    // ---- negotiate_capabilities: emulation strategy selection --------------

    #[test]
    fn negotiate_emulated_includes_strategy() {
        let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
        let res = negotiate_capabilities(&[Capability::Vision], &m);
        assert_eq!(res.emulated.len(), 1);
        assert_eq!(res.emulated[0].0, Capability::Vision);
        assert_eq!(res.emulated[0].1, EmulationStrategy::Approximate);
    }

    #[test]
    fn negotiate_unsupported_includes_reason() {
        let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        let res = negotiate_capabilities(&[Capability::Logprobs], &m);
        assert_eq!(res.unsupported.len(), 1);
        assert_eq!(res.unsupported[0].0, Capability::Logprobs);
        assert!(!res.unsupported[0].1.is_empty());
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
            emulated: vec![(Capability::ToolWrite, EmulationStrategy::ClientSide)],
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
            unsupported: vec![(Capability::Logprobs, "no API".into())],
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
            emulated: vec![
                (Capability::ToolRead, EmulationStrategy::ClientSide),
                (Capability::ToolWrite, EmulationStrategy::ServerFallback),
            ],
            unsupported: vec![(Capability::Logprobs, "N/A".into())],
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
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![(Capability::Logprobs, "N/A".into())],
        };
        let report = generate_report(&result);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_summary_counts() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolUse],
            emulated: vec![(Capability::ToolBash, EmulationStrategy::ClientSide)],
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
            emulated: vec![
                (Capability::Streaming, EmulationStrategy::ClientSide),
                (Capability::ToolRead, EmulationStrategy::ServerFallback),
            ],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.compatible);
        assert!(report.summary.contains("fully compatible"));
    }

    // ---- NegotiationResult basic helpers ----------------------------------

    #[test]
    fn negotiation_result_total() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![(Capability::Logprobs, "N/A".into())],
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
            unsupported: vec![(Capability::Streaming, "missing".into())],
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
        assert_eq!(res.emulated_caps(), vec![Capability::ToolUse]);
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_single_unsupported() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolUse]);
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
            emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
            unsupported: vec![(Capability::Logprobs, "no API".into())],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
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
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = CapabilityRegistry::new();
        let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        reg.register("test-backend", m);
        assert!(reg.get("test-backend").is_some());
        assert!(reg.contains("test-backend"));
        assert!(
            reg.get("test-backend")
                .unwrap()
                .contains_key(&Capability::Streaming)
        );
        assert!(reg.get("missing").is_none());
        assert!(!reg.contains("missing"));
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
        let got = reg.get("x").unwrap();
        assert!(matches!(
            got.get(&Capability::Streaming),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn registry_unregister() {
        let mut reg = CapabilityRegistry::new();
        reg.register("a", BTreeMap::new());
        assert!(reg.unregister("a"));
        assert!(!reg.unregister("a"));
        assert!(reg.is_empty());
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
        assert!(
            reg.negotiate_by_name("nope", &[Capability::Streaming])
                .is_none()
        );
    }

    #[test]
    fn registry_with_defaults_has_six_backends() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.len(), 6);
        assert!(reg.contains("openai/gpt-4o"));
        assert!(reg.contains("anthropic/claude-3.5-sonnet"));
        assert!(reg.contains("google/gemini-1.5-pro"));
        assert!(reg.contains("moonshot/kimi"));
        assert!(reg.contains("openai/codex"));
        assert!(reg.contains("github/copilot"));
    }

    #[test]
    fn registry_query_capability() {
        let reg = CapabilityRegistry::with_defaults();
        let results = reg.query_capability(&Capability::Streaming);
        assert_eq!(results.len(), 6);
        assert!(
            results
                .iter()
                .all(|(_, level)| matches!(level, SupportLevel::Native))
        );
    }

    #[test]
    fn registry_compare_claude_to_openai() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg
            .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
            .unwrap();
        assert!(
            result
                .unsupported_caps()
                .contains(&Capability::ExtendedThinking)
        );
    }

    #[test]
    fn registry_compare_missing_source() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
    }

    #[test]
    fn registry_compare_missing_target() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.compare("openai/gpt-4o", "nonexistent").is_none());
    }

    #[test]
    fn registry_compare_same_backend() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg.compare("openai/gpt-4o", "openai/gpt-4o").unwrap();
        assert!(result.is_viable());
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
    fn kimi_streaming_native() {
        let m = kimi_manifest();
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Native
        );
    }

    #[test]
    fn kimi_code_execution_unsupported() {
        let m = kimi_manifest();
        assert!(matches!(
            check_capability(&m, &Capability::CodeExecution),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn codex_tool_bash_native() {
        let m = codex_manifest();
        assert_eq!(
            check_capability(&m, &Capability::ToolBash),
            SupportLevel::Native
        );
    }

    #[test]
    fn codex_code_execution_native() {
        let m = codex_manifest();
        assert_eq!(
            check_capability(&m, &Capability::CodeExecution),
            SupportLevel::Native
        );
    }

    #[test]
    fn copilot_tool_web_search_native() {
        let m = copilot_manifest();
        assert_eq!(
            check_capability(&m, &Capability::ToolWebSearch),
            SupportLevel::Native
        );
    }

    #[test]
    fn copilot_extended_thinking_unsupported() {
        let m = copilot_manifest();
        assert!(matches!(
            check_capability(&m, &Capability::ExtendedThinking),
            SupportLevel::Unsupported { .. }
        ));
    }

    #[test]
    fn cross_model_negotiation_streaming_and_vision() {
        let required = &[Capability::Streaming, Capability::Vision];
        let openai = negotiate_capabilities(required, &openai_gpt4o_manifest());
        let claude = negotiate_capabilities(required, &claude_35_sonnet_manifest());
        let gemini = negotiate_capabilities(required, &gemini_15_pro_manifest());
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

    #[test]
    fn cross_model_negotiation_all_six_streaming() {
        let required = &[Capability::Streaming];
        for manifest_fn in [
            openai_gpt4o_manifest,
            claude_35_sonnet_manifest,
            gemini_15_pro_manifest,
            kimi_manifest,
            codex_manifest,
            copilot_manifest,
        ] {
            let res = negotiate_capabilities(required, &manifest_fn());
            assert!(res.is_compatible(), "streaming should be native for all");
        }
    }

    #[test]
    fn cross_model_negotiation_codex_vs_copilot_tools() {
        let required = &[
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolBash,
        ];
        let codex = negotiate_capabilities(required, &codex_manifest());
        let copilot = negotiate_capabilities(required, &copilot_manifest());
        assert!(codex.is_compatible());
        assert!(copilot.is_compatible());
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

    // ---- default_emulation_strategy tests ---------------------------------

    #[test]
    fn default_strategy_client_side() {
        assert_eq!(
            default_emulation_strategy(&Capability::StructuredOutputJsonSchema),
            EmulationStrategy::ClientSide
        );
        assert_eq!(
            default_emulation_strategy(&Capability::CodeExecution),
            EmulationStrategy::ClientSide
        );
    }

    #[test]
    fn default_strategy_server_fallback() {
        assert_eq!(
            default_emulation_strategy(&Capability::FunctionCalling),
            EmulationStrategy::ServerFallback
        );
        assert_eq!(
            default_emulation_strategy(&Capability::ExtendedThinking),
            EmulationStrategy::ServerFallback
        );
    }

    #[test]
    fn default_strategy_approximate() {
        assert_eq!(
            default_emulation_strategy(&Capability::Vision),
            EmulationStrategy::Approximate
        );
        assert_eq!(
            default_emulation_strategy(&Capability::Embeddings),
            EmulationStrategy::Approximate
        );
    }
}
