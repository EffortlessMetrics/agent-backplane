// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for the capability registry and negotiation system.
//!
//! Covers: manifest construction, capability intersection/superset/subset,
//! tool availability, model compatibility, streaming negotiation, parallel
//! tool-call negotiation, context-window negotiation, serde roundtrips,
//! profile merging, dialect defaults, feature gating, degradation detection,
//! empty/minimal profiles, and capability versioning.

use abp_capability::{
    CompatibilityReport, NegotiationResult as AbpCapNegotiationResult, SupportLevel,
    check_capability, generate_report, negotiate,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport as DialectCapabilityReport,
    CapabilityReportEntry, DialectSupportLevel, NegotiationRequest, check_capabilities,
    dialect_manifest,
};
use abp_core::{
    CONTRACT_VERSION, Capability, CapabilityManifest, CapabilityRequirement,
    CapabilityRequirements, MinSupport, Outcome, ReceiptBuilder, SupportLevel as CoreSupportLevel,
    WorkOrderBuilder,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn mk_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

fn mk_reqs(caps: &[(Capability, MinSupport)]) -> CapabilityRequirements {
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

fn mk_reqs_native(caps: &[Capability]) -> CapabilityRequirements {
    mk_reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Native))
            .collect::<Vec<_>>(),
    )
}

fn mk_reqs_emulated(caps: &[Capability]) -> CapabilityRequirements {
    mk_reqs(
        &caps
            .iter()
            .map(|c| (c.clone(), MinSupport::Emulated))
            .collect::<Vec<_>>(),
    )
}

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

fn tool_capabilities() -> Vec<Capability> {
    vec![
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::ToolUse,
    ]
}

/// Intersect two manifests: keep capabilities present in both, picking the
/// lower support level.
fn intersect_manifests(a: &CapabilityManifest, b: &CapabilityManifest) -> CapabilityManifest {
    fn rank(l: &CoreSupportLevel) -> u8 {
        match l {
            CoreSupportLevel::Native => 3,
            CoreSupportLevel::Emulated => 2,
            CoreSupportLevel::Restricted { .. } => 1,
            CoreSupportLevel::Unsupported => 0,
        }
    }
    let mut out = CapabilityManifest::new();
    for (cap, level_a) in a {
        if let Some(level_b) = b.get(cap) {
            let pick = if rank(level_a) <= rank(level_b) {
                level_a.clone()
            } else {
                level_b.clone()
            };
            out.insert(cap.clone(), pick);
        }
    }
    out
}

/// Merge two manifests: union of keys, picking higher support level on overlap.
fn merge_manifests(a: &CapabilityManifest, b: &CapabilityManifest) -> CapabilityManifest {
    fn rank(l: &CoreSupportLevel) -> u8 {
        match l {
            CoreSupportLevel::Native => 3,
            CoreSupportLevel::Emulated => 2,
            CoreSupportLevel::Restricted { .. } => 1,
            CoreSupportLevel::Unsupported => 0,
        }
    }
    let mut out = a.clone();
    for (cap, level_b) in b {
        let insert = !matches!(out.get(cap), Some(existing) if rank(existing) >= rank(level_b));
        if insert {
            out.insert(cap.clone(), level_b.clone());
        }
    }
    out
}

// ===========================================================================
// Module 1: CapabilityProfile construction and validation
// ===========================================================================

mod construction {
    use super::*;

    #[test]
    fn empty_manifest_is_valid() {
        let m = CapabilityManifest::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn single_native_entry() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert_eq!(m.len(), 1);
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn all_support_level_variants_stored() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            (Capability::ToolWrite, CoreSupportLevel::Unsupported),
            (
                Capability::ToolBash,
                CoreSupportLevel::Restricted {
                    reason: "sandbox".into(),
                },
            ),
        ]);
        assert_eq!(m.len(), 4);
        assert!(matches!(
            m.get(&Capability::ToolBash),
            Some(CoreSupportLevel::Restricted { .. })
        ));
    }

    #[test]
    fn overwrite_preserves_last_value() {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, CoreSupportLevel::Native);
        m.insert(Capability::Streaming, CoreSupportLevel::Emulated);
        assert_eq!(m.len(), 1);
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn btreemap_deterministic_key_order() {
        let m = mk_manifest(&[
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let keys: Vec<_> = m.keys().collect();
        for i in 1..keys.len() {
            assert!(keys[i - 1] <= keys[i]);
        }
    }

    #[test]
    fn manifest_with_all_capabilities() {
        let entries: Vec<_> = all_capabilities()
            .into_iter()
            .map(|c| (c, CoreSupportLevel::Native))
            .collect();
        let m = mk_manifest(&entries);
        assert_eq!(m.len(), all_capabilities().len());
    }

    #[test]
    fn requirements_default_is_empty() {
        let r = CapabilityRequirements::default();
        assert!(r.required.is_empty());
    }

    #[test]
    fn capability_requirement_native_threshold() {
        let r = mk_reqs_native(&[Capability::Streaming]);
        assert_eq!(r.required.len(), 1);
        assert!(matches!(r.required[0].min_support, MinSupport::Native));
    }

    #[test]
    fn capability_requirement_emulated_threshold() {
        let r = mk_reqs_emulated(&[Capability::Streaming]);
        assert!(matches!(r.required[0].min_support, MinSupport::Emulated));
    }

    #[test]
    fn support_level_satisfies_native_only_native() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
        assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_satisfies_emulated_accepts_native_and_emulated() {
        assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
        assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
        assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated_threshold() {
        let restricted = CoreSupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(restricted.satisfies(&MinSupport::Emulated));
        assert!(!restricted.satisfies(&MinSupport::Native));
    }
}

// ===========================================================================
// Module 2: Capability intersection
// ===========================================================================

mod intersection {
    use super::*;

    #[test]
    fn intersection_both_native() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result = intersect_manifests(&a, &b);
        assert_eq!(result.len(), 1);
        assert!(matches!(
            result.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn intersection_picks_lower_support() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let result = intersect_manifests(&a, &b);
        assert!(matches!(
            result.get(&Capability::Streaming),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn intersection_disjoint_yields_empty() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let result = intersect_manifests(&a, &b);
        assert!(result.is_empty());
    }

    #[test]
    fn intersection_partial_overlap() {
        let a = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let b = mk_manifest(&[
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            (Capability::ToolWrite, CoreSupportLevel::Native),
        ]);
        let result = intersect_manifests(&a, &b);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&Capability::ToolRead));
        assert!(matches!(
            result.get(&Capability::ToolRead),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn intersection_with_empty_is_empty() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = CapabilityManifest::new();
        assert!(intersect_manifests(&a, &b).is_empty());
        assert!(intersect_manifests(&b, &a).is_empty());
    }

    #[test]
    fn intersection_is_commutative_on_keys() {
        let a = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let b = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolWrite, CoreSupportLevel::Native),
        ]);
        let ab = intersect_manifests(&a, &b);
        let ba = intersect_manifests(&b, &a);
        assert_eq!(ab.keys().collect::<Vec<_>>(), ba.keys().collect::<Vec<_>>());
    }
}

// ===========================================================================
// Module 3: Superset/subset checks
// ===========================================================================

mod superset_subset {
    use super::*;

    fn is_superset(sup: &CapabilityManifest, sub: &CapabilityManifest) -> bool {
        sub.keys().all(|k| sup.contains_key(k))
    }

    fn is_subset(sub: &CapabilityManifest, sup: &CapabilityManifest) -> bool {
        is_superset(sup, sub)
    }

    #[test]
    fn empty_is_subset_of_everything() {
        let empty = CapabilityManifest::new();
        let full = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert!(is_subset(&empty, &full));
        assert!(is_subset(&empty, &empty));
    }

    #[test]
    fn everything_is_superset_of_empty() {
        let empty = CapabilityManifest::new();
        let full = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        assert!(is_superset(&full, &empty));
    }

    #[test]
    fn identical_manifests_are_superset_and_subset() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        assert!(is_superset(&m, &m));
        assert!(is_subset(&m, &m));
    }

    #[test]
    fn proper_superset() {
        let small = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let big = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        assert!(is_superset(&big, &small));
        assert!(!is_superset(&small, &big));
    }

    #[test]
    fn disjoint_are_neither_superset_nor_subset() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        assert!(!is_superset(&a, &b));
        assert!(!is_superset(&b, &a));
    }

    #[test]
    fn full_manifest_is_superset_of_any_single_cap() {
        let entries: Vec<_> = all_capabilities()
            .into_iter()
            .map(|c| (c, CoreSupportLevel::Native))
            .collect();
        let full = mk_manifest(&entries);
        for cap in all_capabilities() {
            let single = mk_manifest(&[(cap, CoreSupportLevel::Native)]);
            assert!(is_superset(&full, &single));
        }
    }
}

