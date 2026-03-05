// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Pre-execution capability preflight checks.
//!
//! [`check_preflight`] validates that a backend's capability manifest can
//! satisfy the requirements of a [`WorkOrder`] *before* execution begins.
//! When gaps exist, [`suggest_alternatives`] recommends emulation strategies
//! or alternative backends from a registry.

use crate::{
    CapabilityRegistry, EmulationStrategy, NegotiationResult, SupportLevel, check_capability,
    default_emulation_strategy, negotiate_capabilities,
};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel, WorkOrder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Describes a single missing or degraded capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityGap {
    /// A required tool capability is missing.
    MissingTool {
        /// The specific tool capability that is missing.
        capability: Capability,
    },
    /// Streaming support is missing.
    NoStreaming,
    /// Vision / image-input support is missing.
    NoVision,
    /// Function-calling support is missing.
    NoFunctionCalling,
    /// Code execution support is missing.
    NoCodeExecution,
    /// Web search support is missing.
    NoWebSearch,
    /// File access (read/write/edit) support is missing.
    NoFileAccess {
        /// Which file-access capabilities are missing.
        missing: Vec<Capability>,
    },
    /// An arbitrary capability is absent.
    Other {
        /// The capability that is missing.
        capability: Capability,
        /// Why it is considered missing.
        reason: String,
    },
}

impl CapabilityGap {
    /// Return the primary capability associated with this gap.
    #[must_use]
    pub fn capability(&self) -> Option<&Capability> {
        match self {
            Self::MissingTool { capability } | Self::Other { capability, .. } => Some(capability),
            Self::NoFileAccess { missing } => missing.first(),
            Self::NoStreaming => None,
            Self::NoVision => None,
            Self::NoFunctionCalling => None,
            Self::NoCodeExecution => None,
            Self::NoWebSearch => None,
        }
    }
}

impl fmt::Display for CapabilityGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTool { capability } => write!(f, "missing tool: {capability:?}"),
            Self::NoStreaming => write!(f, "streaming not supported"),
            Self::NoVision => write!(f, "vision not supported"),
            Self::NoFunctionCalling => write!(f, "function calling not supported"),
            Self::NoCodeExecution => write!(f, "code execution not supported"),
            Self::NoWebSearch => write!(f, "web search not supported"),
            Self::NoFileAccess { missing } => {
                write!(f, "file access not supported: {missing:?}")
            }
            Self::Other { capability, reason } => {
                write!(f, "{capability:?}: {reason}")
            }
        }
    }
}

/// Suggested action to address a [`CapabilityGap`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Alternative {
    /// An emulation strategy can bridge the gap.
    Emulate {
        /// The gap being addressed.
        gap: CapabilityGap,
        /// The emulation strategy to use.
        strategy: EmulationStrategy,
        /// Human-readable description.
        description: String,
    },
    /// A different backend supports the capability natively.
    UseBackend {
        /// The recommended backend name.
        backend: String,
        /// Capabilities it would satisfy that the current one cannot.
        satisfies: Vec<Capability>,
    },
}

impl fmt::Display for Alternative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Emulate {
                gap,
                strategy,
                description,
            } => {
                write!(f, "emulate {gap} via {strategy}: {description}")
            }
            Self::UseBackend {
                backend, satisfies, ..
            } => {
                write!(f, "use backend \"{backend}\" (satisfies {satisfies:?})")
            }
        }
    }
}

/// Outcome of a preflight check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PreflightResult {
    /// All required capabilities are natively supported.
    Ok {
        /// Capabilities confirmed as natively supported.
        native: Vec<Capability>,
    },
    /// Some capabilities require emulation but execution can proceed.
    Degraded {
        /// Natively supported capabilities.
        native: Vec<Capability>,
        /// Gaps that can be bridged via emulation.
        gaps: Vec<CapabilityGap>,
        /// Suggested emulation alternatives.
        alternatives: Vec<Alternative>,
    },
    /// One or more required capabilities cannot be satisfied.
    Failed {
        /// Gaps that cannot be bridged.
        gaps: Vec<CapabilityGap>,
        /// Suggested alternatives (backend switches, partial emulation).
        alternatives: Vec<Alternative>,
    },
}

impl PreflightResult {
    /// Returns `true` when execution can proceed (Ok or Degraded).
    #[must_use]
    pub fn can_proceed(&self) -> bool {
        matches!(self, Self::Ok { .. } | Self::Degraded { .. })
    }

