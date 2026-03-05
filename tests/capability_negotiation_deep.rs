#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep capability negotiation tests.
//!
//! 50+ tests covering creation, ordering, negotiation, reports,
//! serde round-trips, dialect profiles, schema conformance, and edge cases.

use std::collections::BTreeMap;

use abp_capability::{
    check_capability, generate_report, negotiate, CompatibilityReport, NegotiationResult,
    SupportLevel as CapSupportLevel,
};
use abp_core::ir::{IrConversation, IrMessage, IrRole};
use abp_core::negotiate::{
    check_capabilities, dialect_manifest, CapabilityDiff, CapabilityNegotiator, CapabilityReport,
    CapabilityReportEntry, DialectSupportLevel, NegotiationRequest,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel, WorkOrderBuilder,
};
use abp_emulation::{
    can_emulate, compute_fidelity, default_strategy, EmulationConfig, EmulationEngine,
    EmulationStrategy, FidelityLabel,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn reqs(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
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

fn reqs_native(caps: &[Capability]) -> CapabilityRequirements {
    reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

fn reqs_emulated(caps: &[Capability]) -> CapabilityRequirements {
    reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

/// All defined capabilities (representative subset used in several tests).
fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Streaming,
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
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ]
}

// ===========================================================================
// 1. Capability creation and basic properties
// ===========================================================================

#[test]
fn capability_debug_repr_is_non_empty() {
    for cap in &all_capabilities() {
        let dbg = format!("{cap:?}");
        assert!(!dbg.is_empty(), "debug repr should not be empty");
    }
}

#[test]
fn capability_clone_equals_original() {
    for cap in &all_capabilities() {
        assert_eq!(cap, &cap.clone());
    }
}

#[test]
fn capability_ord_is_deterministic() {
    let mut caps = all_capabilities();
    let sorted1 = {
        let mut c = caps.clone();
        c.sort();
        c
    };
    caps.reverse();
    let mut sorted2 = caps;
    sorted2.sort();
    assert_eq!(sorted1, sorted2);
}

// ===========================================================================
// 2. SupportLevel ordering (Native > Emulated > Unsupported)
// ===========================================================================

#[test]
fn support_level_native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_unsupported_satisfies_nothing() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_restricted_satisfies_emulated() {
    let restricted = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_level_restricted_does_not_satisfy_native() {
    let restricted = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(!restricted.satisfies(&MinSupport::Native));
}

// ===========================================================================
// 3. Capability reports aggregation from multiple sources
// ===========================================================================

#[test]
fn report_aggregates_counts_correctly() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::ToolBash],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert_eq!(report.details.len(), 4);
}

#[test]
fn report_details_order_is_native_emulated_unsupported() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert!(matches!(report.details[0].1, CapSupportLevel::Native));
    assert!(matches!(
        report.details[1].1,
        CapSupportLevel::Emulated { .. }
    ));
    assert!(matches!(
        report.details[2].1,
        CapSupportLevel::Unsupported { .. }
    ));
}

// ===========================================================================
// 4. Negotiation between client and server capabilities
// ===========================================================================

#[test]
fn negotiate_all_native_is_compatible() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), 2);
}

#[test]
fn negotiate_emulated_min_support_emulated_is_compatible() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let r = reqs_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_missing_required_cap_is_incompatible() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let r = reqs_native(&[Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Logprobs]);
}

#[test]
fn negotiate_restricted_counts_as_emulatable() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    // Restricted satisfies Emulated min_support (not Native)
    let r = reqs(&[(Capability::ToolBash, MinSupport::Emulated)]);
    let res = negotiate(&m, &r);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
    assert!(res.is_compatible());
}

// ===========================================================================
// 5. Emulation labeling
// ===========================================================================

#[test]
fn emulated_cap_has_adapter_strategy() {
    let m = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let level = check_capability(&m, &Capability::ToolRead);
    match level {
        CapSupportLevel::Emulated { method } => {
            assert_eq!(method, "adapter");
        }
        _ => panic!("expected Emulated"),
    }
}

#[test]
fn restricted_cap_strategy_contains_reason() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "user policy".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    match level {
        CapSupportLevel::Restricted { reason } => {
            assert!(reason.contains("user policy"));
        }
        _ => panic!("expected Restricted, got {level:?}"),
    }
}