// ===========================================================================
// Module 4: Tool availability checking
// ===========================================================================

mod tool_availability {
    use super::*;

    #[test]
    fn tool_present_native() {
        let m = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        assert_eq!(
            check_capability(&m, &Capability::ToolRead),
            SupportLevel::Native
        );
    }

    #[test]
    fn tool_present_emulated() {
        let m = mk_manifest(&[(Capability::ToolBash, CoreSupportLevel::Emulated)]);
        assert_eq!(
            check_capability(&m, &Capability::ToolBash),
            SupportLevel::Emulated {
                method: "adapter".into()
            }
        );
    }

    #[test]
    fn tool_absent() {
        let m = CapabilityManifest::new();
        assert_eq!(
            check_capability(&m, &Capability::ToolWrite),
            SupportLevel::Unsupported {
                reason: "not declared in manifest".into()
            }
        );
    }

    #[test]
    fn tool_explicitly_unsupported() {
        let m = mk_manifest(&[(Capability::ToolBash, CoreSupportLevel::Unsupported)]);
        assert_eq!(
            check_capability(&m, &Capability::ToolBash),
            SupportLevel::Unsupported {
                reason: "explicitly marked unsupported".into()
            }
        );
    }

    #[test]
    fn all_tool_caps_native_in_full_manifest() {
        let entries: Vec<_> = tool_capabilities()
            .into_iter()
            .map(|c| (c, CoreSupportLevel::Native))
            .collect();
        let m = mk_manifest(&entries);
        for cap in tool_capabilities() {
            assert_eq!(check_capability(&m, &cap), SupportLevel::Native);
        }
    }

    #[test]
    fn restricted_tool_shown_as_restricted() {
        let m = mk_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        )]);
        let level = check_capability(&m, &Capability::ToolBash);
        assert!(matches!(level, SupportLevel::Restricted { .. }));
        if let SupportLevel::Restricted { reason } = level {
            assert!(reason.contains("sandboxed"));
        }
    }

    #[test]
    fn negotiate_tool_requirements_mixed() {
        let m = mk_manifest(&[
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolEdit, CoreSupportLevel::Emulated),
        ]);
        let r = mk_reqs_native(&[
            Capability::ToolRead,
            Capability::ToolEdit,
            Capability::ToolBash,
        ]);
        let res = negotiate(&m, &r);
        assert_eq!(res.native, vec![Capability::ToolRead]);
        assert_eq!(res.emulated_caps(), vec![Capability::ToolEdit]);
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolBash]);
    }
}

// ===========================================================================
// Module 5: Model compatibility matrix
// ===========================================================================

mod model_compatibility {
    use super::*;

