// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the capability negotiation and registry system.
//!
//! Covers: `abp_capability` crate (SupportLevel, NegotiationResult, negotiate,
//! check_capability, generate_report, CompatibilityReport) and the advanced
//! negotiation types from `abp_core::negotiate` (CapabilityNegotiator,
//! CapabilityDiff, CapabilityReport, dialect manifests).

use abp_capability::{
    CompatibilityReport, NegotiationResult, SupportLevel, check_capability, generate_report,
    negotiate,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport as DialectCapabilityReport,
    CapabilityReportEntry, DialectSupportLevel, NegotiationRequest,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
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

/// All Capability variants for exhaustive testing.
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
// 1. CapabilityManifest (BTreeMap) construction
// ===========================================================================

#[test]
fn manifest_empty() {
    let m = CapabilityManifest::new();
    assert!(m.is_empty());
    assert_eq!(m.len(), 0);
}

#[test]
fn manifest_single_entry() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(m.len(), 1);
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn manifest_multiple_entries() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
    ]);
    assert_eq!(m.len(), 3);
}

#[test]
fn manifest_overwrite_key() {
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
fn manifest_deterministic_iteration_order() {
    let m = manifest(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    // BTreeMap: keys are in Ord order, which is derived
    let keys: Vec<_> = m.keys().collect();
    for i in 1..keys.len() {
        assert!(keys[i - 1] <= keys[i]);
    }
}

#[test]
fn manifest_all_capabilities() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    assert_eq!(m.len(), all_capabilities().len());
}

#[test]
fn manifest_remove_entry() {
    let mut m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    m.remove(&Capability::Streaming);
    assert_eq!(m.len(), 1);
    assert!(!m.contains_key(&Capability::Streaming));
}

// ===========================================================================
// 2. SupportLevel enum variants (abp_capability::SupportLevel)
// ===========================================================================

#[test]
fn support_level_native() {
    let l = SupportLevel::Native;
    assert!(matches!(l, SupportLevel::Native));
}

#[test]
fn support_level_emulated() {
    let l = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    assert!(matches!(l, SupportLevel::Emulated { .. }));
    if let SupportLevel::Emulated { method } = &l {
        assert_eq!(method, "polyfill");
    }
}

#[test]
fn support_level_unsupported() {
    let l = SupportLevel::Unsupported {
        reason: "unsupported".into(),
    };
    assert!(matches!(l, SupportLevel::Unsupported { .. }));
}

#[test]
fn support_level_equality() {
    assert_eq!(SupportLevel::Native, SupportLevel::Native);
    assert!(matches!(
        SupportLevel::Unsupported {
            reason: "unsupported".into()
        },
        SupportLevel::Unsupported { .. }
    ));
    assert_eq!(
        SupportLevel::Emulated { method: "x".into() },
        SupportLevel::Emulated { method: "x".into() }
    );
}

#[test]
fn support_level_inequality() {
    assert_ne!(
        SupportLevel::Native,
        SupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
    assert_ne!(
        SupportLevel::Emulated { method: "a".into() },
        SupportLevel::Emulated { method: "b".into() }
    );
}

#[test]
fn support_level_clone() {
    let l = SupportLevel::Emulated {
        method: "clone me".into(),
    };
    let l2 = l.clone();
    assert_eq!(l, l2);
}

#[test]
fn support_level_debug_format() {
    let l = SupportLevel::Native;
    let dbg = format!("{l:?}");
    assert!(dbg.contains("Native"));
}

// ===========================================================================
// 2b. CoreSupportLevel (abp_core::SupportLevel) — satisfies checks
// ===========================================================================

#[test]
fn core_support_native_satisfies_native() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn core_support_native_satisfies_emulated() {
    assert!(CoreSupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn core_support_emulated_does_not_satisfy_native() {
    assert!(!CoreSupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn core_support_emulated_satisfies_emulated() {
    assert!(CoreSupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn core_support_unsupported_never_satisfies() {
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!CoreSupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn core_support_restricted_satisfies_emulated() {
    let r = CoreSupportLevel::Restricted { reason: "x".into() };
    assert!(r.satisfies(&MinSupport::Emulated));
}

#[test]
fn core_support_restricted_does_not_satisfy_native() {
    let r = CoreSupportLevel::Restricted { reason: "x".into() };
    assert!(!r.satisfies(&MinSupport::Native));
}

// ===========================================================================
// 3. Capability negotiation (abp_capability::negotiate)
// ===========================================================================

#[test]
fn negotiate_all_native() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), 2);
    assert!(res.emulated.is_empty());
    assert!(res.unsupported.is_empty());
}

#[test]
fn negotiate_all_emulated() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Emulated),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), 2);
}

#[test]
fn negotiate_all_unsupported() {
    let m = CapabilityManifest::new();
    let r = reqs_native(&[Capability::Streaming, Capability::ToolRead]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), 2);
}

#[test]
fn negotiate_mixed_native_emulated_unsupported() {
    let m = manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let r = reqs_native(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.native, vec![Capability::Streaming]);
    assert_eq!(res.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolWrite]);
}

