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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the capability negotiation subsystem.
//!
//! Covers: manifest construction, SupportLevel comparisons, negotiation,
//! backend projection, emulation decisions, multi-backend scenarios, edge cases.

use std::collections::BTreeMap;

use abp_capability::{
    CompatibilityReport, NegotiationResult, SupportLevel as CapSupportLevel, check_capability,
    generate_report, negotiate,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_projection::{ProjectionError, ProjectionMatrix};

// =========================================================================
// Helpers
// =========================================================================

fn manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
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

fn wo_with_reqs(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

fn passthrough_wo(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": "claude" });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

/// All capability variants for exhaustive iteration.
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

// =========================================================================
// 1. Capability manifest construction and serialization
// =========================================================================

#[test]
fn manifest_construction_empty() {
    let m: CapabilityManifest = BTreeMap::new();
    assert!(m.is_empty());
}

#[test]
fn manifest_construction_single() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert_eq!(m.len(), 1);
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn manifest_construction_multiple() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolWrite, SupportLevel::Unsupported),
    ]);
    assert_eq!(m.len(), 3);
}

#[test]
fn manifest_btreemap_is_sorted() {
    let m = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let keys: Vec<_> = m.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys);
}

#[test]
fn manifest_serde_roundtrip() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::Logprobs, SupportLevel::Unsupported),
        (
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        ),
    ]);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(m.len(), back.len());
    for k in m.keys() {
        assert!(back.contains_key(k));
    }
}

#[test]
fn manifest_all_capabilities_native() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, SupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    assert_eq!(m.len(), all_capabilities().len());
}

#[test]
fn manifest_duplicate_key_last_wins() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Emulated),
    ]);
    assert_eq!(m.len(), 1);
}

#[test]
fn manifest_get_existing() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn manifest_get_missing() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert!(!m.contains_key(&Capability::Logprobs));
}

// =========================================================================
// 2. SupportLevel comparisons (satisfies)
// =========================================================================

#[test]
fn native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn unsupported_does_not_satisfy_native() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_does_not_satisfy_emulated() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_satisfies_emulated() {
    let r = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(r.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_does_not_satisfy_native() {
    let r = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn support_level_serde_native() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Native));
}

#[test]
fn support_level_serde_emulated() {
    let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Emulated));
}

#[test]
fn support_level_serde_unsupported() {
    let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Unsupported));
}

#[test]
fn support_level_serde_restricted() {
    let r = SupportLevel::Restricted {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Restricted { .. }));
}

// =========================================================================
// 3. Capability negotiation: requirements vs manifest matching
// =========================================================================

#[test]
fn negotiate_all_native_match() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
    assert!(res.emulated.is_empty());
    assert!(res.unsupported.is_empty());
    assert!(res.is_compatible());
}

#[test]
fn negotiate_all_emulated_match() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Emulated),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let r = require_emulated(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(res.native.is_empty());
    assert_eq!(res.emulated.len(), 2);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_all_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported.len(), 2);
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_mixed_native_emulated_unsupported() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let r = require_emulated(&[
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
fn negotiate_restricted_treated_as_emulatable() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    )]);
    let r = require_emulated(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolBash]);
    assert!(res.is_compatible());
}

#[test]
fn negotiate_explicit_unsupported_in_manifest() {
    let m = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    let r = require_native(&[Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported_caps(), vec![Capability::Logprobs]);
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_preserves_requirement_order() {
    let m = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
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
            Capability::ToolWrite,
        ]
    );
}

#[test]
fn negotiate_duplicate_requirements_kept() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let r = require_native(&[Capability::Streaming, Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
}

#[test]
fn negotiate_total_matches_input() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let r = require_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::Logprobs,
    ]);
    let res = negotiate(&m, &r);
    assert_eq!(res.total(), 3);
}

// =========================================================================
// 3b. check_capability
// =========================================================================

#[test]
fn check_cap_native() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        CapSupportLevel::Native
    );
}