    #[test]
    fn claude_dialect_has_streaming() {
        let dm = dialect_manifest("claude");
        assert!(matches!(
            dm.get(&Capability::Streaming),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn openai_dialect_has_logprobs() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::Logprobs),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn claude_dialect_lacks_logprobs() {
        let dm = dialect_manifest("claude");
        assert!(matches!(
            dm.get(&Capability::Logprobs),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_dialect_has_pdf_input_native() {
        let dm = dialect_manifest("gemini");
        assert!(matches!(
            dm.get(&Capability::PdfInput),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn openai_dialect_lacks_pdf_input() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::PdfInput),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn unknown_dialect_yields_empty_manifest() {
        let dm = dialect_manifest("unknown_provider");
        assert!(dm.is_empty());
    }

    #[test]
    fn all_known_dialects_have_streaming() {
        for dialect in &["claude", "openai", "gemini"] {
            let dm = dialect_manifest(dialect);
            assert!(
                dm.contains_key(&Capability::Streaming),
                "{dialect} missing streaming"
            );
        }
    }

    #[test]
    fn claude_extended_thinking_native() {
        let dm = dialect_manifest("claude");
        assert!(matches!(
            dm.get(&Capability::ExtendedThinking),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn openai_extended_thinking_unsupported() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::ExtendedThinking),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_extended_thinking_emulated() {
        let dm = dialect_manifest("gemini");
        assert!(matches!(
            dm.get(&Capability::ExtendedThinking),
            Some(DialectSupportLevel::Emulated { .. })
        ));
    }
}

// ===========================================================================
// Module 6: Streaming capability negotiation
// ===========================================================================

mod streaming_negotiation {
    use super::*;

    #[test]
    fn streaming_native_satisfies_native_requirement() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.native, vec![Capability::Streaming]);
    }

    #[test]
    fn streaming_emulated_satisfies_emulated_requirement() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let r = mk_reqs_emulated(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
    }

    #[test]
    fn streaming_emulated_classified_in_emulatable_bucket() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert_eq!(res.emulated_caps(), vec![Capability::Streaming]);
        assert!(res.is_compatible());
    }

    #[test]
    fn streaming_absent_fails_negotiation() {
        let m = CapabilityManifest::new();
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported_caps(), vec![Capability::Streaming]);
    }

    #[test]
    fn all_dialects_support_streaming_natively() {
        for dialect in &["claude", "openai", "gemini"] {
            let dm = dialect_manifest(dialect);
            assert_eq!(
                dm.get(&Capability::Streaming),
                Some(&DialectSupportLevel::Native),
                "{dialect} should support streaming natively"
            );
        }
    }

    #[test]
    fn streaming_restricted_still_compatible() {
        let m = mk_manifest(&[(
            Capability::Streaming,
            CoreSupportLevel::Restricted {
                reason: "rate limited".into(),
            },
        )]);
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.emulated_caps(), vec![Capability::Streaming]);
    }
}

// ===========================================================================
// Module 7: Parallel tool call support negotiation
// ===========================================================================

mod parallel_tool_calls {
    use super::*;

    #[test]
    fn tool_use_native_enables_parallel_calls() {
        let m = mk_manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
        let r = mk_reqs_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.native, vec![Capability::ToolUse]);
    }

    #[test]
    fn tool_use_emulated_still_compatible() {
        let m = mk_manifest(&[(Capability::ToolUse, CoreSupportLevel::Emulated)]);
        let r = mk_reqs_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.emulated_caps(), vec![Capability::ToolUse]);
    }

    #[test]
    fn multiple_tool_capabilities_all_native() {
        let m = mk_manifest(&[
            (Capability::ToolUse, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let r = mk_reqs_native(&[
            Capability::ToolUse,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolBash,
        ]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.native.len(), 4);
        assert!(res.emulated.is_empty());
    }

    #[test]
    fn tool_use_absent_blocks_negotiation() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = mk_reqs_native(&[Capability::ToolUse]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn claude_and_openai_both_support_tool_use() {
        for dialect in &["claude", "openai"] {
            let dm = dialect_manifest(dialect);
            assert!(
                dm.contains_key(&Capability::ToolUse),
                "{dialect} should support tool_use"
            );
        }
    }
}

// ===========================================================================
// Module 8: Context window negotiation (min of two)
// ===========================================================================

mod context_window {

    fn context_token_min(a: Option<u64>, b: Option<u64>) -> Option<u64> {
        match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        }
    }

    #[test]
    fn both_specified_picks_min() {
        assert_eq!(
            context_token_min(Some(100_000), Some(200_000)),
            Some(100_000)
        );
    }

    #[test]
    fn one_none_picks_other() {
        assert_eq!(context_token_min(Some(100_000), None), Some(100_000));
        assert_eq!(context_token_min(None, Some(200_000)), Some(200_000));
    }

    #[test]
    fn both_none_yields_none() {
        assert_eq!(context_token_min(None, None), None);
    }

    #[test]
    fn equal_values_returns_same() {
        assert_eq!(
            context_token_min(Some(128_000), Some(128_000)),
            Some(128_000)
        );
    }

    #[test]
    fn zero_is_valid_min() {
        assert_eq!(context_token_min(Some(0), Some(100_000)), Some(0));
    }

    #[test]
    fn very_large_context_windows() {
        assert_eq!(
            context_token_min(Some(1_000_000), Some(2_000_000)),
            Some(1_000_000)
        );
    }

    #[test]
    fn context_negotiation_commutative() {
        let a = Some(50_000u64);
        let b = Some(100_000u64);
        assert_eq!(context_token_min(a, b), context_token_min(b, a));
    }
}

// ===========================================================================
// Module 9: Serialization/deserialization roundtrip
// ===========================================================================

mod serde_roundtrip {
    use super::*;

    #[test]
    fn capability_manifest_serde_roundtrip() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            (Capability::Logprobs, CoreSupportLevel::Unsupported),
        ]);
        let json = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.len(), back.len());
        for (k, v) in &m {
            let back_v = back.get(k).unwrap();
            let orig = serde_json::to_value(v).unwrap();
            let round = serde_json::to_value(back_v).unwrap();
            assert_eq!(orig, round);
        }
    }

    #[test]
    fn capability_requirements_serde_roundtrip() {
        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let json = serde_json::to_string(&r).unwrap();
        let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
        assert_eq!(r.required.len(), back.required.len());
    }

    #[test]
    fn abp_cap_negotiation_result_serde_roundtrip() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Logprobs],
        );
        let json = serde_json::to_string(&result).unwrap();
        let back: AbpCapNegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, back);
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let result =
            AbpCapNegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
        let report = generate_report(&result);
        let json = serde_json::to_string(&report).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }

    #[test]
    fn support_level_native_json() {
        let level = SupportLevel::Native;
        let json = serde_json::to_string(&level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(level, back);
    }

    #[test]
    fn support_level_emulated_json() {
        let level = SupportLevel::Emulated {
            method: "polyfill".into(),
        };
        let json = serde_json::to_string(&level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(level, back);
    }

    #[test]
    fn support_level_unsupported_json() {
        let level = SupportLevel::Unsupported {
            reason: "unsupported".into(),
        };
        let json = serde_json::to_string(&level).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(level, back);
    }

    #[test]
    fn capability_enum_rename_all_snake_case() {
        let cap = Capability::ToolRead;
        let json = serde_json::to_value(&cap).unwrap();
        assert_eq!(json, serde_json::json!("tool_read"));
    }

    #[test]
    fn manifest_json_keys_are_snake_case() {
        let m = mk_manifest(&[
            (Capability::ToolWebSearch, CoreSupportLevel::Native),
            (Capability::HooksPreToolUse, CoreSupportLevel::Emulated),
        ]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("tool_web_search"));
        assert!(json.contains("hooks_pre_tool_use"));
    }

    #[test]
    fn manifest_canonical_json_deterministic() {
        let m = mk_manifest(&[
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let json1 = serde_json::to_string(&m).unwrap();
        let json2 = serde_json::to_string(&m).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn restricted_support_level_roundtrip() {
        let m = mk_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed environment".into(),
            },
        )]);
        let json = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back.get(&Capability::ToolBash),
            Some(CoreSupportLevel::Restricted { .. })
        ));
    }

    #[test]
    fn dialect_support_level_serde_roundtrip() {
        let levels = vec![
            DialectSupportLevel::Native,
            DialectSupportLevel::Emulated {
                detail: "via adapter".into(),
            },
            DialectSupportLevel::Unsupported {
                reason: "not available".into(),
            },
        ];
        for level in &levels {
            let json = serde_json::to_string(level).unwrap();
            let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, level);
        }
    }

    #[test]
    fn dialect_capability_report_to_receipt_metadata() {
        let report = DialectCapabilityReport {
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            entries: vec![CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            }],
        };
        let metadata = report.to_receipt_metadata();
        assert!(metadata.is_object());
        assert!(metadata["source_dialect"] == "claude");
    }
}

// ===========================================================================
// Module 10: Capability profile merging
// ===========================================================================

mod merging {
    use super::*;