#[test]
fn negotiate_restricted_is_emulatable() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let r = reqs_native(&[Capability::ToolBash]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), 1);
}

#[test]
fn negotiate_explicit_unsupported_in_manifest() {
    let m = manifest(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    let r = reqs_native(&[Capability::Logprobs]);
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::Logprobs]);
}

#[test]
fn negotiate_empty_requirements() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = CapabilityRequirements::default();
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_empty_manifest_empty_requirements() {
    let res = negotiate(
        &CapabilityManifest::new(),
        &CapabilityRequirements::default(),
    );
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn negotiate_empty_manifest() {
    let res = negotiate(
        &CapabilityManifest::new(),
        &reqs_native(&[Capability::Streaming]),
    );
    assert!(!res.is_compatible());
}

#[test]
fn negotiate_preserves_requirement_order() {
    let m = manifest(&[
        (Capability::ToolWrite, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
        (Capability::Streaming, CoreSupportLevel::Native),
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

#[test]
fn negotiate_duplicates_preserved() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = reqs_native(&[Capability::Streaming, Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert_eq!(res.native.len(), 2);
}

#[test]
fn negotiate_large_manifest_small_requirements() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    let r = reqs_native(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.total(), 1);
}

#[test]
fn negotiate_single_native() {
    let m = manifest(&[(Capability::ToolUse, CoreSupportLevel::Native)]);
    let r = reqs_native(&[Capability::ToolUse]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native, vec![Capability::ToolUse]);
}

#[test]
fn negotiate_single_unsupported() {
    let res = negotiate(
        &CapabilityManifest::new(),
        &reqs_native(&[Capability::ToolUse]),
    );
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported_caps(), vec![Capability::ToolUse]);
}

// ===========================================================================
// 3b. check_capability
// ===========================================================================

#[test]
fn check_capability_native() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Native
    );
}

#[test]
fn check_capability_emulated() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Emulated {
            method: "adapter".into()
        }
    );
}

#[test]
fn check_capability_unsupported_explicit() {
    let m = manifest(&[(Capability::Logprobs, CoreSupportLevel::Unsupported)]);
    assert_eq!(
        check_capability(&m, &Capability::Logprobs),
        SupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
}

#[test]
fn check_capability_missing() {
    let m = CapabilityManifest::new();
    assert_eq!(
        check_capability(&m, &Capability::Streaming),
        SupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
}

#[test]
fn check_capability_restricted() {
    let m = manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "policy".into(),
        },
    )]);
    let level = check_capability(&m, &Capability::ToolBash);
    assert!(matches!(level, SupportLevel::Emulated { .. }));
    if let SupportLevel::Emulated { method } = &level {
        assert!(method.contains("restricted"));
        assert!(method.contains("policy"));
    }
}

#[test]
fn check_capability_all_core_variants() {
    let cases: Vec<(CoreSupportLevel, bool)> = vec![
        (CoreSupportLevel::Native, true),
        (CoreSupportLevel::Emulated, true),
        (CoreSupportLevel::Unsupported, false),
        (CoreSupportLevel::Restricted { reason: "x".into() }, true),
    ];
    for (level, should_be_supported) in cases {
        let m = manifest(&[(Capability::Streaming, level)]);
        let result = check_capability(&m, &Capability::Streaming);
        let supported = !matches!(result, SupportLevel::Unsupported { .. });
        assert_eq!(supported, should_be_supported);
    }
}

