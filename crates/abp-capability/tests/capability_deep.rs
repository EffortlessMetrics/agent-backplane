#![allow(clippy::all)]
#![allow(clippy::useless_vec)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for abp-capability: CoreSupportLevel, satisfies(), negotiate(),
//! check_capability, NegotiationResult, CompatibilityReport, registry, and serde.

use abp_capability::{
    check_capability, default_emulation_strategy, generate_report, negotiate,
    negotiate_capabilities, CapabilityRegistry, CompatibilityReport, EmulationStrategy,
    NegotiationResult, SupportLevel,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
    require(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

// ===========================================================================
// 1. CoreSupportLevel::satisfies() — exhaustive matrix
// ===========================================================================

// --- Native (manifest) × MinSupport ---

#[test]
fn satisfies_native_meets_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_native_meets_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

// --- Emulated (manifest) × MinSupport ---

#[test]
fn satisfies_emulated_does_not_meet_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_emulated_meets_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

// --- Restricted (manifest) × MinSupport ---

#[test]
fn satisfies_restricted_does_not_meet_native() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_restricted_meets_emulated() {
    let restricted = CoreSupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

// --- Unsupported (manifest) × MinSupport ---

#[test]
fn satisfies_unsupported_does_not_meet_native() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn satisfies_unsupported_does_not_meet_emulated() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ===========================================================================
// 2. negotiate() — satisfies semantics integration
// ===========================================================================

#[test]
fn negotiate_native_manifest_native_min_passes() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_native_manifest_emulated_min_passes() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = require_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::Streaming]);
}

#[test]
fn negotiate_emulated_manifest_native_min_fails() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
}

#[test]
fn negotiate_emulated_manifest_emulated_min_passes() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = require_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated_caps(), vec![Capability::Streaming]);
}

#[test]
fn negotiate_restricted_manifest_native_min_fails() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    )]);
    let r = require_native(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolBash]);
}

#[test]
fn negotiate_restricted_manifest_emulated_min_passes() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    )]);
    let r = require_emulated(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
}

#[test]
fn negotiate_unsupported_manifest_native_min_fails() {
    let m = manifest_from(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let r = require_native(&[Capability::Audio]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn negotiate_unsupported_manifest_emulated_min_fails() {
    let m = manifest_from(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let r = require_emulated(&[Capability::Audio]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn negotiate_missing_from_manifest_native_min_fails() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::Vision]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Vision]);
}

#[test]
fn negotiate_missing_from_manifest_emulated_min_fails() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_emulated(&[Capability::Vision]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Vision]);
}

// ===========================================================================
// 3. negotiate() — mixed requirements with different MinSupport
// ===========================================================================

#[test]
fn negotiate_mixed_min_support_all_pass() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let r = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
    ]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
}

#[test]
fn negotiate_mixed_min_support_one_fails() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Emulated),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let r = require(&[
        (Capability::Streaming, MinSupport::Native), // fails: Emulated < Native
        (Capability::ToolRead, MinSupport::Emulated), // passes
    ]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
}

#[test]
fn negotiate_native_satisfies_emulated_min_via_negotiate() {
    // Native in manifest, Emulated min → should land in native bucket
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Native)]);
    let r = require_emulated(&[Capability::Vision]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::Vision]);
    assert!(res.emulated.is_empty());
}

// ===========================================================================
// 4. negotiate() — empty inputs
// ===========================================================================

#[test]
fn negotiate_empty_manifest_empty_requirements() {
    let res = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_empty_requirements_nonempty_manifest() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_empty_manifest_nonempty_requirements() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::Streaming, Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 2);
}

// ===========================================================================
// 5. negotiate() — duplicate requirements
// ===========================================================================

#[test]
fn negotiate_duplicate_requirements_both_counted() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = require_native(&[Capability::Streaming, Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_duplicate_requirements_different_min_support() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = require(&[
        (Capability::Streaming, MinSupport::Native),   // fails
        (Capability::Streaming, MinSupport::Emulated), // passes
    ]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 1);
    assert_eq!(res.emulated.len(), 1);
}

// ===========================================================================
// 6. negotiate() — order preservation
// ===========================================================================

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

// ===========================================================================
// 7. negotiate_capabilities (simple &[Capability] form)
// ===========================================================================

#[test]
fn negotiate_capabilities_all_native_simple() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Native),
    ]);
    let res = negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &m);
    assert_eq!(res.native.len(), 2);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_capabilities_restricted_goes_to_emulated() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let res = negotiate_capabilities(&[Capability::ToolBash], &m);
    assert!(res.is_compatible());
    assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
}

