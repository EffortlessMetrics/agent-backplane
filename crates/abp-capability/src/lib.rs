// SPDX-License-Identifier: MIT OR Apache-2.0
#![warn(missing_docs)]
//! Capability negotiation between work-order requirements and backend manifests.
//!
//! This crate provides functions to compare a [`CapabilityManifest`] against
//! [`CapabilityRequirements`] and produce structured negotiation results,
//! per-capability support checks, and human-readable compatibility reports.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirements, SupportLevel as CoreSupportLevel,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a single capability would be fulfilled after negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum SupportLevel {
    /// The backend supports this capability natively.
    Native,
    /// The capability can be provided through an adapter/polyfill.
    Emulated {
        /// Human-readable description of the emulation strategy.
        strategy: String,
    },
    /// The capability is not available.
    Unsupported,
}

/// Outcome of negotiating a full set of requirements against a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationResult {
    /// Capabilities the manifest supports natively.
    pub native: Vec<Capability>,
    /// Capabilities the manifest can provide via emulation.
    pub emulatable: Vec<Capability>,
    /// Capabilities the manifest cannot provide.
    pub unsupported: Vec<Capability>,
}

impl NegotiationResult {
    /// Returns `true` when every required capability is native or emulatable.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.unsupported.is_empty()
    }

    /// Total number of capabilities evaluated.
    #[must_use]
    pub fn total(&self) -> usize {
        self.native.len() + self.emulatable.len() + self.unsupported.len()
    }
}

/// Human-readable summary of a negotiation outcome.
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
// Core API
// ---------------------------------------------------------------------------

/// Classify a single capability against a manifest.
///
/// Returns [`SupportLevel::Native`] for `CoreSupportLevel::Native`,
/// [`SupportLevel::Emulated`] for `CoreSupportLevel::Emulated` and
/// `CoreSupportLevel::Restricted`, and [`SupportLevel::Unsupported`]
/// for everything else (including capabilities absent from the manifest).
#[must_use]
pub fn check_capability(manifest: &CapabilityManifest, cap: &Capability) -> SupportLevel {
    match manifest.get(cap) {
        Some(CoreSupportLevel::Native) => SupportLevel::Native,
        Some(CoreSupportLevel::Emulated) => SupportLevel::Emulated {
            strategy: "adapter".into(),
        },
        Some(CoreSupportLevel::Restricted { reason }) => SupportLevel::Emulated {
            strategy: format!("restricted: {reason}"),
        },
        Some(CoreSupportLevel::Unsupported) | None => SupportLevel::Unsupported,
    }
}

/// Negotiate all required capabilities against a manifest.
///
/// Each required capability is classified via [`check_capability`] and placed
/// into the appropriate bucket of the returned [`NegotiationResult`].
#[must_use]
pub fn negotiate(
    manifest: &CapabilityManifest,
    requirements: &CapabilityRequirements,
) -> NegotiationResult {
    let mut native = Vec::new();
    let mut emulatable = Vec::new();
    let mut unsupported = Vec::new();

    for req in &requirements.required {
        match check_capability(manifest, &req.capability) {
            SupportLevel::Native => native.push(req.capability.clone()),
            SupportLevel::Emulated { .. } => emulatable.push(req.capability.clone()),
            SupportLevel::Unsupported => unsupported.push(req.capability.clone()),
        }
    }

    NegotiationResult {
        native,
        emulatable,
        unsupported,
    }
}