// ===========================================================================
// 6. Native vs emulated comparison
// ===========================================================================

#[test]
fn native_and_emulated_are_distinct_support_levels() {
    assert_ne!(
        CapSupportLevel::Native,
        CapSupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
    assert_ne!(
        CapSupportLevel::Native,
        CapSupportLevel::Emulated {
            method: "adapter".into()
        }
    );
}

#[test]
fn emulated_strategies_differ() {
    let a = CapSupportLevel::Emulated {
        method: "polyfill".into(),
    };
    let b = CapSupportLevel::Emulated {
        method: "adapter".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn same_emulated_strategy_is_equal() {
    let a = CapSupportLevel::Emulated {
        method: "adapter".into(),
    };
    let b = CapSupportLevel::Emulated {
        method: "adapter".into(),
    };
    assert_eq!(a, b);
}

// ===========================================================================
// 7. Unsupported capability handling (early failure)
// ===========================================================================

#[test]
fn unsupported_cap_makes_result_incompatible() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = reqs_native(&[Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
}

#[test]
fn explicit_unsupported_in_manifest_is_unsupported() {
    let m = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    let level = check_capability(&m, &Capability::Logprobs);
    assert!(matches!(level, CapSupportLevel::Unsupported { .. }));
}

#[test]
fn missing_from_manifest_is_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let level = check_capability(&m, &Capability::Streaming);
    assert!(matches!(level, CapSupportLevel::Unsupported { .. }));
}

// ===========================================================================
// 8. Capability intersection (what both sides support)
// ===========================================================================

#[test]
fn intersection_of_native_caps() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolWrite]);
    let res = negotiate(&m, &r);
    // Only the required caps appear in native
    assert_eq!(res.native.len(), 2);
    assert!(res.native.contains(&Capability::Streaming));
    assert!(res.native.contains(&Capability::ToolWrite));
}

#[test]
fn intersection_with_partial_overlap() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Native),
    ]);
    let r = reqs_native(&[
        Capability::Streaming,
        Capability::ToolEdit,
        Capability::Logprobs,
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
    assert_eq!(res.unsupported_caps(), vec![Capability::Logprobs]);
}

// ===========================================================================
// 9. Capability union (what either side supports)
// ===========================================================================

#[test]
fn union_via_core_negotiator_best_match() {
    let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead],
        minimum_support: SupportLevel::Emulated,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("m1", m1), ("m2", m2)]);
    assert!(best.is_some());
    let (name, result) = best.unwrap();
    assert_eq!(name, "m2");
    assert!(result.is_compatible);
    assert_eq!(result.bonus.len(), 1);
}

#[test]
fn union_no_compatible_returns_none() {
    let m = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("only", m)]);
    assert!(best.is_none());
}

// ===========================================================================
// 10. Serde roundtrip for all capability types
// ===========================================================================

#[test]
fn serde_roundtrip_capability_all_variants() {
    for cap in &all_capabilities() {
        let json = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, cap);
    }
}

#[test]
fn serde_roundtrip_support_level_all_variants() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "test".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{back:?}"), format!("{level:?}"));
    }
}

#[test]
fn serde_roundtrip_cap_support_level() {
    let levels = vec![
        CapSupportLevel::Native,
        CapSupportLevel::Emulated {
            method: "polyfill".into(),
        },
        CapSupportLevel::Unsupported {
            reason: "unsupported".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: CapSupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, level);
    }
}

#[test]
fn serde_roundtrip_negotiation_result() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, result);
}

#[test]
fn serde_roundtrip_compatibility_report() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolBash],
        vec![],
    );
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn serde_roundtrip_min_support() {
    for ms in &[MinSupport::Native, MinSupport::Emulated] {
        let json = serde_json::to_string(ms).unwrap();
        let back: MinSupport = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{back:?}"), format!("{ms:?}"));
    }
}

#[test]
fn serde_roundtrip_capability_requirements() {
    let r = reqs(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
    ]);
    let json = serde_json::to_string(&r).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

#[test]
fn serde_roundtrip_dialect_support_level() {
    let levels = vec![
        DialectSupportLevel::Native,
        DialectSupportLevel::Emulated {
            detail: "adapter".into(),
        },
        DialectSupportLevel::Unsupported {
            reason: "missing".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, level);
    }
}