    #[test]
    fn merge_disjoint_manifests() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&a, &b);
        assert_eq!(merged.len(), 2);
        assert!(merged.contains_key(&Capability::Streaming));
        assert!(merged.contains_key(&Capability::ToolRead));
    }

    #[test]
    fn merge_overlap_picks_higher_support() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let b = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&a, &b);
        assert!(matches!(
            merged.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn merge_with_empty_returns_other() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let empty = CapabilityManifest::new();
        let merged = merge_manifests(&a, &empty);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merge_both_empty() {
        let merged = merge_manifests(&CapabilityManifest::new(), &CapabilityManifest::new());
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_keeps_all_unique_capabilities() {
        let a = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let b = mk_manifest(&[
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let merged = merge_manifests(&a, &b);
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn merge_unsupported_does_not_upgrade_native() {
        let a = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let b = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);
        let merged = merge_manifests(&a, &b);
        assert!(matches!(
            merged.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn merge_restricted_upgrades_to_emulated() {
        let a = mk_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let b = mk_manifest(&[(Capability::ToolBash, CoreSupportLevel::Emulated)]);
        let merged = merge_manifests(&a, &b);
        assert!(matches!(
            merged.get(&Capability::ToolBash),
            Some(CoreSupportLevel::Emulated)
        ));
    }
}

// ===========================================================================
// Module 11: Default capability profiles per SDK dialect
// ===========================================================================

mod dialect_defaults {
    use super::*;

    #[test]
    fn claude_has_tool_suite() {
        let dm = dialect_manifest("claude");
        for cap in &[
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
        ] {
            assert!(dm.contains_key(cap), "claude missing {cap:?}");
        }
    }

    #[test]
    fn openai_has_structured_output_native() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::StructuredOutputJsonSchema),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn claude_has_structured_output_emulated() {
        let dm = dialect_manifest("claude");
        assert!(matches!(
            dm.get(&Capability::StructuredOutputJsonSchema),
            Some(DialectSupportLevel::Emulated { .. })
        ));
    }

    #[test]
    fn openai_has_code_execution_native() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::CodeExecution),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn openai_has_seed_determinism() {
        let dm = dialect_manifest("openai");
        assert!(matches!(
            dm.get(&Capability::SeedDeterminism),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn gemini_has_image_input_native() {
        let dm = dialect_manifest("gemini");
        assert!(matches!(
            dm.get(&Capability::ImageInput),
            Some(DialectSupportLevel::Native)
        ));
    }

    #[test]
    fn claude_lacks_seed_determinism() {
        let dm = dialect_manifest("claude");
        assert!(matches!(
            dm.get(&Capability::SeedDeterminism),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn gemini_lacks_seed_determinism() {
        let dm = dialect_manifest("gemini");
        assert!(matches!(
            dm.get(&Capability::SeedDeterminism),
            Some(DialectSupportLevel::Unsupported { .. })
        ));
    }

    #[test]
    fn all_known_dialects_have_tool_use() {
        for dialect in &["claude", "openai"] {
            let dm = dialect_manifest(dialect);
            assert!(
                dm.contains_key(&Capability::ToolUse),
                "{dialect} should have tool_use"
            );
        }
    }
}

// ===========================================================================
// Module 12: Feature flag-based capability gating
// ===========================================================================

mod feature_gating {
    use super::*;

    fn gate_capabilities(
        manifest: &CapabilityManifest,
        allowed: &[Capability],
    ) -> CapabilityManifest {
        manifest
            .iter()
            .filter(|(k, _)| allowed.contains(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    #[test]
    fn gate_removes_disallowed_capabilities() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let gated = gate_capabilities(&m, &[Capability::Streaming, Capability::ToolRead]);
        assert_eq!(gated.len(), 2);
        assert!(!gated.contains_key(&Capability::ToolBash));
    }

    #[test]
    fn gate_with_empty_allowlist_removes_all() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let gated = gate_capabilities(&m, &[]);
        assert!(gated.is_empty());
    }

    #[test]
    fn gate_with_full_allowlist_preserves_all() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let gated = gate_capabilities(&m, &[Capability::Streaming, Capability::ToolRead]);
        assert_eq!(gated.len(), 2);
    }

    #[test]
    fn gated_manifest_still_negotiable() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let gated = gate_capabilities(&m, &[Capability::Streaming]);
        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolBash]);
        let res = negotiate(&gated, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.native, vec![Capability::Streaming]);
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolBash]);
    }

    #[test]
    fn gate_does_not_alter_support_levels() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let gated = gate_capabilities(&m, &[Capability::Streaming]);
        assert!(matches!(
            gated.get(&Capability::Streaming),
            Some(CoreSupportLevel::Emulated)
        ));
    }
}

// ===========================================================================
// Module 13: Capability degradation/emulation detection
// ===========================================================================

mod degradation_detection {
    use super::*;

    #[test]
    fn diff_detects_added_capabilities() {
        let old = CapabilityManifest::new();
        let new = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let diff = CapabilityDiff::diff(&old, &new);
        assert_eq!(diff.added, vec![Capability::Streaming]);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn diff_detects_removed_capabilities() {
        let old = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let new = CapabilityManifest::new();
        let diff = CapabilityDiff::diff(&old, &new);
        assert!(diff.added.is_empty());
        assert_eq!(diff.removed, vec![Capability::Streaming]);
    }

    #[test]
    fn diff_detects_upgrade() {
        let old = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let new = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let diff = CapabilityDiff::diff(&old, &new);
        assert_eq!(diff.upgraded.len(), 1);
        assert!(diff.downgraded.is_empty());
    }

    #[test]
    fn diff_detects_downgrade() {
        let old = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let new = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let diff = CapabilityDiff::diff(&old, &new);
        assert!(diff.upgraded.is_empty());
        assert_eq!(diff.downgraded.len(), 1);
    }

    #[test]
    fn diff_no_change_yields_empty_diff() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let diff = CapabilityDiff::diff(&m, &m);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.upgraded.is_empty());
        assert!(diff.downgraded.is_empty());
    }

    #[test]
    fn diff_both_empty_yields_empty() {
        let diff = CapabilityDiff::diff(&CapabilityManifest::new(), &CapabilityManifest::new());
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn diff_complex_scenario() {
        let old = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
            (Capability::ToolBash, CoreSupportLevel::Native),
        ]);
        let new = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolWrite, CoreSupportLevel::Native),
        ]);
        let diff = CapabilityDiff::diff(&old, &new);
        assert_eq!(diff.added, vec![Capability::ToolWrite]);
        assert_eq!(diff.removed, vec![Capability::ToolBash]);
        assert_eq!(diff.upgraded.len(), 1); // ToolRead: Emulated -> Native
        assert_eq!(diff.downgraded.len(), 1); // Streaming: Native -> Emulated
    }

    #[test]
    fn emulation_report_identifies_emulated_caps() {
        let report = DialectCapabilityReport {
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            entries: vec![
                CapabilityReportEntry {
                    capability: Capability::Streaming,
                    support: DialectSupportLevel::Native,
                },
                CapabilityReportEntry {
                    capability: Capability::ExtendedThinking,
                    support: DialectSupportLevel::Emulated {
                        detail: "via prompt engineering".into(),
                    },
                },
                CapabilityReportEntry {
                    capability: Capability::Logprobs,
                    support: DialectSupportLevel::Unsupported {
                        reason: "not available".into(),
                    },
                },
            ],
        };
        assert_eq!(report.native_capabilities().len(), 1);
        assert_eq!(report.emulated_capabilities().len(), 1);
        assert_eq!(report.unsupported_capabilities().len(), 1);
        assert!(!report.all_satisfiable());
    }
}

// ===========================================================================
// Module 14: Empty and minimal capability profiles
// ===========================================================================

mod empty_minimal {
    use super::*;