#[test]
fn negotiate_capabilities_missing_is_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let res = negotiate_capabilities(&[Capability::Audio], &m);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn negotiate_capabilities_explicit_unsupported_is_unsupported() {
    let m = manifest_from(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
    let res = negotiate_capabilities(&[Capability::Audio], &m);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Audio]);
}

#[test]
fn negotiate_capabilities_empty_required_is_compatible() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let res = negotiate_capabilities(&[], &m);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_capabilities_empty_manifest_empty_required() {
    let m: CapabilityManifest = BTreeMap::new();
    let res = negotiate_capabilities(&[], &m);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

// ===========================================================================
// 8. check_capability — all CoreSupportLevel variants
// ===========================================================================

#[test]
fn check_capability_native_returns_native() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_emulated_returns_emulated() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    assert!(matches!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Emulated { .. }
    ));
}

#[test]
fn check_capability_restricted_returns_restricted() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "policy".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    assert!(matches!(level, SupportLevel::Restricted { .. }));
    if let SupportLevel::Restricted { reason } = level {
        assert_eq!(reason, "policy");
    }
}

#[test]
fn check_capability_unsupported_explicit_returns_unsupported() {
    let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    let level = check_capability(&m, &Capability::Logprobs);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
    if let SupportLevel::Unsupported { reason } = level {
        assert!(reason.contains("explicitly"));
    }
}

#[test]
fn check_capability_missing_returns_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&m, &Capability::Vision);
    assert!(matches!(level, SupportLevel::Unsupported { .. }));
    if let SupportLevel::Unsupported { reason } = level {
        assert!(reason.contains("not declared"));
    }
}

// ===========================================================================
// 9. NegotiationResult — methods and accessors
// ===========================================================================

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
    assert!(!result.is_compatible());
}

#[test]
fn negotiation_result_total_counts_all_buckets() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming, Capability::ToolUse],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "N/A".into())],
    };
    assert_eq!(result.total(), 4);
}

#[test]
fn negotiation_result_total_zero_for_empty() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    assert_eq!(result.total(), 0);
}

#[test]
fn negotiation_result_warnings_returns_only_approximate() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![
            (Capability::ToolRead, EmulationStrategy::ClientSide),
            (
                Capability::FunctionCalling,
                EmulationStrategy::ServerFallback,
            ),
            (Capability::Vision, EmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let w = result.warnings();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_warnings_empty_when_no_approximate() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![],
    };
    assert!(result.warnings().is_empty());
}

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
fn negotiation_result_from_simple_constructs_correctly() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
    // from_simple uses ClientSide for emulated
    assert_eq!(result.emulated[0].1, EmulationStrategy::ClientSide);
    // from_simple uses "not available" for unsupported
    assert_eq!(result.unsupported[0].1, "not available");
    assert_eq!(result.total(), 3);
}

// ===========================================================================
// 10. NegotiationResult — Display
// ===========================================================================

#[test]
fn negotiation_result_display_viable() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let s = format!("{result}");
    assert!(s.contains("1 native"));
    assert!(s.contains("viable"));
    assert!(!s.contains("not viable"));
}

#[test]
fn negotiation_result_display_not_viable() {
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

// ===========================================================================
// 11. EmulationStrategy
// ===========================================================================

#[test]
fn emulation_strategy_display_client_side() {
    assert_eq!(
        format!("{}", EmulationStrategy::ClientSide),
        "client-side emulation"
    );
}

#[test]
fn emulation_strategy_display_server_fallback() {
    assert_eq!(
        format!("{}", EmulationStrategy::ServerFallback),
        "server fallback"
    );
}

#[test]
fn emulation_strategy_display_approximate() {
    assert_eq!(format!("{}", EmulationStrategy::Approximate), "approximate");
}

#[test]
fn emulation_strategy_fidelity_loss_only_approximate() {
    assert!(!EmulationStrategy::ClientSide.has_fidelity_loss());
    assert!(!EmulationStrategy::ServerFallback.has_fidelity_loss());
    assert!(EmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn emulation_strategy_serde_roundtrip() {
    for s in &[
        EmulationStrategy::ClientSide,
        EmulationStrategy::ServerFallback,
        EmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, s);
    }
}

#[test]
fn emulation_strategy_json_values() {
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::ClientSide).unwrap(),
        "\"client_side\""
    );
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::ServerFallback).unwrap(),
        "\"server_fallback\""
    );
    assert_eq!(
        serde_json::to_string(&EmulationStrategy::Approximate).unwrap(),
        "\"approximate\""
    );
}