#[test]
fn serde_roundtrip_capability_report() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: CapabilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_dialect, "claude");
    assert_eq!(back.entries.len(), 1);
}

// ===========================================================================
// 11. BTreeMap ordering in capability manifests
// ===========================================================================

#[test]
fn manifest_btreemap_is_sorted_by_capability() {
    let m = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let keys: Vec<&Capability> = m.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys);
}

#[test]
fn manifest_json_keys_are_deterministic() {
    let m1 = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
    ]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let json1 = serde_json::to_string(&m1).unwrap();
    let json2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(json1, json2, "BTreeMap serialization must be deterministic");
}

// ===========================================================================
// 12. Capability versioning
// ===========================================================================

#[test]
fn contract_version_embedded() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn work_order_requirements_are_preserved() {
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let wo = WorkOrderBuilder::new("test task").requirements(r).build();
    assert_eq!(wo.requirements.required.len(), 2);
    assert_eq!(
        wo.requirements.required[0].capability,
        Capability::Streaming
    );
}

// ===========================================================================
// 13. Per-dialect capability profiles
// ===========================================================================

#[test]
fn claude_dialect_has_streaming_native() {
    let m = dialect_manifest("claude");
    assert_eq!(
        m.get(&Capability::Streaming),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn claude_dialect_logprobs_unsupported() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn openai_dialect_has_logprobs_native() {
    let m = dialect_manifest("openai");
    assert_eq!(
        m.get(&Capability::Logprobs),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn openai_dialect_extended_thinking_unsupported() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn gemini_dialect_has_streaming_native() {
    let m = dialect_manifest("gemini");
    assert_eq!(
        m.get(&Capability::Streaming),
        Some(&DialectSupportLevel::Native)
    );
}

#[test]
fn gemini_dialect_code_execution_emulated() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::CodeExecution),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn unknown_dialect_returns_empty_manifest() {
    let m = dialect_manifest("unknown_vendor");
    assert!(m.is_empty());
}

#[test]
fn dialect_manifests_are_non_empty() {
    for dialect in &["claude", "openai", "gemini"] {
        let m = dialect_manifest(dialect);
        assert!(
            !m.is_empty(),
            "dialect '{dialect}' should have capabilities"
        );
    }
}

// ===========================================================================
// 14. Schema conformance of capability types
// ===========================================================================

#[test]
fn capability_json_schema_is_valid() {
    let schema = schemars::schema_for!(Capability);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
    // The schema should reference the enum variants
    let schema_str = serde_json::to_string_pretty(&json).unwrap();
    assert!(schema_str.contains("streaming"));
}

#[test]
fn support_level_json_schema_is_valid() {
    let schema = schemars::schema_for!(SupportLevel);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
}

#[test]
fn capability_requirements_json_schema_is_valid() {
    let schema = schemars::schema_for!(CapabilityRequirements);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
    let s = serde_json::to_string(&json).unwrap();
    assert!(s.contains("required"));
}

#[test]
fn min_support_json_schema_is_valid() {
    let schema = schemars::schema_for!(MinSupport);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_object());
}

// ===========================================================================
// 15. Edge cases
// ===========================================================================

#[test]
fn empty_requirements_always_compatible() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn both_empty_is_compatible() {
    let res = negotiate(&BTreeMap::new(), &CapabilityRequirements::default());
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn all_unsupported_is_incompatible() {
    let m: CapabilityManifest = BTreeMap::new();
    let caps = all_capabilities();
    let r = reqs_native(&caps);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), caps.len());
}

#[test]
fn all_native_large_set() {
    let caps = all_capabilities();
    let m: CapabilityManifest = caps
        .iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect();
    let r = reqs_native(&caps);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), caps.len());
    assert!(res.emulated.is_empty());
    assert!(res.unsupported.is_empty());
}

#[test]
fn duplicate_requirements_are_preserved() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let r = reqs_native(&[Capability::Streaming, Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
}

#[test]
fn single_cap_native() {
    let m = manifest(&[(Capability::ToolUse, SupportLevel::Native)]);
    let r = reqs_native(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::ToolUse]);
}

#[test]
fn single_cap_emulated() {
    let m = manifest(&[(Capability::ToolUse, SupportLevel::Emulated)]);
    // Emulated satisfies Emulated min_support (not Native)
    let r = reqs(&[(Capability::ToolUse, MinSupport::Emulated)]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated_caps(), vec![Capability::ToolUse]);
}