    /// Returns `true` only when all capabilities are natively met.
    #[must_use]
    pub fn is_fully_met(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    /// Collect all gaps (empty for Ok).
    #[must_use]
    pub fn gaps(&self) -> Vec<&CapabilityGap> {
        match self {
            Self::Ok { .. } => vec![],
            Self::Degraded { gaps, .. } | Self::Failed { gaps, .. } => gaps.iter().collect(),
        }
    }
}

impl fmt::Display for PreflightResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok { native } => write!(f, "preflight ok: {} native capabilities", native.len()),
            Self::Degraded { native, gaps, .. } => write!(
                f,
                "preflight degraded: {} native, {} gaps (emulation available)",
                native.len(),
                gaps.len(),
            ),
            Self::Failed { gaps, .. } => {
                write!(f, "preflight failed: {} unsatisfiable gaps", gaps.len())
            }
        }
    }
}

/// A backend scored for suitability against a set of requirements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredBackend {
    /// Backend name.
    pub name: String,
    /// Number of required capabilities natively supported.
    pub native_count: usize,
    /// Number of required capabilities that can be emulated.
    pub emulated_count: usize,
    /// Number of required capabilities that are unsupported.
    pub unsupported_count: usize,
    /// Score from 0.0 (nothing matched) to 1.0 (everything native).
    pub score: f64,
}

impl fmt::Display for ScoredBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: score={:.2} ({} native, {} emulated, {} unsupported)",
            self.name, self.score, self.native_count, self.emulated_count, self.unsupported_count,
        )
    }
}

// ---------------------------------------------------------------------------
// PreflightCheck
// ---------------------------------------------------------------------------

/// Validates work-order requirements against a backend's capabilities before
/// execution begins.
///
/// Construct via [`PreflightCheck::new`], then call [`PreflightCheck::run`]
/// to get a [`PreflightResult`].
pub struct PreflightCheck<'a> {
    required: Vec<Capability>,
    manifest: &'a CapabilityManifest,
}

impl<'a> PreflightCheck<'a> {
    /// Build a preflight check from a work order and backend manifest.
    #[must_use]
    pub fn new(work_order: &WorkOrder, manifest: &'a CapabilityManifest) -> Self {
        let required: Vec<Capability> = work_order
            .requirements
            .required
            .iter()
            .map(|r| r.capability.clone())
            .collect();
        Self { required, manifest }
    }

    /// Build a preflight check from raw capability list and manifest.
    #[must_use]
    pub fn from_capabilities(required: Vec<Capability>, manifest: &'a CapabilityManifest) -> Self {
        Self { required, manifest }
    }