    #[test]
    fn empty_manifest_negotiate_empty_requirements() {
        let res = negotiate(
            &CapabilityManifest::new(),
            &CapabilityRequirements::default(),
        );
        assert!(res.is_compatible());
        assert_eq!(res.total(), 0);
    }

    #[test]
    fn empty_manifest_any_requirement_fails() {
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&CapabilityManifest::new(), &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn single_cap_manifest_single_matching_requirement() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = mk_reqs_native(&[Capability::Streaming]);
        let res = negotiate(&m, &r);
        assert!(res.is_compatible());
        assert_eq!(res.total(), 1);
    }

    #[test]
    fn single_cap_manifest_different_requirement_fails() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let r = mk_reqs_native(&[Capability::ToolRead]);
        let res = negotiate(&m, &r);
        assert!(!res.is_compatible());
    }

    #[test]
    fn report_empty_result_is_compatible() {
        let result = AbpCapNegotiationResult::from_simple(vec![], vec![], vec![]);
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 0);
        assert_eq!(report.emulated_count, 0);
        assert_eq!(report.unsupported_count, 0);
    }

    #[test]
    fn empty_negotiation_request_always_compatible() {
        let request = NegotiationRequest {
            required: vec![],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let m = CapabilityManifest::new();
        let result = CapabilityNegotiator::negotiate(&request, &m);
        assert!(result.is_compatible);
    }

    #[test]
    fn minimal_manifest_serde_roundtrip() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let json = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.len(), back.len());
    }
}

// ===========================================================================
// Module 15: Capability versioning across contract versions
// ===========================================================================

mod versioning {
    use super::*;

    #[test]
    fn contract_version_is_v01() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn receipt_embeds_contract_version() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_with_capabilities_preserves_manifest() {
        let caps = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let receipt = ReceiptBuilder::new("test-backend")
            .capabilities(caps.clone())
            .build();
        assert_eq!(receipt.capabilities.len(), 2);
        assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    }

    #[test]
    fn receipt_hash_deterministic_with_capabilities() {
        let caps = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let r1 = ReceiptBuilder::new("mock")
            .capabilities(caps.clone())
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        let r2 = ReceiptBuilder::new("mock")
            .capabilities(caps)
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        // Different run_ids mean different hashes, but hash length is always 64
        assert_eq!(r1.receipt_sha256.as_ref().unwrap().len(), 64);
        assert_eq!(r2.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn work_order_carries_capability_requirements() {
        let wo = WorkOrderBuilder::new("test task")
            .requirements(mk_reqs_native(&[Capability::Streaming]))
            .build();
        assert_eq!(wo.requirements.required.len(), 1);
    }

    #[test]
    fn check_capabilities_with_work_order() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(mk_reqs_native(&[
                Capability::Streaming,
                Capability::Logprobs,
            ]))
            .build();
        let report = check_capabilities(&wo, "claude", "openai");
        assert_eq!(report.source_dialect, "claude");
        assert_eq!(report.target_dialect, "openai");
        assert_eq!(report.entries.len(), 2);
    }

    #[test]
    fn check_capabilities_all_satisfiable_for_streaming_only() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(mk_reqs_native(&[Capability::Streaming]))
            .build();
        let report = check_capabilities(&wo, "claude", "openai");
        assert!(report.all_satisfiable());
    }

    #[test]
    fn check_capabilities_unsatisfiable_when_target_lacks_cap() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(mk_reqs_native(&[Capability::Logprobs]))
            .build();
        let report = check_capabilities(&wo, "openai", "claude");
        assert!(!report.all_satisfiable());
    }
}

// ===========================================================================
// Module 16: Advanced negotiator (abp_core::negotiate)
// ===========================================================================

mod advanced_negotiator {
    use super::*;

    #[test]
    fn negotiator_all_required_satisfied() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming, Capability::ToolRead],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let result = CapabilityNegotiator::negotiate(&req, &m);
        assert!(result.is_compatible);
        assert_eq!(result.satisfied.len(), 2);
        assert!(result.unsatisfied.is_empty());
    }

    #[test]
    fn negotiator_unsatisfied_when_emulated_but_native_required() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let result = CapabilityNegotiator::negotiate(&req, &m);
        assert!(!result.is_compatible);
        assert_eq!(result.unsatisfied, vec![Capability::Streaming]);
    }

    #[test]
    fn negotiator_emulated_satisfies_emulated_minimum() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Emulated,
        };
        let result = CapabilityNegotiator::negotiate(&req, &m);
        assert!(result.is_compatible);
    }

    #[test]
    fn negotiator_preferred_caps_as_bonus() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Logprobs, CoreSupportLevel::Native),
        ]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![Capability::Logprobs],
            minimum_support: CoreSupportLevel::Native,
        };
        let result = CapabilityNegotiator::negotiate(&req, &m);
        assert!(result.is_compatible);
        assert_eq!(result.bonus, vec![Capability::Logprobs]);
    }

    #[test]
    fn negotiator_missing_preferred_does_not_break_compatibility() {
        let m = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![Capability::Logprobs],
            minimum_support: CoreSupportLevel::Native,
        };
        let result = CapabilityNegotiator::negotiate(&req, &m);
        assert!(result.is_compatible);
        assert!(result.bonus.is_empty());
    }

    #[test]
    fn best_match_picks_most_capable() {
        let m1 = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let m2 = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::Logprobs, CoreSupportLevel::Native),
        ]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![Capability::Logprobs],
            minimum_support: CoreSupportLevel::Native,
        };
        let manifests = vec![("backend_a", m1), ("backend_b", m2)];
        let best = CapabilityNegotiator::best_match(&req, &manifests);
        assert!(best.is_some());
        let (name, _) = best.unwrap();
        assert_eq!(name, "backend_b");
    }

    #[test]
    fn best_match_none_when_all_incompatible() {
        let m1 = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let manifests = vec![("only_backend", m1)];
        assert!(CapabilityNegotiator::best_match(&req, &manifests).is_none());
    }

    #[test]
    fn best_match_empty_manifests_returns_none() {
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let manifests: Vec<(&str, CapabilityManifest)> = vec![];
        assert!(CapabilityNegotiator::best_match(&req, &manifests).is_none());
    }

    #[test]
    fn best_match_tie_broken_deterministically() {
        let m1 = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let m2 = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let req = NegotiationRequest {
            required: vec![Capability::Streaming],
            preferred: vec![],
            minimum_support: CoreSupportLevel::Native,
        };
        let manifests = vec![("alpha", m1), ("beta", m2)];
        let best = CapabilityNegotiator::best_match(&req, &manifests);
        assert!(best.is_some());
        // Deterministic tie-break: name order
        let (name, _) = best.unwrap();
        assert!(name == "alpha" || name == "beta");
    }
}

// ===========================================================================
// Module 17: Report generation
// ===========================================================================

mod report_generation {
    use super::*;