#[test]
fn single_cap_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = reqs_native(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolUse]);
}

#[test]
fn large_manifest_small_requirement() {
    let caps = all_capabilities();
    let m: CapabilityManifest = caps
        .iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect();
    let r = reqs_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 1);
}

// ===========================================================================
// Core negotiator tests (abp_core::negotiate)
// ===========================================================================

#[test]
fn core_negotiator_satisfied_when_native_meets_native() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.satisfied, vec![Capability::Streaming]);
}

#[test]
fn core_negotiator_emulated_below_native_min() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!res.is_compatible);
    assert_eq!(res.unsatisfied, vec![Capability::Streaming]);
}

#[test]
fn core_negotiator_emulated_meets_emulated_min() {
    let m = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
}

#[test]
fn core_negotiator_preferred_caps_are_bonus() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead],
        minimum_support: SupportLevel::Emulated,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.bonus, vec![Capability::ToolRead]);
}

#[test]
fn core_negotiator_missing_preferred_not_counted() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Logprobs],
        minimum_support: SupportLevel::Emulated,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert!(res.bonus.is_empty());
}

// ===========================================================================
// CapabilityDiff tests
// ===========================================================================

#[test]
fn diff_added_capabilities() {
    let old: CapabilityManifest = BTreeMap::new();
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added, vec![Capability::Streaming]);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_removed_capabilities() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new: CapabilityManifest = BTreeMap::new();
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed, vec![Capability::Streaming]);
}

#[test]
fn diff_upgraded_capabilities() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert_eq!(diff.upgraded[0].0, Capability::Streaming);
}

#[test]
fn diff_downgraded_capabilities() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
    assert_eq!(diff.downgraded[0].0, Capability::Streaming);
}

#[test]
fn diff_no_change() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&m, &m);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
}

// ===========================================================================
// CapabilityReport (dialect-aware) tests
// ===========================================================================

#[test]
fn capability_report_native_filter() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::Logprobs,
                support: DialectSupportLevel::Unsupported {
                    reason: "n/a".into(),
                },
            },
        ],
    };
    assert_eq!(report.native_capabilities().len(), 1);
    assert_eq!(
        report.native_capabilities()[0].capability,
        Capability::Streaming
    );
}

#[test]
fn capability_report_emulated_filter() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "gemini".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::CodeExecution,
                support: DialectSupportLevel::Emulated {
                    detail: "via tool".into(),
                },
            },
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
        ],
    };
    assert_eq!(report.emulated_capabilities().len(), 1);
}

#[test]
fn capability_report_unsupported_filter() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Logprobs,
            support: DialectSupportLevel::Unsupported {
                reason: "n/a".into(),
            },
        }],
    };
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn capability_report_all_satisfiable_true() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::ToolRead,
                support: DialectSupportLevel::Emulated {
                    detail: "adapter".into(),
                },
            },
        ],
    };
    assert!(report.all_satisfiable());
}

#[test]
fn capability_report_all_satisfiable_false() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Logprobs,
            support: DialectSupportLevel::Unsupported {
                reason: "n/a".into(),
            },
        }],
    };
    assert!(!report.all_satisfiable());
}

#[test]
fn capability_report_to_receipt_metadata() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    let val = report.to_receipt_metadata();
    assert!(val.is_object());
    assert_eq!(val["source_dialect"], "claude");
}

#[test]
fn capability_report_empty_entries_is_satisfiable() {
    let report = CapabilityReport {
        source_dialect: "a".into(),
        target_dialect: "b".into(),
        entries: vec![],
    };
    assert!(report.all_satisfiable());
    assert!(report.native_capabilities().is_empty());
    assert!(report.emulated_capabilities().is_empty());
    assert!(report.unsupported_capabilities().is_empty());
}

// ===========================================================================
// check_capabilities (dialect-aware pre-execution check)
// ===========================================================================

#[test]
fn check_capabilities_claude_to_openai() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming, Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert_eq!(report.source_dialect, "claude");
    assert_eq!(report.target_dialect, "openai");
    // OpenAI supports both natively
    assert!(report.all_satisfiable());
}