// ===========================================================================
// 12. SupportLevel (abp-capability's own) — Display + serde
// ===========================================================================

#[test]
fn support_level_display_native() {
    assert_eq!(format!("{}", SupportLevel::Native), "native");
}

#[test]
fn support_level_display_emulated() {
    let level = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    assert_eq!(format!("{level}"), "emulated (polyfill)");
}

#[test]
fn support_level_display_restricted() {
    let level = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert_eq!(format!("{level}"), "restricted (sandbox)");
}

#[test]
fn support_level_display_unsupported() {
    let level = SupportLevel::Unsupported {
        reason: "N/A".into(),
    };
    assert_eq!(format!("{level}"), "unsupported (N/A)");
}

#[test]
fn support_level_serde_roundtrip_all_variants() {
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

// ===========================================================================
// 13. generate_report
// ===========================================================================

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
fn report_empty_result_is_compatible() {
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
fn report_details_length_matches_total() {
    let result = NegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![(Capability::ToolRead, EmulationStrategy::ClientSide)],
        unsupported: vec![(Capability::Logprobs, "N/A".into())],
    };
    let report = generate_report(&result);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn report_display_uses_summary() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    let display = format!("{report}");
    assert_eq!(display, report.summary);
}

#[test]
fn report_serde_roundtrip() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

// ===========================================================================
// 14. default_emulation_strategy — category checks
// ===========================================================================

#[test]
fn default_emulation_strategy_client_side_caps() {
    let client_side_caps = [
        Capability::StructuredOutputJsonSchema,
        Capability::JsonMode,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::Checkpointing,
    ];
    for cap in &client_side_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ClientSide,
            "expected ClientSide for {cap:?}"
        );
    }
}

#[test]
fn default_emulation_strategy_server_fallback_caps() {
    let server_caps = [
        Capability::FunctionCalling,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::BatchMode,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::McpClient,
        Capability::McpServer,
        Capability::SystemMessage,
    ];
    for cap in &server_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::ServerFallback,
            "expected ServerFallback for {cap:?}"
        );
    }
}

#[test]
fn default_emulation_strategy_approximate_caps() {
    let approx_caps = [
        Capability::Vision,
        Capability::ImageInput,
        Capability::Audio,
        Capability::ImageGeneration,
        Capability::Embeddings,
        Capability::CacheControl,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::Streaming,
        Capability::StopSequences,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
    ];
    for cap in &approx_caps {
        assert_eq!(
            default_emulation_strategy(cap),
            EmulationStrategy::Approximate,
            "expected Approximate for {cap:?}"
        );
    }
}

// ===========================================================================
// 15. negotiate_capabilities — emulation strategy selection
// ===========================================================================

#[test]
fn negotiate_emulated_vision_gets_approximate_strategy() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let res = negotiate_capabilities(&[Capability::Vision], &m);
    assert_eq!(res.emulated.len(), 1);
    assert_eq!(res.emulated[0].1, EmulationStrategy::Approximate);
}

#[test]
fn negotiate_emulated_tool_read_gets_client_side_strategy() {
    let m = manifest_from(&[(Capability::ToolRead, CoreSupportLevel::Emulated)]);
    let res = negotiate_capabilities(&[Capability::ToolRead], &m);
    assert_eq!(res.emulated.len(), 1);
    assert_eq!(res.emulated[0].1, EmulationStrategy::ClientSide);
}

#[test]
fn negotiate_emulated_function_calling_gets_server_fallback_strategy() {
    let m = manifest_from(&[(Capability::FunctionCalling, CoreSupportLevel::Emulated)]);
    let res = negotiate_capabilities(&[Capability::FunctionCalling], &m);
    assert_eq!(res.emulated.len(), 1);
    assert_eq!(res.emulated[0].1, EmulationStrategy::ServerFallback);
}

// ===========================================================================
// 16. Bulk negotiation with many capabilities
// ===========================================================================

#[test]
fn bulk_negotiation_large_manifest() {
    let m = manifest_from(&[
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
        (Capability::Temperature, CoreSupportLevel::Native),
        (Capability::TopP, CoreSupportLevel::Native),
        (Capability::MaxTokens, CoreSupportLevel::Native),
        (Capability::Logprobs, CoreSupportLevel::Native),
        (Capability::ExtendedThinking, CoreSupportLevel::Unsupported),
    ]);
    let required = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
        Capability::JsonMode,
        Capability::Temperature,
        Capability::TopP,
        Capability::MaxTokens,
        Capability::Logprobs,
        Capability::ExtendedThinking,
    ];
    let res = negotiate_capabilities(&required, &m);
    assert_eq!(res.native.len(), 11);
    assert_eq!(res.emulated.len(), 1); // CodeExecution
    assert_eq!(res.unsupported.len(), 1); // ExtendedThinking
    assert!(!res.is_compatible());
    assert_eq!(res.total(), 13);
}