#[test]
fn check_capability_every_variant_against_empty_manifest() {
    let m = CapabilityManifest::new();
    for cap in all_capabilities() {
        assert!(matches!(
            check_capability(&m, &cap),
            SupportLevel::Unsupported { .. }
        ));
    }
}

// ===========================================================================
// 4. Capability negotiation: CapabilityNegotiator (abp_core::negotiate)
// ===========================================================================

fn core_manifest(entries: &[(Capability, CoreSupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

#[test]
fn negotiator_all_satisfied() {
    let m = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.satisfied.len(), 2);
    assert!(res.unsatisfied.is_empty());
}

#[test]
fn negotiator_none_satisfied() {
    let m = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!res.is_compatible);
    assert_eq!(res.unsatisfied, vec![Capability::Streaming]);
}

#[test]
fn negotiator_emulated_below_native_threshold() {
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!res.is_compatible);
}

#[test]
fn negotiator_emulated_meets_emulated_threshold() {
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Emulated,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
}

#[test]
fn negotiator_restricted_meets_restricted_threshold() {
    let m = core_manifest(&[(
        Capability::ToolBash,
        CoreSupportLevel::Restricted {
            reason: "policy".into(),
        },
    )]);
    let req = NegotiationRequest {
        required: vec![Capability::ToolBash],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Restricted {
            reason: String::new(),
        },
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
}

#[test]
fn negotiator_preferred_bonus() {
    let m = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Logprobs, CoreSupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Logprobs],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.bonus, vec![Capability::Logprobs]);
}

#[test]
fn negotiator_preferred_unsatisfied_not_in_bonus() {
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Logprobs],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert!(res.bonus.is_empty());
}

#[test]
fn negotiator_empty_requirements() {
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert!(res.satisfied.is_empty());
}

// ===========================================================================
// 4b. best_match
// ===========================================================================

#[test]
fn best_match_picks_compatible() {
    let good = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let bad = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&req, &[("good", good), ("bad", bad)]);
    assert!(result.is_some());
    let (name, _) = result.unwrap();
    assert_eq!(name, "good");
}

#[test]
fn best_match_none_compatible() {
    let bad = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&req, &[("bad", bad)]);
    assert!(result.is_none());
}

#[test]
fn best_match_prefers_more_bonus() {
    let a = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Logprobs, CoreSupportLevel::Native),
    ]);
    let b = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::Logprobs],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&req, &[("a", a), ("b", b)]);
    let (name, _) = result.unwrap();
    assert_eq!(name, "a");
}

#[test]
fn best_match_empty_manifests() {
    let req = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&req, &[]);
    assert!(result.is_none());
}

#[test]
fn best_match_empty_required_any_manifest_works() {
    let a = CapabilityManifest::new();
    let req = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Native,
    };
    let result = CapabilityNegotiator::best_match(&req, &[("a", a)]);
    assert!(result.is_some());
}

// ===========================================================================
// 4c. CapabilityDiff
// ===========================================================================

#[test]
fn diff_no_changes() {
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let d = CapabilityDiff::diff(&m, &m);
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.upgraded.is_empty());
    assert!(d.downgraded.is_empty());
}

#[test]
fn diff_added_capability() {
    let old = CapabilityManifest::new();
    let new = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.added, vec![Capability::Streaming]);
    assert!(d.removed.is_empty());
}

#[test]
fn diff_removed_capability() {
    let old = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let new = CapabilityManifest::new();
    let d = CapabilityDiff::diff(&old, &new);
    assert!(d.added.is_empty());
    assert_eq!(d.removed, vec![Capability::Streaming]);
}

#[test]
fn diff_upgrade() {
    let old = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let new = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.upgraded.len(), 1);
    assert!(d.downgraded.is_empty());
}

#[test]
fn diff_downgrade() {
    let old = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let new = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let d = CapabilityDiff::diff(&old, &new);
    assert!(d.upgraded.is_empty());
    assert_eq!(d.downgraded.len(), 1);
}

#[test]
fn diff_both_empty() {
    let d = CapabilityDiff::diff(&CapabilityManifest::new(), &CapabilityManifest::new());
    assert!(d.added.is_empty());
    assert!(d.removed.is_empty());
    assert!(d.upgraded.is_empty());
    assert!(d.downgraded.is_empty());
}