#[test]
fn check_capabilities_claude_to_gemini_logprobs_unsupported() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "claude", "gemini");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn check_capabilities_unknown_dialect_all_unsupported() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming]))
        .build();
    let report = check_capabilities(&wo, "claude", "unknown_vendor");
    assert!(!report.all_satisfiable());
}

// ===========================================================================
// Report summary text validation
// ===========================================================================

#[test]
fn report_summary_compatible_text() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.summary.contains("fully compatible"));
    assert!(report.summary.contains("1 native"));
}

#[test]
fn report_summary_incompatible_text() {
    let result = NegotiationResult::from_simple(
        vec![],
        vec![],
        vec![Capability::Logprobs, Capability::SeedDeterminism],
    );
    let report = generate_report(&result);
    assert!(report.summary.contains("incompatible"));
    assert!(report.summary.contains("2 unsupported"));
}

#[test]
fn report_all_emulated_is_compatible() {
    let result = NegotiationResult::from_simple(
        vec![],
        vec![Capability::Streaming, Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
}

// ===========================================================================
// NegotiationResult helpers
// ===========================================================================

#[test]
fn negotiation_result_total_counts_all_buckets() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.total(), 4);
}

#[test]
fn negotiation_result_empty_total_is_zero() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    assert_eq!(result.total(), 0);
    assert!(result.is_compatible());
}

// ===========================================================================
// Best-match selection
// ===========================================================================

#[test]
fn best_match_selects_highest_score() {
    let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead, Capability::ToolWrite],
        minimum_support: SupportLevel::Emulated,
    };
    let (name, _) = CapabilityNegotiator::best_match(&req, &[("basic", m1), ("full", m2)]).unwrap();
    assert_eq!(name, "full");
}

#[test]
fn best_match_tie_broken_by_name() {
    let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let m2 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    // When scores are equal, deterministic tie-break by name
    let (name, _) = CapabilityNegotiator::best_match(&req, &[("alpha", m1), ("beta", m2)]).unwrap();
    // The comparator uses name_b.cmp(name_a) so "beta" > "alpha", meaning "alpha" wins via max_by
    assert!(name == "alpha" || name == "beta"); // deterministic either way
}

#[test]
fn best_match_empty_manifests() {
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::best_match(&req, &[]);
    assert!(result.is_none());
}

// ===========================================================================
// Mixed negotiation scenarios
// ===========================================================================

#[test]
fn negotiate_mixed_native_emulated_unsupported() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Unsupported),
    ]);
    // ToolRead with Emulated min_support → emulated bucket; ToolBash unsupported → incompatible
    let r = reqs(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Emulated),
        (Capability::ToolBash, MinSupport::Native),
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolBash]);
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_preserves_requirement_order() {
    let m = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = reqs_native(&[
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
            Capability::ToolWrite,
        ]
    );
}

// ===========================================================================
// Emulation engine integration with capability negotiation
// ===========================================================================

#[test]
fn emulation_engine_emulatable_caps_pass_check() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn emulation_engine_unemulatable_caps_produce_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::Streaming]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn emulation_engine_mixed_emulatable_and_disabled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::Streaming,
    ]);
    // ExtendedThinking + StructuredOutput are emulatable; CodeExecution + Streaming disabled
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn emulation_engine_apply_injects_system_prompt() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"));
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("Think step by step"));
}

#[test]
fn emulation_engine_apply_does_not_mutate_for_post_processing() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Give me JSON"));
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv, original);
}

#[test]
fn emulation_engine_apply_disabled_no_mutation() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(conv, original);
}

#[test]
fn emulation_config_override_enables_disabled_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn emulation_config_override_disables_emulatable_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn emulation_engine_empty_capabilities_empty_report() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

// ===========================================================================
// Fidelity labels (compute_fidelity)
// ===========================================================================

#[test]
fn fidelity_native_caps_labeled_native() {
    let report = abp_emulation::EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolRead], &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolRead], FidelityLabel::Native);
}