#[test]
fn check_cap_emulated() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        CapSupportLevel::Emulated {
            method: "adapter".into()
        }
    );
}

#[test]
fn check_cap_unsupported_explicit() {
    let m = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    assert_eq!(
        check_capability(&m, &Capability::Logprobs),
        CapSupportLevel::Unsupported {
            reason: "explicitly marked unsupported".into()
        }
    );
}

#[test]
fn check_cap_missing() {
    let m: CapabilityManifest = BTreeMap::new();
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        CapSupportLevel::Unsupported {
            reason: "not declared in manifest".into()
        }
    );
}

#[test]
fn check_cap_restricted_contains_reason() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    if let CapSupportLevel::Restricted { reason } = level {
        assert!(reason.contains("policy"));
    } else {
        panic!("expected Restricted");
    }
}

#[test]
fn check_cap_every_core_variant() {
    let cases: Vec<(SupportLevel, bool)> = vec![
        (SupportLevel::Native, true),
        (SupportLevel::Emulated, true),
        (SupportLevel::Unsupported, false),
        (SupportLevel::Restricted { reason: "x".into() }, true),
    ];
    for (core_level, should_satisfy) in cases {
        let m = manifest(&[(Capability::Streaming, core_level)]);
        let level = check_capability(&m, &Capability::Streaming);
        let satisfied = !matches!(level, CapSupportLevel::Unsupported { .. });
        assert_eq!(satisfied, should_satisfy);
    }
}

// =========================================================================
// 3c. generate_report
// =========================================================================

#[test]
fn report_fully_compatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolRead],
        vec![Capability::ToolWrite],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn report_incompatible() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn report_empty_result() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
}

#[test]
fn report_counts_match() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 2);
    assert_eq!(report.unsupported_count, 1);
}

#[test]
fn report_details_length_matches_total() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn report_summary_contains_counts() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::ToolBash],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.summary.contains("2 native"));
    assert!(report.summary.contains("1 emulated"));
    assert!(report.summary.contains("0 unsupported"));
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

#[test]
fn report_serde_roundtrip() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back, report);
}

#[test]
fn negotiation_result_serde_roundtrip() {
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
fn cap_support_level_serde_roundtrip() {
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

// =========================================================================
// 4. Backend projection: selecting best backend from capability matrix
// =========================================================================

#[test]
fn project_single_backend_exact_match() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "only");
    assert!(result.fallback_chain.is_empty());
}

#[test]
fn project_selects_higher_capability_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn project_priority_breaks_tie() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "low-prio",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "high-prio",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "high-prio");
}

#[test]
fn project_empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn project_no_suitable_backend_error() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "limited",
        manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn project_empty_caps_backend_no_suitable() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn project_fallback_chain_excludes_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    for entry in &result.fallback_chain {
        assert_ne!(entry.backend_id, result.selected_backend);
    }
}

#[test]
fn project_fallback_chain_descending_score() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 90), ("b", 60), ("c", 30), ("d", 10)] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|e| e.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback chain not sorted descending");
    }
}

#[test]
fn project_empty_requirements_all_match() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(CapabilityRequirements::default());
    let result = pm.project(&wo).unwrap();
    assert!(!result.selected_backend.is_empty());
    assert_eq!(result.fallback_chain.len(), 1);
}

#[test]
fn project_backend_count() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    assert_eq!(pm.backend_count(), 2);
}

#[test]
fn project_register_overwrites() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::Claude,
        90,
    );
    assert_eq!(pm.backend_count(), 1);
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn project_priority_normalized_to_max() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn project_zero_priority_still_selectable() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "zero-prio",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        0,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "zero-prio");
}

// =========================================================================
// 5. Emulation decisions: when to emulate vs fail
// =========================================================================

#[test]
fn emulation_listed_for_emulated_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
}

#[test]
fn emulation_multiple_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations.len(), 2);
}

#[test]
fn emulation_empty_when_all_native() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert!(result.required_emulations.is_empty());
}

