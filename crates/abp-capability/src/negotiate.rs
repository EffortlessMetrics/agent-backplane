// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Pre-execution capability negotiation with policy-based enforcement.
//!
//! This module provides [`pre_negotiate`] to validate backend support before
//! running a work order, and [`apply_policy`] to enforce a [`NegotiationPolicy`]
//! against the negotiation result.
//!
//! It also provides [`negotiate`] for detailed negotiation with
//! [`NegotiationReport`] output, and `EmulationPlan` describing how each
//! emulated capability would be fulfilled.

use crate::{
    EmulationStrategy, NegotiationResult, SupportLevel, check_capability,
    default_emulation_strategy, negotiate_capabilities,
};
use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Policy that controls how negotiation failures are handled.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::NegotiationPolicy;
///
/// let policy = NegotiationPolicy::Strict;
/// assert!(matches!(policy, NegotiationPolicy::Strict));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NegotiationPolicy {
    /// Any unsupported capability causes a hard failure.
    #[default]
    Strict,
    /// Proceed with emulation; warn on unsupported capabilities.
    BestEffort,
    /// Proceed regardless; document all limitations.
    Permissive,
}

impl fmt::Display for NegotiationPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::BestEffort => write!(f, "best-effort"),
            Self::Permissive => write!(f, "permissive"),
        }
    }
}

/// Error returned when [`apply_policy`] rejects a negotiation result.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::{NegotiationError, NegotiationPolicy};
/// use abp_core::Capability;
///
/// let err = NegotiationError {
///     policy: NegotiationPolicy::Strict,
///     unsupported: vec![(Capability::Streaming, "not declared in manifest".into())],
///     warnings: vec![],
/// };
/// assert!(err.to_string().contains("strict"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiationError {
    /// The policy that was applied.
    pub policy: NegotiationPolicy,
    /// Capabilities that are unsupported.
    pub unsupported: Vec<(Capability, String)>,
    /// Capabilities that triggered warnings (emulated with fidelity loss).
    pub warnings: Vec<Capability>,
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capability negotiation failed (policy: {}): {} unsupported",
            self.policy,
            self.unsupported.len(),
        )?;
        if !self.unsupported.is_empty() {
            let names: Vec<String> = self
                .unsupported
                .iter()
                .map(|(c, _)| format!("{c:?}"))
                .collect();
            write!(f, " [{}]", names.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for NegotiationError {}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Pre-negotiate required capabilities against a backend manifest.
///
/// Checks each required capability against the manifest and returns a
/// [`NegotiationResult`] with supported/emulated/unsupported classifications.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::pre_negotiate;
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
/// manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);
///
/// let result = pre_negotiate(
///     &[Capability::Streaming, Capability::ToolUse, Capability::Vision],
///     &manifest,
/// );
/// assert_eq!(result.native.len(), 1);
/// assert_eq!(result.emulated.len(), 1);
/// assert_eq!(result.unsupported.len(), 1);
/// ```
#[must_use]
pub fn pre_negotiate(required: &[Capability], manifest: &CapabilityManifest) -> NegotiationResult {
    negotiate_capabilities(required, manifest)
}

/// Apply a [`NegotiationPolicy`] to a negotiation result.
///
/// - **Strict**: returns `Err` if any capabilities are unsupported.
/// - **BestEffort**: returns `Err` only if there are unsupported capabilities
///   that cannot be emulated (i.e., truly unsupported). Warnings are included
///   for emulated capabilities with fidelity loss.
/// - **Permissive**: always returns `Ok(())`.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::{pre_negotiate, apply_policy, NegotiationPolicy};
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let result = pre_negotiate(&[Capability::Streaming], &manifest);
/// assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
/// ```
pub fn apply_policy(
    result: &NegotiationResult,
    policy: NegotiationPolicy,
) -> Result<(), NegotiationError> {
    let warning_caps: Vec<Capability> = result
        .warnings()
        .into_iter()
        .map(|(c, _)| c.clone())
        .collect();

    match policy {
        NegotiationPolicy::Strict => {
            if result.unsupported.is_empty() {
                Ok(())
            } else {
                Err(NegotiationError {
                    policy,
                    unsupported: result.unsupported.clone(),
                    warnings: warning_caps,
                })
            }
        }
        NegotiationPolicy::BestEffort => {
            if result.unsupported.is_empty() {
                Ok(())
            } else {
                Err(NegotiationError {
                    policy,
                    unsupported: result.unsupported.clone(),
                    warnings: warning_caps,
                })
            }
        }
        NegotiationPolicy::Permissive => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// NegotiationReport: detailed negotiation output
// ---------------------------------------------------------------------------

/// A single emulation plan entry describing how a capability will be emulated.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::EmulationPlanEntry;
/// use abp_capability::EmulationStrategy;
/// use abp_core::Capability;
///
/// let entry = EmulationPlanEntry {
///     capability: Capability::ToolUse,
///     strategy: EmulationStrategy::ServerFallback,
///     detail: "via function calling API".into(),
/// };
/// assert_eq!(entry.capability, Capability::ToolUse);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulationPlanEntry {
    /// The capability being emulated.
    pub capability: Capability,
    /// The strategy used for emulation.
    pub strategy: EmulationStrategy,
    /// Human-readable detail of how emulation works.
    pub detail: String,
}

impl fmt::Display for EmulationPlanEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {} ({})",
            self.capability, self.strategy, self.detail
        )
    }
}