/// Produce a human-readable [`CompatibilityReport`] from a negotiation result.
#[must_use]
pub fn generate_report(result: &NegotiationResult) -> CompatibilityReport {
    let compatible = result.is_compatible();

    let mut details: Vec<(String, SupportLevel)> = Vec::new();
    for cap in &result.native {
        details.push((format!("{cap:?}"), SupportLevel::Native));
    }
    for cap in &result.emulatable {
        details.push((
            format!("{cap:?}"),
            SupportLevel::Emulated {
                strategy: "adapter".into(),
            },
        ));
    }
    for cap in &result.unsupported {
        details.push((format!("{cap:?}"), SupportLevel::Unsupported));
    }

    let verdict = if compatible {
        "fully compatible"
    } else {
        "incompatible"
    };

    let summary = format!(
        "{} native, {} emulatable, {} unsupported — {verdict}",
        result.native.len(),
        result.emulatable.len(),
        result.unsupported.len(),
    );

    CompatibilityReport {
        compatible,
        native_count: result.native.len(),
        emulated_count: result.emulatable.len(),
        unsupported_count: result.unsupported.len(),
        summary,
        details,
    }
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

    // ---- negotiate: full-coverage -----------------------------------------

    #[test]
    fn negotiate_all_native() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native.len(), 2);
        assert!(res.emulatable.is_empty());
        assert!(res.unsupported.is_empty());
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_all_emulatable() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.native.is_empty());
        assert_eq!(res.emulatable.len(), 2);
        assert!(res.unsupported.is_empty());
        assert!(res.is_compatible());
    }

    #[test]
    fn negotiate_all_unsupported() {
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(res.native.is_empty());
        assert!(res.emulatable.is_empty());
        assert_eq!(res.unsupported.len(), 2);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_mixed() {
        let m = manifest_from(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            // ToolWrite not in manifest → unsupported
        ]);
        let r = require_native(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert_eq!(res.emulatable, vec![Capability::ToolRead]);
        assert_eq!(res.unsupported, vec![Capability::ToolWrite]);
        assert!(!res.is_compatible());
    }

    #[test]
    fn negotiate_some_emulatable_some_unsupported() {
        let m = manifest_from(&[
            (Capability::ToolBash, CoreSupportLevel::Emulated),
            (Capability::ToolEdit, CoreSupportLevel::Unsupported),
        ]);
        let r = require_native(&[Capability::ToolBash, Capability::ToolEdit]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulatable, vec![Capability::ToolBash]);
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
        assert!(res.emulatable.is_empty());
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
    fn negotiate_restricted_treated_as_emulatable() {
        let m = manifest_from(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let r = require_native(&[Capability::ToolBash]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulatable, vec![Capability::ToolBash]);
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
        // duplicates are kept — caller is responsible for dedup
        assert_eq!(res.native.len(), 2);
    }

    #[test]
    fn negotiate_duplicate_mixed_outcomes() {
        // Same capability appears twice but manifest says Unsupported for it,
        // so both land in unsupported.
        let m: CapabilityManifest = BTreeMap::new();
        let r = require_native(&[Capability::Logprobs, Capability::Logprobs]);
        let res = negotiate(&m, &r);
        assert_eq!(res.unsupported.len(), 2);
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
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Emulated {
                strategy: "adapter".into()
            }
        );
    }

    #[test]
    fn check_capability_unsupported_explicit() {
        let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
        assert_eq!(
            check_capability(&m, &Capability::Logprobs),
            SupportLevel::Unsupported
        );
    }

    #[test]
    fn check_capability_missing() {
        let m: CapabilityManifest = BTreeMap::new();
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Unsupported
        );
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
        assert!(matches!(level, SupportLevel::Emulated { .. }));
        if let SupportLevel::Emulated { strategy } = level {
            assert!(strategy.contains("restricted"));
            assert!(strategy.contains("policy"));
        }
    }

    #[test]
    fn check_capability_all_variants() {
        // Ensure every CoreSupportLevel maps to something sensible
        let cases: Vec<(CoreSupportLevel, bool)> = vec![
            (CoreSupportLevel::Native, true),
            (CoreSupportLevel::Emulated, true),
            (CoreSupportLevel::Unsupported, false),
            (CoreSupportLevel::Restricted { reason: "x".into() }, true),
        ];
        for (core_level, should_satisfy) in cases {
            let m = manifest_from(&[(Capability::Streaming, core_level)]);
            let level = check_capability(&m, &Capability::Streaming);
            let satisfied = !matches!(level, SupportLevel::Unsupported);
            assert_eq!(satisfied, should_satisfy);
        }
    }

    // ---- generate_report --------------------------------------------------

    #[test]
    fn report_fully_compatible() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolRead],
            emulatable: vec![Capability::ToolWrite],
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
            emulatable: vec![],
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
            emulatable: vec![],
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
            emulatable: vec![Capability::ToolRead, Capability::ToolWrite],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&result);
        assert_eq!(report.native_count, result.native.len());
        assert_eq!(report.emulated_count, result.emulatable.len());
        assert_eq!(report.unsupported_count, result.unsupported.len());
    }

    #[test]
    fn report_details_length() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulatable: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        let report = generate_report(&result);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_summary_counts() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming, Capability::ToolUse],
            emulatable: vec![Capability::ToolBash],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        assert!(report.summary.contains("2 native"));
        assert!(report.summary.contains("1 emulatable"));
        assert!(report.summary.contains("0 unsupported"));
    }

    #[test]
    fn report_all_emulated_still_compatible() {
        let result = NegotiationResult {
            native: vec![],
            emulatable: vec![Capability::Streaming, Capability::ToolRead],
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
            emulatable: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        assert_eq!(result.total(), 3);
    }

    #[test]
    fn negotiation_result_is_compatible_true() {
        let result = NegotiationResult {
            native: vec![Capability::Streaming],
            emulatable: vec![],
            unsupported: vec![],
        };
        assert!(result.is_compatible());
    }

    #[test]
    fn negotiation_result_is_compatible_false() {
        let result = NegotiationResult {
            native: vec![],
            emulatable: vec![],
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
        assert_eq!(res.emulatable, vec![Capability::ToolUse]);
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
                strategy: "polyfill".into(),
            },
            SupportLevel::Unsupported,
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
            emulatable: vec![Capability::ToolRead],
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
            emulatable: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&result);
        let json = serde_json::to_string(&report).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }
}