#[test]
fn diff_complex() {
    let old = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::ToolWrite, CoreSupportLevel::Native),
    ]);
    let new = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Emulated), // downgraded
        (Capability::ToolRead, CoreSupportLevel::Native),    // upgraded
        // ToolWrite removed
        (Capability::Logprobs, CoreSupportLevel::Native), // added
    ]);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.added, vec![Capability::Logprobs]);
    assert_eq!(d.removed, vec![Capability::ToolWrite]);
    assert_eq!(d.upgraded.len(), 1);
    assert_eq!(d.downgraded.len(), 1);
}

// ===========================================================================
// 5. Serde roundtrip
// ===========================================================================

#[test]
fn serde_support_level_native_roundtrip() {
    let l = SupportLevel::Native;
    let json = serde_json::to_string(&l).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

#[test]
fn serde_support_level_emulated_roundtrip() {
    let l = SupportLevel::Emulated {
        method: "polyfill".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

#[test]
fn serde_support_level_unsupported_roundtrip() {
    let l = SupportLevel::Unsupported {
        reason: "unsupported".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

#[test]
fn serde_negotiation_result_roundtrip() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, back);
}

#[test]
fn serde_negotiation_result_empty_roundtrip() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let json = serde_json::to_string(&result).unwrap();
    let back: NegotiationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(result, back);
}

#[test]
fn serde_compatibility_report_roundtrip() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}

#[test]
fn serde_compatibility_report_incompatible_roundtrip() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, back);
}

#[test]
fn serde_support_level_json_structure() {
    let l = SupportLevel::Native;
    let json = serde_json::to_string(&l).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["level"], "native");
}

#[test]
fn serde_support_level_emulated_json_structure() {
    let l = SupportLevel::Emulated {
        method: "adapter".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["level"], "emulated");
    assert_eq!(v["method"], "adapter");
}

#[test]
fn serde_all_capabilities_roundtrip() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }
}

#[test]
fn serde_capability_manifest_roundtrip() {
    let entries: Vec<_> = vec![
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
        (Capability::Logprobs, CoreSupportLevel::Unsupported),
        (
            Capability::ToolBash,
            CoreSupportLevel::Restricted {
                reason: "sandboxed".into(),
            },
        ),
    ];
    let m = manifest(&entries);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(m.len(), back.len());
    for (k, v) in &m {
        let back_v = back.get(k).unwrap();
        assert_eq!(format!("{v:?}"), format!("{back_v:?}"));
    }
}