/// Comprehensive negotiation report with full detail of outcomes.
///
/// Categorizes every required capability as met (natively satisfied),
/// emulated (can approximate), or missing (unsupported).
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::negotiate;
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
/// manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);
///
/// let report = negotiate(
///     &[Capability::Streaming, Capability::ToolUse, Capability::Vision],
///     &manifest,
/// );
/// assert_eq!(report.met.len(), 1);
/// assert_eq!(report.emulated.len(), 1);
/// assert_eq!(report.missing.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationReport {
    /// Capabilities that are natively satisfied by the backend.
    pub met: Vec<Capability>,
    /// Capabilities that can be emulated/approximated, with strategy.
    pub emulated: Vec<EmulationPlanEntry>,
    /// Capabilities that are unsupported and cannot be fulfilled.
    pub missing: Vec<(Capability, String)>,
}

impl NegotiationReport {
    /// Returns `true` if all required capabilities are met or emulated.
    #[must_use]
    pub fn is_fully_met(&self) -> bool {
        self.missing.is_empty()
    }

    /// Returns `true` if any capabilities are missing.
    #[must_use]
    pub fn has_missing(&self) -> bool {
        !self.missing.is_empty()
    }

    /// Total number of capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.met.len() + self.emulated.len() + self.missing.len()
    }
}

impl fmt::Display for NegotiationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} met, {} emulated, {} missing",
            self.met.len(),
            self.emulated.len(),
            self.missing.len(),
        )
    }
}

/// Negotiate required capabilities against a backend manifest, producing
/// a detailed [`NegotiationReport`].
///
/// Each capability is classified as met (natively supported), emulated
/// (with an [`EmulationPlanEntry`] describing the strategy), or missing.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::negotiate;
/// use abp_core::{Capability, CapabilityManifest, SupportLevel as CoreSupportLevel};
///
/// let mut manifest = CapabilityManifest::new();
/// manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
///
/// let report = negotiate(&[Capability::Streaming], &manifest);
/// assert!(report.is_fully_met());
/// assert_eq!(report.met, vec![Capability::Streaming]);
/// ```
#[must_use]
pub fn negotiate(required: &[Capability], manifest: &CapabilityManifest) -> NegotiationReport {
    let mut met = Vec::new();
    let mut emulated = Vec::new();
    let mut missing = Vec::new();

    for cap in required {
        match check_capability(manifest, cap) {
            SupportLevel::Native => {
                met.push(cap.clone());
            }
            SupportLevel::Emulated { method } => {
                let strategy = default_emulation_strategy(cap);
                emulated.push(EmulationPlanEntry {
                    capability: cap.clone(),
                    strategy,
                    detail: method,
                });
            }
            SupportLevel::Restricted { reason } => {
                let strategy = default_emulation_strategy(cap);
                emulated.push(EmulationPlanEntry {
                    capability: cap.clone(),
                    strategy,
                    detail: format!("restricted: {reason}"),
                });
            }
            SupportLevel::Unsupported { reason } => {
                missing.push((cap.clone(), reason));
            }
        }
    }

    NegotiationReport {
        met,
        emulated,
        missing,
    }
}