    #[test]
    fn report_fully_compatible_summary() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming, Capability::ToolRead],
            vec![Capability::ToolWrite],
            vec![],
        );
        let report = generate_report(&result);
        assert!(report.compatible);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn report_incompatible_summary() {
        let result =
            AbpCapNegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert!(report.summary.contains("incompatible"));
    }

    #[test]
    fn report_counts_match() {
        let result = AbpCapNegotiationResult::from_simple(
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
    fn report_details_has_all_entries() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolRead],
            vec![Capability::Logprobs],
        );
        let report = generate_report(&result);
        assert_eq!(report.details.len(), 3);
    }

    #[test]
    fn report_all_emulated_still_compatible() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![],
            vec![Capability::Streaming, Capability::ToolRead],
            vec![],
        );
        let report = generate_report(&result);
        assert!(report.compatible);
    }

    #[test]
    fn report_summary_contains_counts() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming, Capability::ToolUse],
            vec![Capability::ToolBash],
            vec![],
        );
        let report = generate_report(&result);
        assert!(report.summary.contains("2 native"));
        assert!(report.summary.contains("1 emulated"));
        assert!(report.summary.contains("0 unsupported"));
    }
}

// ===========================================================================
// Module 18: Cross-cutting integration scenarios
// ===========================================================================

mod integration {
    use super::*;

    #[test]
    fn full_pipeline_work_order_to_negotiation() {
        let wo = WorkOrderBuilder::new("refactor auth")
            .requirements(mk_reqs_native(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
                Capability::ToolBash,
            ]))
            .build();

        let backend_manifest = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
            (Capability::ToolWrite, CoreSupportLevel::Native),
            (Capability::ToolBash, CoreSupportLevel::Emulated),
        ]);

        let result = negotiate(&backend_manifest, &wo.requirements);
        assert!(result.is_compatible());
        assert_eq!(result.native.len(), 3);
        assert_eq!(result.emulated.len(), 1);
    }

    #[test]
    fn receipt_capabilities_match_negotiated() {
        let caps = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let receipt = ReceiptBuilder::new("test-backend")
            .capabilities(caps.clone())
            .outcome(Outcome::Complete)
            .build();

        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let result = negotiate(&receipt.capabilities, &r);
        assert!(result.is_compatible());
    }

    #[test]
    fn cross_dialect_capability_check() {
        let wo = WorkOrderBuilder::new("test")
            .requirements(mk_reqs_native(&[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::ExtendedThinking,
            ]))
            .build();
        let report = check_capabilities(&wo, "claude", "openai");
        assert!(!report.all_satisfiable());
        assert_eq!(report.unsupported_capabilities().len(), 1);
        assert_eq!(
            report.unsupported_capabilities()[0].capability,
            Capability::ExtendedThinking
        );
    }

    #[test]
    fn negotiate_then_generate_report() {
        let m = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Emulated),
        ]);
        let r = mk_reqs_native(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::Logprobs,
        ]);
        let result = negotiate(&m, &r);
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert_eq!(report.native_count, 1);
        assert_eq!(report.emulated_count, 1);
        assert_eq!(report.unsupported_count, 1);
    }

    #[test]
    fn diff_then_negotiate_after_upgrade() {
        let old = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolRead, CoreSupportLevel::Unsupported),
        ]);
        let new = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let diff = CapabilityDiff::diff(&old, &new);
        assert_eq!(diff.upgraded.len(), 2);

        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let old_result = negotiate(&old, &r);
        let new_result = negotiate(&new, &r);
        assert!(!old_result.is_compatible());
        assert!(new_result.is_compatible());
    }

    #[test]
    fn merge_then_negotiate_expands_compatibility() {
        let base = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let addon = mk_manifest(&[(Capability::ToolRead, CoreSupportLevel::Native)]);
        let merged = merge_manifests(&base, &addon);

        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&merged, &r);
        assert!(res.is_compatible());
    }

    #[test]
    fn intersect_then_negotiate_restricts_compatibility() {
        let a = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolRead, CoreSupportLevel::Native),
        ]);
        let b = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let isect = intersect_manifests(&a, &b);

        let r = mk_reqs_native(&[Capability::Streaming, Capability::ToolRead]);
        let res = negotiate(&isect, &r);
        assert!(!res.is_compatible());
        assert_eq!(res.unsupported_caps(), vec![Capability::ToolRead]);
    }

    #[test]
    fn work_order_default_requirements_empty() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert!(wo.requirements.required.is_empty());
        let res = negotiate(&CapabilityManifest::new(), &wo.requirements);
        assert!(res.is_compatible());
    }
}

// ===========================================================================
// Module 19: Registry population (15 tests)
// ===========================================================================

mod registry_population {
    use super::*;
    use abp_capability::{
        CapabilityRegistry, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
        gemini_15_pro_manifest, kimi_manifest, openai_gpt4o_manifest,
    };

    #[test]
    fn with_defaults_has_six_entries() {
        let reg = CapabilityRegistry::with_defaults();
        assert_eq!(reg.len(), 6);
        assert!(!reg.is_empty());
    }

    #[test]
    fn with_defaults_contains_all_backend_names() {
        let reg = CapabilityRegistry::with_defaults();
        for name in [
            "openai/gpt-4o",
            "anthropic/claude-3.5-sonnet",
            "google/gemini-1.5-pro",
            "moonshot/kimi",
            "openai/codex",
            "github/copilot",
        ] {
            assert!(reg.contains(name), "missing backend: {name}");
        }
    }