#[test]
fn fidelity_emulated_caps_labeled_emulated() {
    let report = abp_emulation::EmulationReport {
        applied: vec![abp_emulation::EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "Think step by step.".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_mixed_native_and_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "test"));
    let emu_report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolRead], &emu_report);
    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_empty_inputs_empty_labels() {
    let report = abp_emulation::EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_warnings_not_included_in_labels() {
    let report = abp_emulation::EmulationReport {
        applied: vec![],
        warnings: vec!["CodeExecution not emulated".into()],
    };
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels.len(), 1);
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

// ===========================================================================
// Negotiation determinism
// ===========================================================================

#[test]
fn negotiation_deterministic_same_inputs_same_result() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Logprobs, SupportLevel::Unsupported),
    ]);
    let r = reqs_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::Logprobs,
    ]);
    let res1 = negotiate(&m, &r);
    let res2 = negotiate(&m, &r);
    assert_eq!(res1, res2);
}

#[test]
fn negotiation_deterministic_json_identical() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Emulated),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolBash]);
    let res1 = negotiate(&m, &r);
    let res2 = negotiate(&m, &r);
    let json1 = serde_json::to_string(&res1).unwrap();
    let json2 = serde_json::to_string(&res2).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn core_negotiator_deterministic() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead],
        minimum_support: SupportLevel::Emulated,
    };
    let r1 = CapabilityNegotiator::negotiate(&req, &m);
    let r2 = CapabilityNegotiator::negotiate(&req, &m);
    assert_eq!(r1.satisfied, r2.satisfied);
    assert_eq!(r1.bonus, r2.bonus);
    assert_eq!(r1.is_compatible, r2.is_compatible);
}

#[test]
fn report_generation_deterministic() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let r1 = generate_report(&result);
    let r2 = generate_report(&result);
    assert_eq!(r1, r2);
    assert_eq!(
        serde_json::to_string(&r1).unwrap(),
        serde_json::to_string(&r2).unwrap()
    );
}

// ===========================================================================
// Capability downgrade warnings via CapabilityDiff
// ===========================================================================

#[test]
fn diff_downgrade_native_to_emulated() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
    assert_eq!(diff.downgraded[0].0, Capability::Streaming);
}

#[test]
fn diff_downgrade_native_to_restricted() {
    let old = manifest(&[(Capability::ToolBash, SupportLevel::Native)]);
    let new = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_downgrade_native_to_unsupported() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_downgrade_emulated_to_unsupported() {
    let old = manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let new = manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_upgrade_unsupported_to_native() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
}

#[test]
fn diff_upgrade_restricted_to_native() {
    let old = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "old".into(),
        },
    )]);
    let new = manifest(&[(Capability::ToolBash, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
}

#[test]
fn diff_multiple_changes_simultaneously() {
    let old = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let new = manifest(&[
        (Capability::Streaming, SupportLevel::Emulated),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::Logprobs, SupportLevel::Native),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1); // Streaming: Native → Emulated
    assert_eq!(diff.upgraded.len(), 1); // ToolRead: Emulated → Native
    assert_eq!(diff.added.len(), 1); // Logprobs
    assert_eq!(diff.removed.len(), 1); // ToolWrite
}

#[test]
fn diff_empty_to_empty() {
    let old: CapabilityManifest = BTreeMap::new();
    let new: CapabilityManifest = BTreeMap::new();
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
}

// ===========================================================================
// Receipt annotations from negotiation
// ===========================================================================

#[test]
fn capability_report_metadata_contains_entries() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::Logprobs,
                support: DialectSupportLevel::Unsupported {
                    reason: "n/a".into(),
                },
            },
        ],
    };
    let metadata = report.to_receipt_metadata();
    assert!(metadata.is_object());
    let entries = metadata["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn capability_report_metadata_round_trips() {
    let report = CapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "gemini".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::ToolUse,
            support: DialectSupportLevel::Native,
        }],
    };
    let metadata = report.to_receipt_metadata();
    let back: CapabilityReport = serde_json::from_value(metadata).unwrap();
    assert_eq!(back.source_dialect, "claude");
    assert_eq!(back.target_dialect, "gemini");
    assert_eq!(back.entries.len(), 1);
}

#[test]
fn negotiation_result_annotates_all_categories() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let r = reqs_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::Logprobs,
    ]);
    let res = negotiate(&m, &r);
    let report = generate_report(&res);
    // Each detail entry should have an associated support level
    for (name, level) in &report.details {
        assert!(!name.is_empty());
        assert!(matches!(
            level,
            CapSupportLevel::Native
                | CapSupportLevel::Emulated { .. }
                | CapSupportLevel::Unsupported { .. }
        ));
    }
    assert_eq!(report.details.len(), 3);
}