    /// Execute the preflight check.
    #[must_use]
    pub fn run(&self) -> PreflightResult {
        check_preflight_inner(&self.required, self.manifest)
    }
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Perform a preflight capability check.
///
/// Compares `required` capabilities against `backend_caps` and returns:
/// - [`PreflightResult::Ok`] if all are natively supported.
/// - [`PreflightResult::Degraded`] if gaps exist but every gap has an
///   emulation strategy.
/// - [`PreflightResult::Failed`] if any gap cannot be emulated.
#[must_use]
pub fn check_preflight(
    work_order: &WorkOrder,
    backend_caps: &CapabilityManifest,
) -> PreflightResult {
    let required: Vec<Capability> = work_order
        .requirements
        .required
        .iter()
        .map(|r| r.capability.clone())
        .collect();
    check_preflight_inner(&required, backend_caps)
}

/// Inner implementation shared by the public API and [`PreflightCheck`].
fn check_preflight_inner(
    required: &[Capability],
    backend_caps: &CapabilityManifest,
) -> PreflightResult {
    let result = negotiate_capabilities(required, backend_caps);

    if result.unsupported.is_empty() && result.emulated.is_empty() {
        return PreflightResult::Ok {
            native: result.native,
        };
    }

    // Classify gaps.
    let mut gaps = Vec::new();
    let mut can_emulate_all = true;

    // Emulated capabilities are gaps that *can* be bridged.
    for (cap, _strategy) in &result.emulated {
        gaps.push(classify_gap(cap));
    }

    // Unsupported capabilities — check if emulation is even feasible.
    for (cap, _reason) in &result.unsupported {
        gaps.push(classify_gap(cap));
        let strategy = default_emulation_strategy(cap);
        // Approximate emulation has low fidelity — treat as non-emulatable.
        if strategy == EmulationStrategy::Approximate {
            can_emulate_all = false;
        }
    }

    let alternatives = build_alternatives(&gaps);

    if result.unsupported.is_empty() || can_emulate_all {
        PreflightResult::Degraded {
            native: result.native,
            gaps,
            alternatives,
        }
    } else {
        PreflightResult::Failed { gaps, alternatives }
    }
}

/// Given a list of [`CapabilityGap`]s, suggest emulation strategies or
/// alternative backends from a [`CapabilityRegistry`].
#[must_use]
pub fn suggest_alternatives(gaps: &[CapabilityGap]) -> Vec<Alternative> {
    build_alternatives(gaps)
}

/// Suggest alternatives using a registry of known backends.
///
/// For each gap, the function first suggests emulation. Then it scans the
/// registry for backends that natively support the missing capabilities and
/// adds [`Alternative::UseBackend`] suggestions.
#[must_use]
pub fn suggest_alternatives_with_registry(
    gaps: &[CapabilityGap],
    registry: &CapabilityRegistry,
) -> Vec<Alternative> {
    let mut alts = build_alternatives(gaps);

    // Collect the capabilities we need to find.
    let needed: Vec<Capability> =
        gaps.iter()
            .filter_map(|g| match g {
                CapabilityGap::MissingTool { capability }
                | CapabilityGap::Other { capability, .. } => Some(capability.clone()),
                CapabilityGap::NoStreaming => Some(Capability::Streaming),
                CapabilityGap::NoVision => Some(Capability::Vision),
                CapabilityGap::NoFunctionCalling => Some(Capability::FunctionCalling),
                CapabilityGap::NoCodeExecution => Some(Capability::CodeExecution),
                CapabilityGap::NoWebSearch => Some(Capability::ToolWebSearch),
                CapabilityGap::NoFileAccess { missing } => missing.first().cloned(),
            })
            .collect();

    for name in registry.names() {
        if let Some(manifest) = registry.get(name) {
            let satisfies: Vec<Capability> = needed
                .iter()
                .filter(|cap| matches!(manifest.get(cap), Some(CoreSupportLevel::Native)))
                .cloned()
                .collect();
            if !satisfies.is_empty() {
                alts.push(Alternative::UseBackend {
                    backend: name.to_owned(),
                    satisfies,
                });
            }
        }
    }

    alts
}

/// Rank multiple backends by how well they satisfy the given requirements.
///
/// Returns a sorted list (best first) of [`ScoredBackend`]s. Native support
/// scores 1.0 per capability, emulated scores 0.5, unsupported scores 0.
#[must_use]
pub fn rank_backends(required: &[Capability], registry: &CapabilityRegistry) -> Vec<ScoredBackend> {
    let total = required.len();
    if total == 0 {
        return registry
            .names()
            .into_iter()
            .map(|name| ScoredBackend {
                name: name.to_owned(),
                native_count: 0,
                emulated_count: 0,
                unsupported_count: 0,
                score: 1.0,
            })
            .collect();
    }

    let mut scored: Vec<ScoredBackend> = registry
        .names()
        .into_iter()
        .filter_map(|name| {
            let manifest = registry.get(name)?;
            let result = negotiate_capabilities(required, manifest);
            let native = result.native.len();
            let emulated = result.emulated.len();
            let unsupported = result.unsupported.len();
            let score = (native as f64 + emulated as f64 * 0.5) / total as f64;
            Some(ScoredBackend {
                name: name.to_owned(),
                native_count: native,
                emulated_count: emulated,
                unsupported_count: unsupported,
                score,
            })
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Map a capability to the appropriate [`CapabilityGap`] variant.
fn classify_gap(cap: &Capability) -> CapabilityGap {
    match cap {
        Capability::Streaming => CapabilityGap::NoStreaming,
        Capability::Vision | Capability::ImageInput => CapabilityGap::NoVision,
        Capability::FunctionCalling => CapabilityGap::NoFunctionCalling,
        Capability::CodeExecution => CapabilityGap::NoCodeExecution,
        Capability::ToolWebSearch | Capability::ToolWebFetch => CapabilityGap::NoWebSearch,
        Capability::ToolRead | Capability::ToolWrite | Capability::ToolEdit => {
            CapabilityGap::NoFileAccess {
                missing: vec![cap.clone()],
            }
        }
        Capability::ToolBash
        | Capability::ToolGlob
        | Capability::ToolGrep
        | Capability::ToolAskUser
        | Capability::ToolUse => CapabilityGap::MissingTool {
            capability: cap.clone(),
        },
        other => CapabilityGap::Other {
            capability: other.clone(),
            reason: "not available in backend manifest".into(),
        },
    }
}

/// Build emulation-based [`Alternative`]s for each gap.
fn build_alternatives(gaps: &[CapabilityGap]) -> Vec<Alternative> {
    gaps.iter()
        .map(|gap| {
            let cap = resolve_gap_capability(gap);
            let strategy = default_emulation_strategy(&cap);
            let description = match &strategy {
                EmulationStrategy::ClientSide => {
                    format!("{cap:?}: client-side polyfill in ABP translation layer")
                }
                EmulationStrategy::ServerFallback => {
                    format!("{cap:?}: degraded server-side implementation")
                }
                EmulationStrategy::Approximate => {
                    format!("{cap:?}: best-effort approximation with fidelity loss")
                }
            };
            Alternative::Emulate {
                gap: gap.clone(),
                strategy,
                description,
            }
        })
        .collect()
}

/// Pick a representative capability for a gap so we can look up its emulation
/// strategy.
fn resolve_gap_capability(gap: &CapabilityGap) -> Capability {
    match gap {
        CapabilityGap::MissingTool { capability } | CapabilityGap::Other { capability, .. } => {
            capability.clone()
        }
        CapabilityGap::NoStreaming => Capability::Streaming,
        CapabilityGap::NoVision => Capability::Vision,
        CapabilityGap::NoFunctionCalling => Capability::FunctionCalling,
        CapabilityGap::NoCodeExecution => Capability::CodeExecution,
        CapabilityGap::NoWebSearch => Capability::ToolWebSearch,
        CapabilityGap::NoFileAccess { missing } => {
            missing.first().cloned().unwrap_or(Capability::ToolRead)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
        SupportLevel as CoreSupportLevel,
    };

    // -- helpers --

    fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    fn work_order_requiring(caps: &[Capability]) -> WorkOrder {
        use abp_core::*;
        WorkOrder {
            id: uuid::Uuid::nil(),
            task: "test".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: ".".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements {
                required: caps
                    .iter()
                    .map(|c| CapabilityRequirement {
                        capability: c.clone(),
                        min_support: MinSupport::Native,
                    })
                    .collect(),
            },
            config: RuntimeConfig::default(),
        }
    }

    // ---- PreflightResult variants ----------------------------------------

    #[test]
    fn preflight_ok_when_all_native() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let wo = work_order_requiring(&[Capability::Streaming, Capability::ToolUse]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
        assert!(r.can_proceed());
        assert!(r.gaps().is_empty());
    }

    #[test]
    fn preflight_degraded_when_emulated() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let wo = work_order_requiring(&[Capability::Streaming, Capability::ToolUse]);
        let r = check_preflight(&wo, &m);
        assert!(r.can_proceed());
        assert!(!r.is_fully_met());
        assert_eq!(r.gaps().len(), 1);
    }

    #[test]
    fn preflight_failed_when_unsupported() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let wo = work_order_requiring(&[Capability::Streaming, Capability::Vision]);
        let r = check_preflight(&wo, &m);
        assert!(!r.can_proceed());
        assert!(!r.is_fully_met());
    }

    // ---- CapabilityGap classification ------------------------------------

    #[test]
    fn gap_streaming() {
        let gap = classify_gap(&Capability::Streaming);
        assert!(matches!(gap, CapabilityGap::NoStreaming));
    }

    #[test]
    fn gap_vision() {
        let gap = classify_gap(&Capability::Vision);
        assert!(matches!(gap, CapabilityGap::NoVision));
    }

    #[test]
    fn gap_image_input_maps_to_vision() {
        let gap = classify_gap(&Capability::ImageInput);
        assert!(matches!(gap, CapabilityGap::NoVision));
    }

    #[test]
    fn gap_function_calling() {
        let gap = classify_gap(&Capability::FunctionCalling);
        assert!(matches!(gap, CapabilityGap::NoFunctionCalling));
    }

    #[test]
    fn gap_code_execution() {
        let gap = classify_gap(&Capability::CodeExecution);
        assert!(matches!(gap, CapabilityGap::NoCodeExecution));
    }

    #[test]
    fn gap_web_search() {
        let gap = classify_gap(&Capability::ToolWebSearch);
        assert!(matches!(gap, CapabilityGap::NoWebSearch));
    }

    #[test]
    fn gap_web_fetch_maps_to_web_search() {
        let gap = classify_gap(&Capability::ToolWebFetch);
        assert!(matches!(gap, CapabilityGap::NoWebSearch));
    }

    #[test]
    fn gap_file_access_read() {
        let gap = classify_gap(&Capability::ToolRead);
        assert!(matches!(gap, CapabilityGap::NoFileAccess { .. }));
    }

    #[test]
    fn gap_file_access_write() {
        let gap = classify_gap(&Capability::ToolWrite);
        assert!(matches!(gap, CapabilityGap::NoFileAccess { .. }));
    }

    #[test]
    fn gap_file_access_edit() {
        let gap = classify_gap(&Capability::ToolEdit);
        assert!(matches!(gap, CapabilityGap::NoFileAccess { .. }));
    }

    #[test]
    fn gap_tool_bash() {
        let gap = classify_gap(&Capability::ToolBash);
        assert!(matches!(gap, CapabilityGap::MissingTool { .. }));
    }

    #[test]
    fn gap_tool_use() {
        let gap = classify_gap(&Capability::ToolUse);
        assert!(matches!(gap, CapabilityGap::MissingTool { .. }));
    }

    #[test]
    fn gap_other_capability() {
        let gap = classify_gap(&Capability::Audio);
        assert!(matches!(gap, CapabilityGap::Other { .. }));
    }

    // ---- Empty / edge cases ----------------------------------------------

    #[test]
    fn preflight_empty_requirements() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let wo = work_order_requiring(&[]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
        assert!(r.can_proceed());
    }

    #[test]
    fn preflight_empty_manifest() {
        let m = manifest(&[]);
        let wo = work_order_requiring(&[Capability::Streaming]);
        let r = check_preflight(&wo, &m);
        assert!(!r.can_proceed());
    }

    #[test]
    fn preflight_both_empty() {
        let m = manifest(&[]);
        let wo = work_order_requiring(&[]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_single_unsupported() {
        let m = manifest(&[]);
        let wo = work_order_requiring(&[Capability::Audio]);
        let r = check_preflight(&wo, &m);
        assert!(!r.can_proceed());
    }

    // ---- Specific capability types ---------------------------------------

    #[test]
    fn preflight_tools_native() {
        let m = manifest(&[
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let wo = work_order_requiring(&[
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolBash,
        ]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_streaming_native() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let wo = work_order_requiring(&[Capability::Streaming]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_vision_native() {
        let m = manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
        let wo = work_order_requiring(&[Capability::Vision]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_code_execution_emulated() {
        let m = manifest(&[(Capability::CodeExecution, CoreSupportLevel::Emulated)]);
        let wo = work_order_requiring(&[Capability::CodeExecution]);
        let r = check_preflight(&wo, &m);
        assert!(r.can_proceed());
        assert!(!r.is_fully_met());
    }

    #[test]
    fn preflight_web_search_missing() {
        let m = manifest(&[]);
        let wo = work_order_requiring(&[Capability::ToolWebSearch]);
        let r = check_preflight(&wo, &m);
        assert!(!r.is_fully_met());
    }

    #[test]
    fn preflight_file_access_partial() {
        let m = manifest(&[
            (Capability::ToolRead, CoreSupportLevel::Native),
            // ToolWrite missing
        ]);
        let wo = work_order_requiring(&[Capability::ToolRead, Capability::ToolWrite]);
        let r = check_preflight(&wo, &m);
        assert!(!r.is_fully_met());
    }

    // ---- Emulation suggestions -------------------------------------------

    #[test]
    fn suggest_alternatives_for_streaming() {
        let gaps = vec![CapabilityGap::NoStreaming];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        assert!(matches!(&alts[0], Alternative::Emulate { .. }));
    }

    #[test]
    fn suggest_alternatives_for_vision() {
        let gaps = vec![CapabilityGap::NoVision];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        if let Alternative::Emulate { strategy, .. } = &alts[0] {
            assert_eq!(*strategy, EmulationStrategy::Approximate);
        } else {
            panic!("expected Emulate alternative");
        }
    }

    #[test]
    fn suggest_alternatives_for_function_calling() {
        let gaps = vec![CapabilityGap::NoFunctionCalling];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        if let Alternative::Emulate { strategy, .. } = &alts[0] {
            assert_eq!(*strategy, EmulationStrategy::ServerFallback);
        } else {
            panic!("expected Emulate alternative");
        }
    }

    #[test]
    fn suggest_alternatives_for_code_execution() {
        let gaps = vec![CapabilityGap::NoCodeExecution];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        if let Alternative::Emulate { strategy, .. } = &alts[0] {
            assert_eq!(*strategy, EmulationStrategy::ClientSide);
        } else {
            panic!("expected Emulate alternative");
        }
    }

    #[test]
    fn suggest_alternatives_for_web_search() {
        let gaps = vec![CapabilityGap::NoWebSearch];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        if let Alternative::Emulate { strategy, .. } = &alts[0] {
            assert_eq!(*strategy, EmulationStrategy::ClientSide);
        } else {
            panic!("expected Emulate alternative");
        }
    }

    #[test]
    fn suggest_alternatives_for_file_access() {
        let gaps = vec![CapabilityGap::NoFileAccess {
            missing: vec![Capability::ToolRead],
        }];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 1);
        if let Alternative::Emulate { strategy, .. } = &alts[0] {
            assert_eq!(*strategy, EmulationStrategy::ClientSide);
        } else {
            panic!("expected Emulate alternative");
        }
    }

    #[test]
    fn suggest_alternatives_empty_gaps() {
        let alts = suggest_alternatives(&[]);
        assert!(alts.is_empty());
    }

    #[test]
    fn suggest_alternatives_multiple_gaps() {
        let gaps = vec![
            CapabilityGap::NoStreaming,
            CapabilityGap::NoVision,
            CapabilityGap::NoFunctionCalling,
        ];
        let alts = suggest_alternatives(&gaps);
        assert_eq!(alts.len(), 3);
    }

    // ---- PreflightCheck struct -------------------------------------------

    #[test]
    fn preflight_check_from_work_order() {
        let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let wo = work_order_requiring(&[Capability::Streaming]);
        let check = PreflightCheck::new(&wo, &m);
        let r = check.run();
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_check_from_capabilities() {
        let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        let check = PreflightCheck::from_capabilities(vec![Capability::ToolUse], &m);
        let r = check.run();
        assert!(r.is_fully_met());
    }

    // ---- Multi-backend ranking -------------------------------------------

    #[test]
    fn rank_backends_default_registry() {
        let reg = CapabilityRegistry::with_defaults();
        let ranked = rank_backends(&[Capability::Streaming, Capability::Vision], &reg);
        assert!(!ranked.is_empty());
        // First should have highest score.
        assert!(ranked[0].score >= ranked.last().unwrap().score);
    }

    #[test]
    fn rank_backends_empty_requirements() {
        let reg = CapabilityRegistry::with_defaults();
        let ranked = rank_backends(&[], &reg);
        // All backends score 1.0 with no requirements.
        for sb in &ranked {
            assert!((sb.score - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn rank_backends_empty_registry() {
        let reg = CapabilityRegistry::new();
        let ranked = rank_backends(&[Capability::Streaming], &reg);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_backends_single_backend() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "test",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        let ranked = rank_backends(&[Capability::Streaming], &reg);
        assert_eq!(ranked.len(), 1);
        assert!((ranked[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(ranked[0].native_count, 1);
    }

    #[test]
    fn rank_backends_prefers_native_over_emulated() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "native-backend",
            manifest(&[(Capability::Vision, CoreSupportLevel::Native)]),
        );
        reg.register(
            "emulated-backend",
            manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]),
        );
        let ranked = rank_backends(&[Capability::Vision], &reg);
        assert_eq!(ranked[0].name, "native-backend");
    }

    #[test]
    fn rank_backends_scoring() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "full",
            manifest(&[
                (Capability::Streaming, CoreSupportLevel::Native),
                (Capability::Vision, CoreSupportLevel::Native),
            ]),
        );
        reg.register(
            "partial",
            manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        let ranked = rank_backends(&[Capability::Streaming, Capability::Vision], &reg);
        assert_eq!(ranked[0].name, "full");
        assert!((ranked[0].score - 1.0).abs() < f64::EPSILON);
        assert!(ranked[1].score < 1.0);
    }

    // ---- suggest_alternatives_with_registry ------------------------------

    #[test]
    fn suggest_with_registry_finds_backend() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "good-backend",
            manifest(&[(Capability::Vision, CoreSupportLevel::Native)]),
        );
        let gaps = vec![CapabilityGap::NoVision];
        let alts = suggest_alternatives_with_registry(&gaps, &reg);
        let backend_alts: Vec<_> = alts
            .iter()
            .filter(|a| matches!(a, Alternative::UseBackend { .. }))
            .collect();
        assert!(!backend_alts.is_empty());
    }

    #[test]
    fn suggest_with_registry_no_match() {
        let reg = CapabilityRegistry::new();
        let gaps = vec![CapabilityGap::NoVision];
        let alts = suggest_alternatives_with_registry(&gaps, &reg);
        // Only emulation alternatives, no backend alternatives.
        assert!(
            alts.iter()
                .all(|a| matches!(a, Alternative::Emulate { .. }))
        );
    }

    #[test]
    fn suggest_with_registry_multiple_backends() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "backend-a",
            manifest(&[(Capability::Vision, CoreSupportLevel::Native)]),
        );
        reg.register(
            "backend-b",
            manifest(&[(Capability::Vision, CoreSupportLevel::Native)]),
        );
        let gaps = vec![CapabilityGap::NoVision];
        let alts = suggest_alternatives_with_registry(&gaps, &reg);
        let backend_alts: Vec<_> = alts
            .iter()
            .filter(|a| matches!(a, Alternative::UseBackend { .. }))
            .collect();
        assert_eq!(backend_alts.len(), 2);
    }

    // ---- Display impls ---------------------------------------------------

    #[test]
    fn gap_display_missing_tool() {
        let gap = CapabilityGap::MissingTool {
            capability: Capability::ToolBash,
        };
        assert!(gap.to_string().contains("missing tool"));
    }

    #[test]
    fn gap_display_no_streaming() {
        let gap = CapabilityGap::NoStreaming;
        assert!(gap.to_string().contains("streaming"));
    }

    #[test]
    fn gap_display_no_vision() {
        let gap = CapabilityGap::NoVision;
        assert!(gap.to_string().contains("vision"));
    }

    #[test]
    fn gap_display_other() {
        let gap = CapabilityGap::Other {
            capability: Capability::Audio,
            reason: "test reason".into(),
        };
        let s = gap.to_string();
        assert!(s.contains("Audio"));
        assert!(s.contains("test reason"));
    }

    #[test]
    fn preflight_result_display_ok() {
        let r = PreflightResult::Ok {
            native: vec![Capability::Streaming],
        };
        assert!(r.to_string().contains("ok"));
    }

    #[test]
    fn preflight_result_display_degraded() {
        let r = PreflightResult::Degraded {
            native: vec![Capability::Streaming],
            gaps: vec![CapabilityGap::NoVision],
            alternatives: vec![],
        };
        assert!(r.to_string().contains("degraded"));
    }

    #[test]
    fn preflight_result_display_failed() {
        let r = PreflightResult::Failed {
            gaps: vec![CapabilityGap::NoVision],
            alternatives: vec![],
        };
        assert!(r.to_string().contains("failed"));
    }

    #[test]
    fn alternative_display_emulate() {
        let alt = Alternative::Emulate {
            gap: CapabilityGap::NoVision,
            strategy: EmulationStrategy::Approximate,
            description: "best-effort".into(),
        };
        assert!(alt.to_string().contains("emulate"));
    }

    #[test]
    fn alternative_display_use_backend() {
        let alt = Alternative::UseBackend {
            backend: "openai".into(),
            satisfies: vec![Capability::Vision],
        };
        assert!(alt.to_string().contains("openai"));
    }

    #[test]
    fn scored_backend_display() {
        let sb = ScoredBackend {
            name: "test".into(),
            native_count: 2,
            emulated_count: 1,
            unsupported_count: 0,
            score: 0.83,
        };
        assert!(sb.to_string().contains("test"));
        assert!(sb.to_string().contains("0.83"));
    }

    // ---- Capability gap helper method ------------------------------------

    #[test]
    fn gap_capability_for_missing_tool() {
        let gap = CapabilityGap::MissingTool {
            capability: Capability::ToolBash,
        };
        assert_eq!(gap.capability(), Some(&Capability::ToolBash));
    }

    #[test]
    fn gap_capability_for_no_streaming() {
        let gap = CapabilityGap::NoStreaming;
        assert_eq!(gap.capability(), None);
    }

    #[test]
    fn gap_capability_for_other() {
        let gap = CapabilityGap::Other {
            capability: Capability::Audio,
            reason: "test".into(),
        };
        assert_eq!(gap.capability(), Some(&Capability::Audio));
    }

    // ---- Serde roundtrips ------------------------------------------------

    #[test]
    fn capability_gap_serde_roundtrip() {
        let gap = CapabilityGap::MissingTool {
            capability: Capability::ToolBash,
        };
        let json = serde_json::to_string(&gap).unwrap();
        let back: CapabilityGap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gap);
    }

    #[test]
    fn preflight_result_ok_serde() {
        let r = PreflightResult::Ok {
            native: vec![Capability::Streaming],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: PreflightResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn preflight_result_degraded_serde() {
        let r = PreflightResult::Degraded {
            native: vec![Capability::Streaming],
            gaps: vec![CapabilityGap::NoVision],
            alternatives: vec![],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: PreflightResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn preflight_result_failed_serde() {
        let r = PreflightResult::Failed {
            gaps: vec![CapabilityGap::NoStreaming],
            alternatives: vec![],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: PreflightResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn alternative_emulate_serde() {
        let alt = Alternative::Emulate {
            gap: CapabilityGap::NoVision,
            strategy: EmulationStrategy::Approximate,
            description: "best-effort".into(),
        };
        let json = serde_json::to_string(&alt).unwrap();
        let back: Alternative = serde_json::from_str(&json).unwrap();
        assert_eq!(back, alt);
    }

    #[test]
    fn alternative_use_backend_serde() {
        let alt = Alternative::UseBackend {
            backend: "test".into(),
            satisfies: vec![Capability::Vision],
        };
        let json = serde_json::to_string(&alt).unwrap();
        let back: Alternative = serde_json::from_str(&json).unwrap();
        assert_eq!(back, alt);
    }

    // ---- Mixed / complex scenarios ---------------------------------------

    #[test]
    fn preflight_mixed_native_emulated_unsupported() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
            // Vision absent → unsupported
        ]);
        let wo = work_order_requiring(&[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ]);
        let r = check_preflight(&wo, &m);
        // Vision is Approximate emulation → Failed.
        assert!(!r.can_proceed());
    }

    #[test]
    fn preflight_degraded_with_client_side_emulation() {
        // ToolRead is emulated and uses ClientSide strategy.
        let m = manifest(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
        let wo = work_order_requiring(&[Capability::ToolRead]);
        let r = check_preflight(&wo, &m);
        assert!(r.can_proceed());
        assert!(!r.is_fully_met());
    }

    #[test]
    fn preflight_many_capabilities_all_native() {
        let m = manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
            (Capability::FunctionCalling, CoreSupportLevel::Native),
            (Capability::CodeExecution, CoreSupportLevel::Native),
            (Capability::ToolWebSearch, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let wo = work_order_requiring(&[
            Capability::Streaming,
            Capability::Vision,
            Capability::FunctionCalling,
            Capability::CodeExecution,
            Capability::ToolWebSearch,
            Capability::ToolRead,
        ]);
        let r = check_preflight(&wo, &m);
        assert!(r.is_fully_met());
    }

    #[test]
    fn preflight_all_emulated_client_side_only() {
        // All capabilities are emulated and use ClientSide strategy.
        let m = manifest(&[
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            (Capability::CodeExecution, CoreSupportLevel::Emulated),
            (Capability::ToolWebSearch, CoreSupportLevel::Emulated),
        ]);
        let wo = work_order_requiring(&[
            Capability::ToolRead,
            Capability::CodeExecution,
            Capability::ToolWebSearch,
        ]);
        let r = check_preflight(&wo, &m);
        assert!(r.can_proceed());
    }

    #[test]
    fn rank_backends_with_defaults_streaming_and_tool_use() {
        let reg = CapabilityRegistry::with_defaults();
        let ranked = rank_backends(&[Capability::Streaming, Capability::ToolUse], &reg);
        // All default backends support Streaming natively.
        assert!(ranked[0].score > 0.0);
        assert!(ranked[0].native_count >= 1);
    }

    #[test]
    fn preflight_result_gaps_count() {
        let r = PreflightResult::Degraded {
            native: vec![Capability::Streaming],
            gaps: vec![CapabilityGap::NoVision, CapabilityGap::NoFunctionCalling],
            alternatives: vec![],
        };
        assert_eq!(r.gaps().len(), 2);
    }
}