#[test]
fn serde_dialect_support_level_native_roundtrip() {
    let l = DialectSupportLevel::Native;
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

#[test]
fn serde_dialect_support_level_emulated_roundtrip() {
    let l = DialectSupportLevel::Emulated {
        detail: "via tool".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

#[test]
fn serde_dialect_support_level_unsupported_roundtrip() {
    let l = DialectSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, back);
}

// ===========================================================================
// 6. Cross-backend capability comparison (dialect manifests)
// ===========================================================================

#[test]
fn dialect_manifest_claude_has_streaming() {
    let m = abp_core::negotiate::dialect_manifest("claude");
    assert!(m.contains_key(&Capability::Streaming));
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_openai_has_streaming() {
    let m = abp_core::negotiate::dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_gemini_has_streaming() {
    let m = abp_core::negotiate::dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_unknown_is_empty() {
    let m = abp_core::negotiate::dialect_manifest("unknown");
    assert!(m.is_empty());
}

#[test]
fn dialect_manifest_claude_no_logprobs() {
    let m = abp_core::negotiate::dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn dialect_manifest_openai_has_logprobs() {
    let m = abp_core::negotiate::dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_claude_has_extended_thinking() {
    let m = abp_core::negotiate::dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_openai_no_extended_thinking() {
    let m = abp_core::negotiate::dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn dialect_manifest_gemini_emulates_extended_thinking() {
    let m = abp_core::negotiate::dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn dialect_manifest_openai_has_seed_determinism() {
    let m = abp_core::negotiate::dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::SeedDeterminism),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_claude_no_seed_determinism() {
    let m = abp_core::negotiate::dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::SeedDeterminism),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn dialect_manifest_gemini_no_seed_determinism() {
    let m = abp_core::negotiate::dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::SeedDeterminism),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn dialect_manifests_are_non_empty_for_known_dialects() {
    for dialect in &["claude", "openai", "gemini"] {
        let m = abp_core::negotiate::dialect_manifest(dialect);
        assert!(!m.is_empty(), "{dialect} manifest should be non-empty");
    }
}

#[test]
fn cross_dialect_streaming_universally_native() {
    for dialect in &["claude", "openai", "gemini"] {
        let m = abp_core::negotiate::dialect_manifest(dialect);
        assert!(
            matches!(
                m.get(&Capability::Streaming),
                Some(DialectSupportLevel::Native)
            ),
            "{dialect} should natively support Streaming"
        );
    }
}

#[test]
fn cross_dialect_tool_use_universally_native() {
    for dialect in &["claude", "openai", "gemini"] {
        let m = abp_core::negotiate::dialect_manifest(dialect);
        assert!(
            matches!(
                m.get(&Capability::ToolUse),
                Some(DialectSupportLevel::Native)
            ),
            "{dialect} should natively support ToolUse"
        );
    }
}

// ===========================================================================
// 6b. DialectCapabilityReport
// ===========================================================================

#[test]
fn dialect_report_all_satisfiable() {
    let report = DialectCapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::ToolUse,
                support: DialectSupportLevel::Emulated {
                    detail: "polyfill".into(),
                },
            },
        ],
    };
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
    assert_eq!(report.emulated_capabilities().len(), 1);
    assert_eq!(report.unsupported_capabilities().len(), 0);
}

#[test]
fn dialect_report_not_all_satisfiable() {
    let report = DialectCapabilityReport {
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
fn dialect_report_to_receipt_metadata() {
    let report = DialectCapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    let meta = report.to_receipt_metadata();
    assert!(meta.is_object());
    assert_eq!(meta["source_dialect"], "claude");
    assert_eq!(meta["target_dialect"], "openai");
}

#[test]
fn dialect_report_empty_entries() {
    let report = DialectCapabilityReport {
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
// 7. generate_report
// ===========================================================================

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
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

#[test]
fn report_empty() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
}

#[test]
fn report_details_length_matches_total() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.details.len(), result.total());
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
    assert!(report.summary.contains("1 emulatable"));
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
fn report_counts_match_result() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead, Capability::ToolWrite],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.native_count, result.native.len());
    assert_eq!(report.emulated_count, result.emulated.len());
    assert_eq!(report.unsupported_count, result.unsupported.len());
}

// ===========================================================================
// 8. NegotiationResult helpers
// ===========================================================================

#[test]
fn negotiation_result_total() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.total(), 3);
}

#[test]
fn negotiation_result_total_zero() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![]);
    assert_eq!(result.total(), 0);
}

#[test]
fn negotiation_result_is_compatible_true() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    assert!(result.is_compatible());
}

#[test]
fn negotiation_result_is_compatible_false() {
    let result = NegotiationResult::from_simple(vec![], vec![], vec![Capability::Streaming]);
    assert!(!result.is_compatible());
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn edge_empty_manifest_empty_reqs() {
    let res = negotiate(
        &CapabilityManifest::new(),
        &CapabilityRequirements::default(),
    );
    assert!(res.is_compatible());
    assert_eq!(res.total(), 0);
}

#[test]
fn edge_all_capabilities_native() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Native))
        .collect();
    let m = manifest(&entries);
    let r = reqs_native(&all_capabilities());
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), all_capabilities().len());
}

#[test]
fn edge_all_capabilities_unsupported() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Unsupported))
        .collect();
    let m = manifest(&entries);
    let r = reqs_native(&all_capabilities());
    let res = negotiate(&m, &r);
    assert!(!res.is_compatible());
    assert_eq!(res.unsupported.len(), all_capabilities().len());
}

#[test]
fn edge_all_capabilities_emulated() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Emulated))
        .collect();
    let m = manifest(&entries);
    let r = reqs_native(&all_capabilities());
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), all_capabilities().len());
}

#[test]
fn edge_unknown_capability_absent_from_manifest() {
    // Test that a capability not in the manifest resolves to unsupported
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    assert_eq!(
        check_capability(&m, &Capability::McpServer),
        SupportLevel::Unsupported {
            reason: "unsupported".into()
        }
    );
}

#[test]
fn edge_capability_requirements_default() {
    let r = CapabilityRequirements::default();
    assert!(r.required.is_empty());
}