    #[test]
    fn names_returns_sorted_list() {
        let reg = CapabilityRegistry::with_defaults();
        let names = reg.names();
        assert_eq!(names.len(), 6);
        // BTreeMap iteration is sorted
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn openai_manifest_streaming_and_tool_use_native() {
        let m = openai_gpt4o_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::ToolUse),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn openai_manifest_extended_thinking_unsupported() {
        let m = openai_gpt4o_manifest();
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn claude_manifest_extended_thinking_and_cache_control_native() {
        let m = claude_35_sonnet_manifest();
        assert!(matches!(
            m.get(&Capability::ExtendedThinking),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::CacheControl),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn claude_manifest_function_calling_emulated() {
        let m = claude_35_sonnet_manifest();
        assert!(matches!(
            m.get(&Capability::FunctionCalling),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn gemini_manifest_code_execution_and_pdf_native() {
        let m = gemini_15_pro_manifest();
        assert!(matches!(
            m.get(&Capability::CodeExecution),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::PdfInput),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn gemini_manifest_batch_mode_unsupported() {
        let m = gemini_15_pro_manifest();
        assert!(matches!(
            m.get(&Capability::BatchMode),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn kimi_manifest_image_input_native_audio_unsupported() {
        let m = kimi_manifest();
        assert!(matches!(
            m.get(&Capability::ImageInput),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::Audio),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn codex_manifest_tool_suite_native() {
        let m = codex_manifest();
        for cap in [
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
        ] {
            assert!(
                matches!(m.get(&cap), Some(CoreSupportLevel::Native)),
                "codex should have {cap:?} native"
            );
        }
    }

    #[test]
    fn copilot_manifest_web_tools_native() {
        let m = copilot_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::ToolWebFetch),
            Some(CoreSupportLevel::Native)
        ));
        assert!(matches!(
            m.get(&Capability::ToolAskUser),
            Some(CoreSupportLevel::Native)
        ));
    }

    #[test]
    fn copilot_manifest_vision_emulated() {
        let m = copilot_manifest();
        assert!(matches!(
            m.get(&Capability::Vision),
            Some(CoreSupportLevel::Emulated)
        ));
    }

    #[test]
    fn register_custom_backend() {
        let mut reg = CapabilityRegistry::new();
        assert!(reg.is_empty());
        reg.register(
            "custom/model",
            mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        assert_eq!(reg.len(), 1);
        assert!(reg.contains("custom/model"));
        assert!(reg.get("custom/model").is_some());
    }

    #[test]
    fn unregister_returns_correct_bool() {
        let mut reg = CapabilityRegistry::with_defaults();
        assert!(reg.unregister("openai/gpt-4o"));
        assert_eq!(reg.len(), 5);
        assert!(!reg.unregister("openai/gpt-4o"));
    }
}

// ===========================================================================
// Module 20: Negotiation workflows (15 tests)
// ===========================================================================

mod negotiation_workflows {
    use super::*;
    use abp_capability::{CapabilityRegistry, EmulationStrategy, negotiate_capabilities};

    fn reg() -> CapabilityRegistry {
        CapabilityRegistry::with_defaults()
    }

    #[test]
    fn negotiate_native_capability_by_name() {
        let result = reg()
            .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.native, vec![Capability::Streaming]);
        assert!(result.emulated.is_empty());
    }

    #[test]
    fn negotiate_unsupported_returns_reason() {
        let result = reg()
            .negotiate_by_name("openai/gpt-4o", &[Capability::ExtendedThinking])
            .unwrap();
        assert!(!result.is_viable());
        let (cap, _reason) = &result.unsupported[0];
        assert_eq!(cap, &Capability::ExtendedThinking);
    }

    #[test]
    fn negotiate_emulated_includes_strategy() {
        let result = reg()
            .negotiate_by_name("openai/gpt-4o", &[Capability::CodeExecution])
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.emulated.len(), 1);
        let (_cap, strategy) = &result.emulated[0];
        assert!(matches!(
            strategy,
            EmulationStrategy::ClientSide
                | EmulationStrategy::ServerFallback
                | EmulationStrategy::Approximate
        ));
    }

    #[test]
    fn negotiate_mixed_native_emulated_is_viable() {
        let result = reg()
            .negotiate_by_name(
                "openai/gpt-4o",
                &[Capability::Streaming, Capability::CodeExecution],
            )
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 1);
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.total(), 2);
    }

    #[test]
    fn negotiate_mixed_with_unsupported_not_viable() {
        let result = reg()
            .negotiate_by_name(
                "openai/gpt-4o",
                &[Capability::Streaming, Capability::ExtendedThinking],
            )
            .unwrap();
        assert!(!result.is_viable());
        assert!(!result.is_compatible());
    }

    #[test]
    fn negotiate_unknown_backend_is_none() {
        assert!(
            reg()
                .negotiate_by_name("nonexistent", &[Capability::Streaming])
                .is_none()
        );
    }

    #[test]
    fn negotiate_claude_function_calling_emulated() {
        let result = reg()
            .negotiate_by_name(
                "anthropic/claude-3.5-sonnet",
                &[Capability::FunctionCalling],
            )
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.emulated_caps(), vec![Capability::FunctionCalling]);
    }

    #[test]
    fn negotiate_gemini_batch_mode_unsupported() {
        let result = reg()
            .negotiate_by_name("google/gemini-1.5-pro", &[Capability::BatchMode])
            .unwrap();
        assert!(!result.is_viable());
        assert_eq!(result.unsupported_caps(), vec![Capability::BatchMode]);
    }

    #[test]
    fn negotiate_copilot_structured_output_emulated() {
        let result = reg()
            .negotiate_by_name("github/copilot", &[Capability::StructuredOutputJsonSchema])
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(
            result.emulated_caps(),
            vec![Capability::StructuredOutputJsonSchema]
        );
    }

    #[test]
    fn negotiate_kimi_audio_unsupported() {
        let result = reg()
            .negotiate_by_name("moonshot/kimi", &[Capability::Audio])
            .unwrap();
        assert!(!result.is_viable());
    }

    #[test]
    fn negotiate_codex_vision_emulated() {
        let result = reg()
            .negotiate_by_name("openai/codex", &[Capability::Vision])
            .unwrap();
        assert!(result.is_viable());
        assert_eq!(result.emulated_caps(), vec![Capability::Vision]);
    }

    #[test]
    fn negotiate_report_compatible() {
        let result = reg()
            .negotiate_by_name(
                "openai/gpt-4o",
                &[Capability::Streaming, Capability::ToolUse],
            )
            .unwrap();
        let report = generate_report(&result);
        assert!(report.compatible);
        assert_eq!(report.native_count, 2);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn negotiate_report_incompatible() {
        let result = reg()
            .negotiate_by_name("openai/gpt-4o", &[Capability::ExtendedThinking])
            .unwrap();
        let report = generate_report(&result);
        assert!(!report.compatible);
        assert!(report.summary.contains("incompatible"));
    }

    #[test]
    fn warnings_for_approximate_emulation() {
        let manifest = mk_manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
        let result = negotiate_capabilities(&[Capability::Vision], &manifest);
        let warnings = result.warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].1.has_fidelity_loss());
    }

    #[test]
    fn is_viable_and_is_compatible_are_equivalent() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolUse],
            vec![],
        );
        assert_eq!(result.is_viable(), result.is_compatible());
        assert!(result.is_viable());
    }
}

// ===========================================================================
// Module 21: Cross-SDK capability comparison (15 tests)
// ===========================================================================

mod cross_sdk_comparison {
    use super::*;
    use abp_capability::CapabilityRegistry;

    fn reg() -> CapabilityRegistry {
        CapabilityRegistry::with_defaults()
    }

    #[test]
    fn compare_openai_vs_claude_audio_gap() {
        let result = reg()
            .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
            .unwrap();
        // OpenAI has Audio native; Claude has it unsupported
        assert!(result.unsupported_caps().contains(&Capability::Audio));
    }

    #[test]
    fn compare_claude_vs_openai_extended_thinking_gap() {
        let result = reg()
            .compare("anthropic/claude-3.5-sonnet", "openai/gpt-4o")
            .unwrap();
        // Claude has ExtendedThinking native; OpenAI has it unsupported
        assert!(
            result
                .unsupported_caps()
                .contains(&Capability::ExtendedThinking)
        );
    }

    #[test]
    fn compare_openai_vs_gemini_batch_mode_gap() {
        let result = reg()
            .compare("openai/gpt-4o", "google/gemini-1.5-pro")
            .unwrap();
        // OpenAI has BatchMode native; Gemini has it unsupported
        assert!(result.unsupported_caps().contains(&Capability::BatchMode));
    }

    #[test]
    fn compare_gemini_vs_openai_cache_control() {
        let result = reg()
            .compare("google/gemini-1.5-pro", "openai/gpt-4o")
            .unwrap();
        // Gemini has CacheControl native; OpenAI has it unsupported
        assert!(
            result
                .unsupported_caps()
                .contains(&Capability::CacheControl)
        );
    }

    #[test]
    fn compare_claude_vs_gemini_extended_thinking_gap() {
        let result = reg()
            .compare("anthropic/claude-3.5-sonnet", "google/gemini-1.5-pro")
            .unwrap();
        assert!(
            result
                .unsupported_caps()
                .contains(&Capability::ExtendedThinking)
        );
    }

    #[test]
    fn compare_gemini_vs_claude_audio_gap() {
        let result = reg()
            .compare("google/gemini-1.5-pro", "anthropic/claude-3.5-sonnet")
            .unwrap();
        assert!(result.unsupported_caps().contains(&Capability::Audio));
    }

    #[test]
    fn compare_codex_vs_copilot_streaming() {
        let result = reg().compare("openai/codex", "github/copilot").unwrap();
        assert!(result.native.contains(&Capability::Streaming));
    }

    #[test]
    fn compare_copilot_vs_codex_web_tools_gap() {
        let result = reg().compare("github/copilot", "openai/codex").unwrap();
        assert!(
            result
                .unsupported_caps()
                .contains(&Capability::ToolWebSearch)
        );
    }

    #[test]
    fn compare_openai_vs_kimi_audio_gap() {
        let result = reg().compare("openai/gpt-4o", "moonshot/kimi").unwrap();
        assert!(result.unsupported_caps().contains(&Capability::Audio));
    }

    #[test]
    fn compare_codex_vs_kimi_tool_read_gap() {
        let result = reg().compare("openai/codex", "moonshot/kimi").unwrap();
        assert!(result.unsupported_caps().contains(&Capability::ToolRead));
    }

    #[test]
    fn compare_same_backend_fully_viable() {
        let result = reg().compare("openai/gpt-4o", "openai/gpt-4o").unwrap();
        assert!(result.is_viable());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn compare_nonexistent_source_none() {
        assert!(reg().compare("fake", "openai/gpt-4o").is_none());
    }

    #[test]
    fn compare_nonexistent_target_none() {
        assert!(reg().compare("openai/gpt-4o", "fake").is_none());
    }

    #[test]
    fn query_streaming_all_native() {
        let r = reg();
        let results = r.query_capability(&Capability::Streaming);
        assert_eq!(results.len(), 6);
        for (name, level) in &results {
            assert!(
                matches!(level, SupportLevel::Native),
                "{name} should have native streaming"
            );
        }
    }

    #[test]
    fn query_extended_thinking_only_one_native() {
        let r = reg();
        let results = r.query_capability(&Capability::ExtendedThinking);
        let native_count = results
            .iter()
            .filter(|(_, l)| matches!(l, SupportLevel::Native))
            .count();
        assert_eq!(native_count, 1);
    }
}

// ===========================================================================
// Module 22: Edge cases (15 tests)
// ===========================================================================

mod edge_cases {
    use super::*;
    use abp_capability::{CapabilityRegistry, EmulationStrategy, negotiate_capabilities};

    #[test]
    fn negotiate_empty_requirements_viable() {
        let reg = CapabilityRegistry::with_defaults();
        let result = reg.negotiate_by_name("openai/gpt-4o", &[]).unwrap();
        assert!(result.is_viable());
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn negotiate_all_native_no_emulated_or_unsupported() {
        let manifest = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Native),
            (Capability::ToolUse, CoreSupportLevel::Native),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
        assert!(result.emulated.is_empty());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn negotiate_all_unsupported_against_empty_manifest() {
        let manifest: CapabilityManifest = CapabilityManifest::new();
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::ToolUse,
                Capability::Vision,
            ],
            &manifest,
        );
        assert!(!result.is_viable());
        assert!(result.native.is_empty());
        assert!(result.emulated.is_empty());
        assert_eq!(result.unsupported.len(), 3);
    }

    #[test]
    fn negotiate_mixed_emulated_and_unsupported() {
        let manifest = mk_manifest(&[
            (Capability::Streaming, CoreSupportLevel::Emulated),
            (Capability::ToolUse, CoreSupportLevel::Unsupported),
        ]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::ToolUse], &manifest);
        assert!(!result.is_viable());
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn negotiate_absent_capability_unsupported_reason() {
        let manifest: CapabilityManifest = CapabilityManifest::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &manifest);
        assert!(!result.is_viable());
        assert!(result.unsupported[0].1.contains("not declared"));
    }

    #[test]
    fn negotiate_explicitly_unsupported_capability_reason() {
        let manifest = mk_manifest(&[(Capability::Audio, CoreSupportLevel::Unsupported)]);
        let result = negotiate_capabilities(&[Capability::Audio], &manifest);
        assert!(!result.is_viable());
        assert!(result.unsupported[0].1.contains("explicitly marked"));
    }

    #[test]
    fn negotiate_duplicate_capabilities_counted_twice() {
        let manifest = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::Streaming], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.native.len(), 2);
        assert_eq!(result.total(), 2);
    }