// ---------------------------------------------------------------------------
// CapabilityNegotiator: stateful dialect-to-dialect negotiation
// ---------------------------------------------------------------------------

/// A single capability that is degraded (partial support with caveats).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DegradedFeature {
    /// The capability.
    pub capability: Capability,
    /// Support level label in the source dialect.
    pub source_level: String,
    /// Support level label in the target dialect.
    pub target_level: String,
    /// Human-readable caveats about the degradation.
    pub caveats: String,
}

impl fmt::Display for DegradedFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {} → {} ({})",
            self.capability, self.source_level, self.target_level, self.caveats,
        )
    }
}

/// Detailed result of dialect-to-dialect capability negotiation.
///
/// Unlike [`NegotiationResult`] which classifies into native/emulated/unsupported,
/// this adds a `degraded` category for capabilities that have partial support
/// with caveats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailedNegotiationResult {
    /// Source dialect name.
    pub source: String,
    /// Target dialect name.
    pub target: String,
    /// Capabilities that pass through natively (same or better support).
    pub native: Vec<Capability>,
    /// Capabilities that can be emulated with an emulation strategy.
    pub emulatable: Vec<(Capability, crate::EmulationStrategy)>,
    /// Capabilities that are completely unsupported and must fail early.
    pub unsupported: Vec<(Capability, String)>,
    /// Capabilities with partial support — degraded but usable with caveats.
    pub degraded: Vec<DegradedFeature>,
}

impl DetailedNegotiationResult {
    /// Returns `true` if no capabilities are unsupported.
    #[must_use]
    pub fn is_viable(&self) -> bool {
        self.unsupported.is_empty()
    }

    /// Total capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.native.len() + self.emulatable.len() + self.unsupported.len() + self.degraded.len()
    }
}

impl fmt::Display for DetailedNegotiationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} → {}: {} native, {} emulatable, {} degraded, {} unsupported",
            self.source,
            self.target,
            self.native.len(),
            self.emulatable.len(),
            self.degraded.len(),
            self.unsupported.len(),
        )
    }
}

/// Stateful negotiator that compares source and target dialect capabilities.
///
/// Takes source dialect capabilities + target dialect capabilities and produces
/// a [`DetailedNegotiationResult`] classifying features as native, emulatable,
/// degraded, or unsupported.
///
/// # Examples
///
/// ```
/// use abp_capability::negotiate::CapabilityNegotiator;
/// use abp_core::{Capability, SupportLevel as CoreSupportLevel};
/// use std::collections::BTreeMap;
///
/// let mut src = BTreeMap::new();
/// src.insert(Capability::Streaming, CoreSupportLevel::Native);
/// src.insert(Capability::Vision, CoreSupportLevel::Native);
///
/// let mut tgt = BTreeMap::new();
/// tgt.insert(Capability::Streaming, CoreSupportLevel::Native);
/// tgt.insert(Capability::Vision, CoreSupportLevel::Emulated);
///
/// let negotiator = CapabilityNegotiator::new("claude", src, "openai", tgt);
/// let result = negotiator.negotiate();
/// assert_eq!(result.native.len(), 1);
/// assert_eq!(result.degraded.len(), 1);
/// ```
pub struct CapabilityNegotiator {
    source_name: String,
    source_manifest: CapabilityManifest,
    target_name: String,
    target_manifest: CapabilityManifest,
}