#[test]
fn emulation_strategy_is_adapter() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::ToolRead]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations[0].strategy, "adapter");
}

#[test]
fn emulation_restricted_also_listed() {
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = require_emulated(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert_eq!(res.emulated.len(), 1);
}

#[test]
fn emulation_fail_when_unsupported() {
    let m = manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    let r = require_native(&[Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 1);
}

#[test]
fn emulation_fail_when_missing_from_manifest() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
}

// =========================================================================
// 6. Multi-backend negotiation scenarios
// =========================================================================

#[test]
fn multi_backend_best_coverage_wins() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "rich",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "poor",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolBash,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "rich");
}

#[test]
fn multi_backend_three_tiered_fallback() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "gold",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "silver",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "bronze",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        20,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "gold");
    assert_eq!(result.fallback_chain.len(), 2);
}

#[test]
fn multi_backend_incompatible_still_in_fallback() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "compatible",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "compatible");
    assert!(
        result
            .fallback_chain
            .iter()
            .any(|e| e.backend_id == "partial")
    );
}

#[test]
fn multi_backend_all_native_same_priority_id_sort() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "b-backend",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "a-backend",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    // Same score → alphabetical id sort
    assert_eq!(result.selected_backend, "a-backend");
}

#[test]
fn multi_backend_five_backends_complex() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be1",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        90,
    );
    pm.register_backend(
        "be2",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::Claude,
        80,
    );
    pm.register_backend(
        "be3",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        70,
    );
    pm.register_backend(
        "be4",
        manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
        Dialect::Codex,
        60,
    );
    pm.register_backend("be5", CapabilityManifest::new(), Dialect::Kimi, 50);
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    // be1 has full native coverage + highest priority among compatible
    assert_eq!(result.selected_backend, "be1");
    assert_eq!(result.fallback_chain.len(), 4);
}

#[test]
fn multi_backend_passthrough_prefers_same_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let wo = passthrough_wo(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn multi_backend_passthrough_bonus_overrides_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-low",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        30,
    );
    pm.register_backend(
        "openai-high",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    let wo = passthrough_wo(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "claude-low");
}

#[test]
fn multi_backend_non_passthrough_ignores_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-low",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        30,
    );
    pm.register_backend(
        "openai-high",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "openai-high");
}

#[test]
fn multi_backend_same_dialect_fidelity_boost() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    // Same-dialect gets 1.0 fidelity vs potentially 0.0 for openai (no mapping configured)
    assert_eq!(result.selected_backend, "claude-be");
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

// =========================================================================
// 7. Edge cases
// =========================================================================

#[test]
fn edge_empty_manifest_empty_requirements() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn edge_empty_manifest_single_requirement() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
    assert!(!res.is_compatible());
}

#[test]
fn edge_empty_requirements_populated_manifest() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn edge_all_unsupported_manifest() {
    let caps = all_capabilities();
    let entries: Vec<_> = caps
        .iter()
        .map(|c| (c.clone(), SupportLevel::Unsupported))
        .collect();
    let m = manifest(&entries);
    let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 2);
}

#[test]
fn edge_impossible_requirements_all_missing() {
    let m: CapabilityManifest = BTreeMap::new();
    let caps = all_capabilities();
    let r = require_native(&caps);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), caps.len());
}

#[test]
fn edge_single_capability_native() {
    let m = manifest(&[(Capability::ToolUse, SupportLevel::Native)]);
    let r = require_native(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::ToolUse]);
    assert!(res.is_compatible());
}

#[test]
fn edge_single_capability_emulated() {
    let m = manifest(&[(Capability::ToolUse, SupportLevel::Emulated)]);
    let r = require_emulated(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolUse]);
    assert!(res.is_compatible());
}

#[test]
fn edge_single_capability_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolUse]);
    assert!(!res.is_compatible());
}