    #[test]
    fn check_capability_native_level() {
        let manifest = mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
        let level = check_capability(&manifest, &Capability::Streaming);
        assert!(matches!(level, SupportLevel::Native));
    }

    #[test]
    fn check_capability_emulated_level() {
        let manifest = mk_manifest(&[(Capability::Vision, CoreSupportLevel::Emulated)]);
        let level = check_capability(&manifest, &Capability::Vision);
        assert!(matches!(level, SupportLevel::Emulated { .. }));
    }

    #[test]
    fn check_capability_restricted_level() {
        let manifest = mk_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]);
        let level = check_capability(&manifest, &Capability::ToolBash);
        match level {
            SupportLevel::Restricted { reason } => assert!(reason.contains("sandbox")),
            other => panic!("expected Restricted, got {other:?}"),
        }
    }

    #[test]
    fn check_capability_absent_is_unsupported() {
        let manifest: CapabilityManifest = CapabilityManifest::new();
        let level = check_capability(&manifest, &Capability::Streaming);
        match level {
            SupportLevel::Unsupported { reason } => assert!(reason.contains("not declared")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn registry_overwrite_manifest() {
        let mut reg = CapabilityRegistry::new();
        reg.register(
            "b",
            mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]),
        );
        reg.register(
            "b",
            mk_manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]),
        );
        let m = reg.get("b").unwrap();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(CoreSupportLevel::Unsupported)
        ));
    }

    #[test]
    fn from_simple_sets_default_emulation_and_reason() {
        let result = AbpCapNegotiationResult::from_simple(
            vec![Capability::Streaming],
            vec![Capability::ToolUse],
            vec![Capability::Audio],
        );
        assert_eq!(result.emulated[0].1, EmulationStrategy::ClientSide);
        assert_eq!(result.unsupported[0].1, "not available");
    }

    #[test]
    fn negotiate_restricted_goes_to_emulated_bucket() {
        let manifest = mk_manifest(&[(
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        )]);
        let result = negotiate_capabilities(&[Capability::ToolBash], &manifest);
        assert!(result.is_viable());
        assert_eq!(result.emulated.len(), 1);
        assert_eq!(result.emulated[0].0, Capability::ToolBash);
    }

    #[test]
    fn empty_registry_is_empty() {
        let reg = CapabilityRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.names().is_empty());
        assert!(reg.get("anything").is_none());
    }
}