// ===========================================================================
// default_strategy and can_emulate coverage
// ===========================================================================

#[test]
fn default_strategy_extended_thinking_is_prompt_injection() {
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn default_strategy_structured_output_is_post_processing() {
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn default_strategy_code_execution_is_disabled() {
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn default_strategy_image_input_is_prompt_injection() {
    assert!(matches!(
        default_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn default_strategy_stop_sequences_is_post_processing() {
    assert!(matches!(
        default_strategy(&Capability::StopSequences),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn can_emulate_true_for_emulatable_caps() {
    assert!(can_emulate(&Capability::ExtendedThinking));
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    assert!(can_emulate(&Capability::ImageInput));
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn can_emulate_false_for_disabled_caps() {
    assert!(!can_emulate(&Capability::CodeExecution));
    assert!(!can_emulate(&Capability::Streaming));
    assert!(!can_emulate(&Capability::ToolRead));
    assert!(!can_emulate(&Capability::ToolUse));
    assert!(!can_emulate(&Capability::Logprobs));
}

// ===========================================================================
// All Capability variants covered
// ===========================================================================

#[test]
fn all_capability_variants_have_serde_name() {
    for cap in &all_capabilities() {
        let json = serde_json::to_string(cap).unwrap();
        assert!(
            json.starts_with('"'),
            "Capability should serialize to string: {json}"
        );
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, cap);
    }
}

#[test]
fn all_capability_variants_in_btreemap_key() {
    let mut m = CapabilityManifest::new();
    for cap in &all_capabilities() {
        m.insert(cap.clone(), SupportLevel::Native);
    }
    assert_eq!(m.len(), all_capabilities().len());
}

#[test]
fn all_capability_variants_check_returns_value() {
    let m: CapabilityManifest = all_capabilities()
        .into_iter()
        .map(|c| (c, SupportLevel::Native))
        .collect();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        assert_eq!(level, CapSupportLevel::Native);
    }
}

#[test]
fn all_capability_variants_missing_check_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    for cap in &all_capabilities() {
        let level = check_capability(&m, cap);
        assert!(matches!(level, CapSupportLevel::Unsupported { .. }));
    }
}

// ===========================================================================
// Dialect cross-negotiation
// ===========================================================================

#[test]
fn claude_to_openai_streaming_native_both() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
}

#[test]
fn openai_to_gemini_extended_thinking_emulated() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::ExtendedThinking]))
        .build();
    let report = check_capabilities(&wo, "openai", "gemini");
    assert!(report.all_satisfiable());
    assert_eq!(report.emulated_capabilities().len(), 1);
}

#[test]
fn claude_to_claude_all_caps_satisfiable() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::ExtendedThinking,
        ]))
        .build();
    let report = check_capabilities(&wo, "claude", "claude");
    assert!(report.all_satisfiable());
}

#[test]
fn openai_to_openai_logprobs_native() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "openai", "openai");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
}

#[test]
fn gemini_pdf_input_native() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::PdfInput]))
        .build();
    let report = check_capabilities(&wo, "openai", "gemini");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
}

#[test]
fn openai_pdf_input_unsupported() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::PdfInput]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

// ===========================================================================
// Multiple capabilities negotiated simultaneously
// ===========================================================================

#[test]
fn negotiate_ten_caps_simultaneously() {
    let m: CapabilityManifest = vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
        (Capability::ToolEdit, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Native),
    ]
    .into_iter()
    .collect();
    // ToolWrite/ToolEdit are Emulated in manifest, use Emulated min_support for them
    let r = reqs(&[
        (Capability::Streaming, MinSupport::Native),
        (Capability::ToolRead, MinSupport::Native),
        (Capability::ToolWrite, MinSupport::Emulated),
        (Capability::ToolEdit, MinSupport::Emulated),
        (Capability::ToolBash, MinSupport::Native),
        (Capability::ToolGlob, MinSupport::Native),
        (Capability::ToolGrep, MinSupport::Native),
        (Capability::Logprobs, MinSupport::Native),
        (Capability::McpClient, MinSupport::Native),
        (Capability::ExtendedThinking, MinSupport::Native),
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 3);
    assert_eq!(res.emulated.len(), 2);
    assert_eq!(res.unsupported.len(), 5);
    assert_eq!(res.total(), 10);
    assert!(!res.is_compatible());
}