#[test]
fn edge_negotiate_many_duplicates() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let caps: Vec<_> = (0..50).map(|_| Capability::Streaming).collect();
    let r = reqs_native(&caps);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), 50);
}

#[test]
fn edge_diff_add_all_from_empty() {
    let old = CapabilityManifest::new();
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Native))
        .collect();
    let new = manifest(&entries);
    let d = CapabilityDiff::diff(&old, &new);
    assert_eq!(d.added.len(), all_capabilities().len());
    assert!(d.removed.is_empty());
}

#[test]
fn edge_diff_remove_all_to_empty() {
    let entries: Vec<_> = all_capabilities()
        .into_iter()
        .map(|c| (c, CoreSupportLevel::Native))
        .collect();
    let old = manifest(&entries);
    let new = CapabilityManifest::new();
    let d = CapabilityDiff::diff(&old, &new);
    assert!(d.added.is_empty());
    assert_eq!(d.removed.len(), all_capabilities().len());
}

#[test]
fn edge_restricted_reason_preserved_in_check() {
    let m = manifest(&[(
        Capability::ToolEdit,
        CoreSupportLevel::Restricted {
            reason: "read-only workspace".into(),
        },
    )]);
    if let SupportLevel::Emulated { method } = check_capability(&m, &Capability::ToolEdit) {
        assert!(method.contains("read-only workspace"));
    } else {
        panic!("Expected Emulated for Restricted");
    }
}

#[test]
fn edge_negotiation_result_clone() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let cloned = result.clone();
    assert_eq!(result, cloned);
}

#[test]
fn edge_compatibility_report_clone() {
    let result = NegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    let cloned = report.clone();
    assert_eq!(report, cloned);
}

#[test]
fn edge_emulated_min_support_accepts_native() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Native)]);
    let r = reqs_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    // check_capability ignores MinSupport; Native maps to Native in result
    assert!(res.is_compatible());
    assert_eq!(res.native.len(), 1);
}

#[test]
fn edge_emulated_min_support_accepts_emulated() {
    let m = manifest(&[(Capability::Streaming, CoreSupportLevel::Emulated)]);
    let r = reqs_emulated(&[Capability::Streaming]);
    let res = negotiate(&m, &r);
    assert!(res.is_compatible());
    assert_eq!(res.emulated.len(), 1);
}

#[test]
fn edge_report_detail_ordering() {
    let result = NegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    // Details are listed native first, then emulatable, then unsupported
    assert!(matches!(report.details[0].1, SupportLevel::Native));
    assert!(matches!(report.details[1].1, SupportLevel::Emulated { .. }));
    assert!(matches!(
        report.details[2].1,
        SupportLevel::Unsupported { .. }
    ));
}

#[test]
fn edge_negotiator_unsupported_threshold_everything_passes() {
    // With Unsupported as minimum, even absent capabilities should fail
    // because absent caps won't be in the manifest
    let m = core_manifest(&[(Capability::Streaming, CoreSupportLevel::Unsupported)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: CoreSupportLevel::Unsupported,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    // Unsupported has rank 0, and manifest has rank 0 for this cap, so 0 >= 0 = true
    assert!(res.is_compatible);
}

#[test]
fn edge_negotiator_mixed_required_and_preferred() {
    let m = core_manifest(&[
        (Capability::Streaming, CoreSupportLevel::Native),
        (Capability::Logprobs, CoreSupportLevel::Native),
        (Capability::ToolRead, CoreSupportLevel::Emulated),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![
            Capability::Logprobs,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
        minimum_support: CoreSupportLevel::Native,
    };
    let res = CapabilityNegotiator::negotiate(&req, &m);
    assert!(res.is_compatible);
    assert_eq!(res.satisfied, vec![Capability::Streaming]);
    // Only Logprobs meets Native threshold among preferred
    assert_eq!(res.bonus, vec![Capability::Logprobs]);
}

#[test]
fn edge_dialect_report_serde_roundtrip() {
    let report = DialectCapabilityReport {
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
                    reason: "not available".into(),
                },
            },
        ],
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: DialectCapabilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_dialect, "claude");
    assert_eq!(back.target_dialect, "openai");
    assert_eq!(back.entries.len(), 2);
}