#[test]
fn bulk_negotiate_with_requirements_all_emulated_min() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
    ]);
    let r = require_emulated(&[
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Vision,
        Capability::Audio,
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert_eq!(res.emulated.len(), 2);
    assert_eq!(res.unsupported.len(), 1);
    assert!(!res.is_compatible());
}

// ===========================================================================
// 17. CapabilityRegistry
// ===========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.names().is_empty());
}

#[test]
fn registry_register_and_lookup() {
    let mut reg = CapabilityRegistry::new();
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    reg.register("test", m);
    assert!(reg.contains("test"));
    assert!(!reg.contains("missing"));
    assert!(reg.get("test").is_some());
    assert!(reg.get("missing").is_none());
    assert_eq!(reg.len(), 1);
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
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_names_sorted() {
    let mut reg = CapabilityRegistry::new();
    reg.register("zz", BTreeMap::new());
    reg.register("aa", BTreeMap::new());
    reg.register("mm", BTreeMap::new());
    // BTreeMap ensures sorted order
    assert_eq!(reg.names(), vec!["aa", "mm", "zz"]);
}

#[test]
fn registry_negotiate_by_name_found() {
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
    assert!(reg
        .negotiate_by_name("nope", &[Capability::Streaming])
        .is_none());
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
fn registry_query_capability_streaming_all_native() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 6);
    assert!(results
        .iter()
        .all(|(_, level)| matches!(level, SupportLevel::Native)));
}

#[test]
fn registry_compare_same_backend_is_viable() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/gpt-4o", "openai/gpt-4o").unwrap();
    assert!(result.is_viable());
}

#[test]
fn registry_compare_missing_source_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("nonexistent", "openai/gpt-4o").is_none());
}

#[test]
fn registry_compare_missing_target_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "nonexistent").is_none());
}

#[test]
fn registry_compare_claude_to_openai_shows_extended_thinking_gap() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
        .unwrap();
    assert!(result
        .unsupported_caps()
        .contains(&Capability::ExtendedThinking));
}

// ===========================================================================
// 18. NegotiationResult serde roundtrip
// ===========================================================================

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
fn negotiation_result_serde_empty() {
    let result = NegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, result);
}

// ===========================================================================
// 19. CapabilityRequirements serde roundtrip
// ===========================================================================

#[test]
fn capability_requirements_serde_roundtrip() {
    let reqs = require(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
    ]);
    let json = serde_json::to_string(&reqs).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

#[test]
fn min_support_serde_values() {
    let native_json = serde_json::to_string(&MinSupport::Native).unwrap();
    assert_eq!(native_json, "\"native\"");
    let emulated_json = serde_json::to_string(&MinSupport::Emulated).unwrap();
    assert_eq!(emulated_json, "\"emulated\"");
}

#[test]
fn core_support_level_serde_roundtrip() {
    let levels = vec![
        CoreSupportLevel::Native,
        CoreSupportLevel::Emulated,
        CoreSupportLevel::Unsupported,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: CoreSupportLevel = serde_json::from_str(&json).unwrap();
        // We can compare via debug strings since CoreSupportLevel may not derive PartialEq
        assert_eq!(format!("{back:?}"), format!("{level:?}"));
    }
}

// ===========================================================================
// 20. Edge cases and stress
// ===========================================================================

#[test]
fn negotiate_restricted_with_empty_reason() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: String::new(),
        },
    )]);
    let r = require_emulated(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
}

#[test]
fn negotiate_many_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let caps: Vec<Capability> = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Vision,
        Capability::Audio,
        Capability::CodeExecution,
        Capability::FunctionCalling,
        Capability::JsonMode,
        Capability::Temperature,
        Capability::TopP,
        Capability::MaxTokens,
    ];
    let r = require_native(&caps);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 10);
    assert_eq!(res.total(), 10);
}