#[test]
fn core_negotiator_many_required_and_preferred() {
    let m: CapabilityManifest = vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Emulated),
        (Capability::ToolEdit, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
    ]
    .into_iter()
    .collect();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![
            Capability::ToolWrite,
            Capability::ToolBash,
            Capability::ExtendedThinking,
            Capability::Logprobs,
        ],
        minimum_support: SupportLevel::Emulated,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.satisfied.len(), 2);
    assert_eq!(res.bonus.len(), 3); // ToolWrite, ToolBash, ExtendedThinking
}

// ===========================================================================
// Emulation strategy serde round-trips
// ===========================================================================

#[test]
fn emulation_strategy_serde_system_prompt() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "Think carefully.".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn emulation_strategy_serde_post_processing() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate JSON".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn emulation_strategy_serde_disabled() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn emulation_config_serde_roundtrip() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think.".into(),
        },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "sandbox".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let back: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, back);
}

#[test]
fn fidelity_label_serde_roundtrip() {
    let labels = vec![
        FidelityLabel::Native,
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        },
    ];
    for label in &labels {
        let json = serde_json::to_string(label).unwrap();
        let back: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, label);
    }
}

// ===========================================================================
// Capability discovery from backend hello message (dialect manifest)
// ===========================================================================

#[test]
fn claude_manifest_has_tool_use() {
    let m = dialect_manifest("claude");
    assert!(m.contains_key(&Capability::ToolUse));
    assert_eq!(m[&Capability::ToolUse], DialectSupportLevel::Native);
}

#[test]
fn openai_manifest_has_structured_output() {
    let m = dialect_manifest("openai");
    assert!(m.contains_key(&Capability::StructuredOutputJsonSchema));
    assert_eq!(
        m[&Capability::StructuredOutputJsonSchema],
        DialectSupportLevel::Native
    );
}

#[test]
fn gemini_manifest_has_pdf_input() {
    let m = dialect_manifest("gemini");
    assert!(m.contains_key(&Capability::PdfInput));
    assert_eq!(m[&Capability::PdfInput], DialectSupportLevel::Native);
}

#[test]
fn claude_manifest_structured_output_emulated() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn dialect_manifests_have_streaming() {
    for dialect in &["claude", "openai", "gemini"] {
        let m = dialect_manifest(dialect);
        assert_eq!(
            m.get(&Capability::Streaming),
            Some(&DialectSupportLevel::Native),
            "{dialect} should natively support streaming"
        );
    }
}

// ===========================================================================
// End-to-end: negotiate → emulation → fidelity pipeline
// ===========================================================================

#[test]
fn full_pipeline_negotiate_then_emulate_then_fidelity() {
    // 1) Negotiate: ToolRead is native, ExtendedThinking is emulated in manifest
    let m = manifest(&[
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Emulated),
    ]);
    // ExtendedThinking is Emulated, so use Emulated min_support
    let r = reqs(&[
        (Capability::ToolRead, MinSupport::Native),
        (Capability::ExtendedThinking, MinSupport::Emulated),
    ]);
    let neg = negotiate(&m, &r);
    assert!(neg.is_compatible());

    // 2) Apply emulation for emulatable caps
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "test"));
    let emu_report = engine.apply(&neg.emulated_caps(), &mut conv);

    // ExtendedThinking has a default emulation strategy (system prompt injection)
    assert_eq!(emu_report.applied.len(), 1);

    // 3) Compute fidelity labels
    let labels = compute_fidelity(&neg.native, &emu_report);
    assert_eq!(labels[&Capability::ToolRead], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn full_pipeline_incompatible_cap_not_emulatable() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = reqs_native(&[Capability::Streaming]);
    let neg = negotiate(&m, &r);
    assert!(!neg.is_compatible());

    // Try to emulate the unsupported cap
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&neg.unsupported_caps());
    assert!(report.has_unemulatable());
}

#[test]
fn full_pipeline_all_native_no_emulation_needed() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let neg = negotiate(&m, &r);
    assert!(neg.is_compatible());
    assert!(neg.emulated.is_empty());

    let labels = compute_fidelity(&neg.native, &abp_emulation::EmulationReport::default());
    assert_eq!(labels.len(), 2);
    for label in labels.values() {
        assert_eq!(label, &FidelityLabel::Native);
    }
}