impl CapabilityNegotiator {
    /// Create a new negotiator for the given source and target dialects.
    #[must_use]
    pub fn new(
        source_name: &str,
        source_manifest: CapabilityManifest,
        target_name: &str,
        target_manifest: CapabilityManifest,
    ) -> Self {
        Self {
            source_name: source_name.to_owned(),
            source_manifest,
            target_name: target_name.to_owned(),
            target_manifest,
        }
    }

    /// Perform negotiation, producing a [`DetailedNegotiationResult`].
    ///
    /// Classification rules:
    /// - **Native**: target supports at same or higher level (unchanged/upgrade).
    /// - **Degraded**: target supports at a lower level but not unsupported
    ///   (e.g. Native→Emulated or Native→Restricted).
    /// - **Emulatable**: target does not support but ABP can emulate.
    /// - **Unsupported**: target does not support and emulation is not available
    ///   (capability is lost).
    #[must_use]
    pub fn negotiate(&self) -> DetailedNegotiationResult {
        use crate::{classify_transition, default_emulation_strategy, TransitionKind};
        use abp_core::SupportLevel as CoreSupportLevel;

        let mut native = Vec::new();
        let mut emulatable = Vec::new();
        let unsupported = Vec::new();
        let mut degraded = Vec::new();

        for (cap, src_level) in &self.source_manifest {
            if matches!(src_level, CoreSupportLevel::Unsupported) {
                continue;
            }

            let tgt_level = self
                .target_manifest
                .get(cap)
                .cloned()
                .unwrap_or(CoreSupportLevel::Unsupported);

            let kind = classify_transition(src_level, &tgt_level);

            match kind {
                TransitionKind::Unchanged | TransitionKind::Upgrade => {
                    native.push(cap.clone());
                }
                TransitionKind::Downgrade => {
                    let src_label = level_label(src_level);
                    let tgt_label = level_label(&tgt_level);
                    degraded.push(DegradedFeature {
                        capability: cap.clone(),
                        source_level: src_label.clone(),
                        target_level: tgt_label.clone(),
                        caveats: format!(
                            "downgraded from {} to {}; may have reduced fidelity",
                            src_label, tgt_label
                        ),
                    });
                }
                TransitionKind::Lost => {
                    let strategy = default_emulation_strategy(cap);
                    // Approximate strategies are low-confidence so we flag as
                    // emulatable but still risky. All strategies at least allow
                    // an attempt.
                    emulatable.push((cap.clone(), strategy));
                }
            }
        }

        DetailedNegotiationResult {
            source: self.source_name.clone(),
            target: self.target_name.clone(),
            native,
            emulatable,
            unsupported,
            degraded,
        }
    }

    /// Negotiate and partition truly unsupported features that cannot be emulated.
    ///
    /// Features that are lost but have `Approximate` strategy are moved to
    /// unsupported since the fidelity is too low for reliable operation.
    #[must_use]
    pub fn negotiate_strict(&self) -> DetailedNegotiationResult {
        let mut result = self.negotiate();
        let mut still_emulatable = Vec::new();
        for (cap, strategy) in result.emulatable.drain(..) {
            if strategy == crate::EmulationStrategy::Approximate {
                result.unsupported.push((
                    cap,
                    "lost capability; approximate emulation insufficient".into(),
                ));
            } else {
                still_emulatable.push((cap, strategy));
            }
        }
        result.emulatable = still_emulatable;
        result
    }
}