#[test]
fn negotiate_all_categories_represented() {
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::Audio, CoreSupportLevel::Unsupported),
    ]);
    let r = require_emulated(&[Capability::Streaming, Capability::Vision, Capability::Audio]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 1);
    assert_eq!(res.emulated.len(), 1);
    assert_eq!(res.unsupported.len(), 1);
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_capabilities_preserves_order() {
    let m = manifest_from(&[
        (Capability::Audio, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Unsupported),
    ]);
    let res = negotiate_capabilities(
        &[Capability::Audio, Capability::Vision, Capability::Streaming],
        &m,
    );
    assert_eq!(res.native, vec![Capability::Vision]);
    assert_eq!(res.emulated_caps(), vec![Capability::Audio]);
    assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
}

#[test]
fn negotiate_unsupported_reason_contains_info() {
    let m = manifest_from(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    let res = negotiate_capabilities(&[Capability::Logprobs], &m);
    let reason = &res.unsupported[0].1;
    assert!(!reason.is_empty());
}

#[test]
fn negotiate_missing_cap_reason_contains_not_declared() {
    let m: CapabilityManifest = BTreeMap::new();
    let res = negotiate_capabilities(&[Capability::Logprobs], &m);
    let reason = &res.unsupported[0].1;
    assert!(reason.contains("not declared"));
}

// ===========================================================================
// 21. Cross-model real-world negotiation scenarios
// ===========================================================================

#[test]
fn cross_model_all_backends_support_streaming_natively() {
    use abp_capability::*;
    let manifests = [
        openai_gpt4o_manifest(),
        claude_35_sonnet_manifest(),
        gemini_15_pro_manifest(),
        kimi_manifest(),
        codex_manifest(),
        copilot_manifest(),
    ];
    for m in &manifests {
        let level = check_capability(m, &Capability::Streaming);
        assert_eq!(level, SupportLevel::Native);
    }
}

#[test]
fn cross_model_extended_thinking_only_claude() {
    use abp_capability::*;
    let claude = claude_35_sonnet_manifest();
    assert_eq!(
        check_capability(&claude, &Capability::ExtendedThinking),
        SupportLevel::Native
    );

    let openai = openai_gpt4o_manifest();
    assert!(matches!(
        check_capability(&openai, &Capability::ExtendedThinking),
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn cross_model_tool_bash_codex_and_copilot() {
    use abp_capability::*;
    let codex = codex_manifest();
    let copilot = copilot_manifest();
    assert_eq!(
        check_capability(&codex, &Capability::ToolBash),
        SupportLevel::Native
    );
    assert_eq!(
        check_capability(&copilot, &Capability::ToolBash),
        SupportLevel::Native
    );
}

// ===========================================================================
// 22. negotiate() — satisfies boundary: Emulated does NOT satisfy Native min
// ===========================================================================

#[test]
fn satisfies_boundary_emulated_never_satisfies_native_min() {
    // This is the #1 gotcha: Emulated manifest level does NOT satisfy Native min_support
    let m = manifest_from(&[
        (Capability::Streaming, CoreSupportLevel::Emulated),
        (Capability::Vision, CoreSupportLevel::Emulated),
        (Capability::ToolUse, CoreSupportLevel::Emulated),
    ]);
    let r = require_native(&[
        Capability::Streaming,
        Capability::Vision,
        Capability::ToolUse,
    ]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 3);
    assert!(res.native.is_empty());
    assert!(res.emulated.is_empty());
}

#[test]
fn satisfies_boundary_restricted_never_satisfies_native_min() {
    let m = manifest_from(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = require_native(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 1);
}

#[test]
fn satisfies_boundary_native_always_satisfies_both() {
    let m = manifest_from(&[(Capability::Streaming, CoreSupportLevel::Native)]);

    let r_native = require_native(&[Capability::Streaming]);
    let res_native = negotiate(&m, &r_native);
    assert!(res_native.is_compatible());
    assert_eq!(res_native.native.len(), 1);

    let r_emulated = require_emulated(&[Capability::Streaming]);
    let res_emulated = negotiate(&m, &r_emulated);
    assert!(res_emulated.is_compatible());
    assert_eq!(res_emulated.native.len(), 1);
}

// ===========================================================================
// 23. negotiate() — unsupported reason message content
// ===========================================================================

#[test]
fn negotiate_unsupported_reason_mentions_support_levels() {
    let m = manifest_from(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
    let r = require_native(&[Capability::Vision]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported.len(), 1);
    let reason = &res.unsupported[0].1;
    // The reason should mention the actual and required support levels
    assert!(reason.contains("Emulated"));
    assert!(reason.contains("Native"));
}

// ===========================================================================
// 24. CompatibilityReport — summary content
// ===========================================================================

#[test]
fn report_summary_contains_counts() {
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