#[test]
fn edge_large_manifest_small_requirements() {
    let all = all_capabilities();
    let entries: Vec<_> = all
        .iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    let r = require_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 1);
}

#[test]
fn edge_negotiation_result_total_zero() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    assert_eq!(result.total(), 0);
    assert!(result.is_compatible());
}

#[test]
fn edge_negotiation_result_is_compatible_only_emulated() {
    let result = NegotiationResult::from_simple(vec![], vec![Capability::Streaming], vec![]);
    assert!(result.is_compatible());
}

#[test]
fn edge_negotiation_result_is_not_compatible_one_unsupported() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert!(!result.is_compatible());
}

#[test]
fn edge_projection_error_display_empty_matrix() {
    let err = ProjectionError::EmptyMatrix;
    let msg = format!("{err}");
    assert!(msg.contains("empty"));
}

#[test]
fn edge_projection_error_display_no_suitable() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("test reason"));
}

#[test]
fn edge_projection_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "round trip".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn edge_projection_error_empty_matrix_serde() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn edge_project_with_all_capabilities_native() {
    let all = all_capabilities();
    let entries: Vec<_> = all
        .iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect();
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("full", manifest(&entries), Dialect::OpenAi, 100);
    let wo = wo_with_reqs(require_emulated(&all));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "full");
    assert!(result.required_emulations.is_empty());
}

#[test]
fn edge_project_with_all_capabilities_emulated() {
    let all = all_capabilities();
    let entries: Vec<_> = all
        .iter()
        .map(|c| (c.clone(), SupportLevel::Emulated))
        .collect();
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("emu", manifest(&entries), Dialect::Claude, 100);
    let wo = wo_with_reqs(require_emulated(&all));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "emu");
    assert_eq!(result.required_emulations.len(), all.len());
}

#[test]
fn edge_check_all_capabilities_against_empty() {
    let m: CapabilityManifest = BTreeMap::new();
    for cap in &all_capabilities() {
        assert!(matches!(
            check_capability(&m, cap),
            CapSupportLevel::Unsupported { .. }
        ));
    }
}

#[test]
fn edge_check_all_capabilities_native() {
    let all = all_capabilities();
    let entries: Vec<_> = all
        .iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    for cap in &all {
        assert_eq!(check_capability(&m, cap), CapSupportLevel::Native);
    }
}

#[test]
fn edge_report_large_unsupported() {
    let all = all_capabilities();
    let result = NegotiationResult::from_simple(vec![], vec![], all.clone());
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.unsupported_count, all.len());
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn edge_report_large_native() {
    let all = all_capabilities();
    let result = NegotiationResult::from_simple(all.clone(), vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, all.len());
}

#[test]
fn edge_many_backends_one_compatible() {
    let mut pm = ProjectionMatrix::new();
    // 9 empty-capability backends
    for i in 0..9 {
        pm.register_backend(
            format!("empty-{i}"),
            CapabilityManifest::new(),
            Dialect::OpenAi,
            50,
        );
    }
    // 1 fully capable backend
    pm.register_backend(
        "capable",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "capable");
}

#[test]
fn edge_capability_coverage_full() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let r = require_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert_eq!(res.total(), r.required.len());
    assert!(res.is_compatible());
}

#[test]
fn edge_duplicate_caps_in_requirements_unsupported() {
    let m: CapabilityManifest = BTreeMap::new();
    let r = require_native(&[Capability::Logprobs, Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert_eq!(res.unsupported.len(), 2);
}

#[test]
fn edge_restricted_with_long_reason() {
    let long_reason = "a".repeat(1000);
    let m = manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: long_reason.clone(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    if let CapSupportLevel::Restricted { reason } = level {
        assert!(reason.contains(&long_reason));
    } else {
        panic!("expected Restricted");
    }
}

#[test]
fn edge_fidelity_score_full_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let wo = wo_with_reqs(require_emulated(&[
        Capability::Streaming,
        Capability::ToolRead,
    ]));
    let result = pm.project(&wo).unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}