/// Short label for a core support level.
fn level_label(level: &CoreSupportLevel) -> String {
    match level {
        CoreSupportLevel::Native => "native".to_owned(),
        CoreSupportLevel::Emulated => "emulated".to_owned(),
        CoreSupportLevel::Unsupported => "unsupported".to_owned(),
        CoreSupportLevel::Restricted { reason } => format!("restricted ({reason})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::SupportLevel as CoreSupportLevel;

    fn make_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
        entries.iter().cloned().collect()
    }

    #[test]
    fn pre_negotiate_all_native() {
        let m = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
        assert_eq!(r.native.len(), 2);
        assert!(r.emulated.is_empty());
        assert!(r.unsupported.is_empty());
        assert!(r.is_viable());
    }

    #[test]
    fn pre_negotiate_all_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert_eq!(r.unsupported.len(), 2);
        assert!(!r.is_viable());
    }

    #[test]
    fn pre_negotiate_mixed() {
        let m = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Emulated),
        ]);
        let r = pre_negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        assert_eq!(r.native.len(), 1);
        assert_eq!(r.emulated.len(), 1);
        assert_eq!(r.unsupported.len(), 1);
    }

    #[test]
    fn pre_negotiate_empty_required() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert!(r.unsupported.is_empty());
        assert!(r.is_viable());
    }

    #[test]
    fn pre_negotiate_empty_manifest() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[], &m);
        assert!(r.is_viable());
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn policy_strict_passes_all_native() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn policy_strict_fails_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        let err = apply_policy(&r, NegotiationPolicy::Strict).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::Strict);
        assert_eq!(err.unsupported.len(), 1);
    }

    #[test]
    fn policy_strict_passes_emulated() {
        let m = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let r = pre_negotiate(&[Capability::ToolUse], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn policy_best_effort_passes_all_native() {
        let m = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = pre_negotiate(&[Capability::Streaming], &m);
        assert!(apply_policy(&r, NegotiationPolicy::BestEffort).is_ok());
    }

    #[test]
    fn policy_best_effort_fails_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Vision], &m);
        let err = apply_policy(&r, NegotiationPolicy::BestEffort).unwrap_err();
        assert_eq!(err.policy, NegotiationPolicy::BestEffort);
        assert_eq!(err.unsupported.len(), 1);
    }

    #[test]
    fn policy_permissive_always_passes() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(&[Capability::Streaming, Capability::Vision], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn policy_permissive_with_all_unsupported() {
        let m = make_manifest(&[]);
        let r = pre_negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &m,
        );
        assert!(!r.is_viable());
        assert!(apply_policy(&r, NegotiationPolicy::Permissive).is_ok());
    }

    #[test]
    fn negotiation_error_display() {
        let err = NegotiationError {
            policy: NegotiationPolicy::Strict,
            unsupported: vec![(Capability::Vision, "not available".into())],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("strict"));
        assert!(msg.contains("1 unsupported"));
        assert!(msg.contains("Vision"));
    }

    #[test]
    fn negotiation_error_multiple_unsupported() {
        let err = NegotiationError {
            policy: NegotiationPolicy::BestEffort,
            unsupported: vec![
                (Capability::Vision, "not available".into()),
                (Capability::Audio, "not available".into()),
            ],
            warnings: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("2 unsupported"));
        assert!(msg.contains("Vision"));
        assert!(msg.contains("Audio"));
    }

    #[test]
    fn policy_default_is_strict() {
        assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
    }

    #[test]
    fn policy_display() {
        assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
        assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
        assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
    }

    #[test]
    fn pre_negotiate_restricted_classified_as_emulated() {
        let m = make_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        let r = pre_negotiate(&[Capability::ToolBash], &m);
        assert!(r.native.is_empty());
        assert_eq!(r.emulated.len(), 1);
        assert!(r.unsupported.is_empty());
    }

    #[test]
    fn pre_negotiate_explicit_unsupported_in_manifest() {
        let m = make_manifest(&[(Capability::Vision, CoreSupportLevel::Unsupported)]);
        let r = pre_negotiate(&[Capability::Vision], &m);
        assert!(r.native.is_empty());
        assert!(r.emulated.is_empty());
        assert_eq!(r.unsupported.len(), 1);
    }

    #[test]
    fn strict_policy_restricted_passes() {
        let m = make_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        let r = pre_negotiate(&[Capability::ToolBash], &m);
        assert!(apply_policy(&r, NegotiationPolicy::Strict).is_ok());
    }

    #[test]
    fn negotiation_error_is_std_error() {
        let err = NegotiationError {
            policy: NegotiationPolicy::Strict,
            unsupported: vec![(Capability::Streaming, "missing".into())],
            warnings: vec![],
        };
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn policy_serde_roundtrip() {
        let policy = NegotiationPolicy::BestEffort;
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }

    // ---- NegotiationReport -----------------------------------------------

    #[test]
    fn negotiate_report_all_met() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
        manifest.insert(Capability::ToolUse, CoreSupportLevel::Native);

        let report = negotiate(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert_eq!(report.met.len(), 2);
        assert!(report.emulated.is_empty());
        assert!(report.missing.is_empty());
        assert!(report.is_fully_met());
        assert!(!report.has_missing());
    }

    #[test]
    fn negotiate_report_partial() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
        manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);

        let report = negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &manifest,
        );
        assert_eq!(report.met.len(), 1);
        assert_eq!(report.emulated.len(), 1);
        assert_eq!(report.missing.len(), 1);
        assert!(!report.is_fully_met());
        assert!(report.has_missing());
    }

    #[test]
    fn negotiate_report_all_missing() {
        let manifest = CapabilityManifest::new();
        let report = negotiate(&[Capability::Streaming, Capability::Vision], &manifest);
        assert!(report.met.is_empty());
        assert!(report.emulated.is_empty());
        assert_eq!(report.missing.len(), 2);
        assert!(report.has_missing());
    }

    #[test]
    fn negotiate_report_empty_requirements() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
        let report = negotiate(&[], &manifest);
        assert!(report.is_fully_met());
        assert!(!report.has_missing());
        assert!(report.met.is_empty());
    }

    #[test]
    fn negotiate_report_display() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
        let report = negotiate(&[Capability::Streaming, Capability::Vision], &manifest);
        let s = format!("{report}");
        assert!(s.contains("1 met"));
        assert!(s.contains("1 missing"));
    }

    #[test]
    fn negotiate_report_serde_roundtrip() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
        manifest.insert(Capability::ToolUse, CoreSupportLevel::Emulated);
        let report = negotiate(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &manifest,
        );
        let json = serde_json::to_string(&report).unwrap();
        let back: NegotiationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.met.len(), 1);
        assert_eq!(back.emulated.len(), 1);
        assert_eq!(back.missing.len(), 1);
    }

    #[test]
    fn emulation_plan_entry_display() {
        let entry = EmulationPlanEntry {
            capability: Capability::ToolUse,
            strategy: EmulationStrategy::ServerFallback,
            detail: "via function calling".into(),
        };
        let s = format!("{entry}");
        assert!(s.contains("ToolUse"));
        assert!(s.contains("server fallback"));
    }

    #[test]
    fn negotiate_report_emulated_strategy_selection() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::ToolRead, CoreSupportLevel::Emulated);
        manifest.insert(Capability::Vision, CoreSupportLevel::Emulated);

        let report = negotiate(&[Capability::ToolRead, Capability::Vision], &manifest);
        assert_eq!(report.emulated.len(), 2);
        // Both should have emulation strategies
        for entry in &report.emulated {
            assert!(!entry.detail.is_empty());
        }
    }

    #[test]
    fn negotiate_report_restricted_classified_as_emulated() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        );

        let report = negotiate(&[Capability::ToolBash], &manifest);
        assert!(report.met.is_empty());
        assert_eq!(report.emulated.len(), 1);
        assert!(report.missing.is_empty());
    }

    #[test]
    fn negotiate_report_explicit_unsupported() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Vision, CoreSupportLevel::Unsupported);

        let report = negotiate(&[Capability::Vision], &manifest);
        assert!(report.met.is_empty());
        assert!(report.emulated.is_empty());
        assert_eq!(report.missing.len(), 1);
    }

    // ---- CapabilityNegotiator --------------------------------------------

    #[test]
    fn negotiator_same_manifests_all_native() {
        let m = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let n = CapabilityNegotiator::new("a", m.clone(), "b", m);
        let result = n.negotiate();
        assert_eq!(result.native.len(), 2);
        assert!(result.emulatable.is_empty());
        assert!(result.unsupported.is_empty());
        assert!(result.degraded.is_empty());
        assert!(result.is_viable());
    }

    #[test]
    fn negotiator_detects_degraded() {
        let src = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let tgt = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        assert!(result.native.is_empty());
        assert_eq!(result.degraded.len(), 1);
        assert_eq!(result.degraded[0].capability, Capability::Streaming);
        assert!(result.degraded[0].caveats.contains("downgraded"));
    }

    #[test]
    fn negotiator_detects_lost_as_emulatable() {
        let src = make_manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
        let tgt: CapabilityManifest = std::collections::BTreeMap::new();
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        assert!(result.native.is_empty());
        assert_eq!(result.emulatable.len(), 1);
        assert_eq!(result.emulatable[0].0, Capability::Vision);
        assert!(result.is_viable());
    }

    #[test]
    fn negotiator_detects_upgrade_as_native() {
        let src = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let tgt = make_manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        assert_eq!(result.native, vec![Capability::ToolUse]);
    }

    #[test]
    fn negotiator_skips_source_unsupported() {
        let src = make_manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
        let tgt = make_manifest(&[(Capability::Audio, CoreSupportLevel::Native)]);
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn negotiator_mixed_result() {
        let src = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
            (Capability::Audio, CoreSupportLevel::Native),
        ]);
        let tgt = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Emulated),
        ]);
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.degraded.len(), 1);
        assert_eq!(result.emulatable.len(), 1);
        assert!(result.is_viable());
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn negotiator_display() {
        let src = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let tgt = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let n = CapabilityNegotiator::new("claude", src, "openai", tgt);
        let result = n.negotiate();
        let s = format!("{result}");
        assert!(s.contains("claude"));
        assert!(s.contains("openai"));
        assert!(s.contains("degraded"));
    }

    #[test]
    fn negotiator_strict_moves_approximate_to_unsupported() {
        let src = make_manifest(&[(Capability::Vision, CoreSupportLevel::Native)]);
        let tgt: CapabilityManifest = std::collections::BTreeMap::new();
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate_strict();
        // Vision uses Approximate strategy, so strict moves it to unsupported
        assert!(result.emulatable.is_empty());
        assert_eq!(result.unsupported.len(), 1);
        assert!(!result.is_viable());
    }

    #[test]
    fn negotiator_strict_keeps_client_side_emulatable() {
        let src = make_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let tgt: CapabilityManifest = std::collections::BTreeMap::new();
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate_strict();
        // ToolRead uses ClientSide strategy, should stay emulatable
        assert_eq!(result.emulatable.len(), 1);
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn degraded_feature_display() {
        let d = DegradedFeature {
            capability: Capability::Streaming,
            source_level: "native".into(),
            target_level: "emulated".into(),
            caveats: "reduced throughput".into(),
        };
        let s = format!("{d}");
        assert!(s.contains("Streaming"));
        assert!(s.contains("native"));
        assert!(s.contains("emulated"));
        assert!(s.contains("reduced throughput"));
    }

    #[test]
    fn detailed_result_serde_roundtrip() {
        let src = make_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Vision, CoreSupportLevel::Native),
        ]);
        let tgt = make_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let n = CapabilityNegotiator::new("src", src, "tgt", tgt);
        let result = n.negotiate();
        let json = serde_json::to_string(&result).unwrap();
        let back: DetailedNegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, "src");
        assert_eq!(back.degraded.len(), result.degraded.len());
    }
}
