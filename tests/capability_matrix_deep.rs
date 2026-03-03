// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive capability-system tests — 90+ tests covering every variant,
//! serde round-trips, BTreeSet operations, support-level semantics,
//! negotiation, dialect manifests, ordering, edge cases, and property checks.

use std::collections::BTreeSet;

use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, DialectSupportLevel, NegotiationRequest,
    check_capabilities, dialect_manifest,
};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel, WorkOrderBuilder,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// All Capability enum variants in definition order.
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
        Capability::FunctionCalling,
        Capability::Vision,
        Capability::Audio,
        Capability::JsonMode,
        Capability::SystemMessage,
        Capability::Temperature,
        Capability::TopP,
        Capability::TopK,
        Capability::MaxTokens,
        Capability::FrequencyPenalty,
        Capability::PresencePenalty,
        Capability::CacheControl,
        Capability::BatchMode,
        Capability::Embeddings,
        Capability::ImageGeneration,
    ]
}

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

fn cap_set(caps: &[Capability]) -> BTreeSet<Capability> {
    caps.iter().cloned().collect()
}

// ===========================================================================
// 1. Capability enum variant exhaustiveness
// ===========================================================================

#[test]
fn all_capability_variants_accounted() {
    let caps = all_capabilities();
    assert_eq!(
        caps.len(),
        41,
        "update all_capabilities() if variants added"
    );
}

#[test]
fn capability_variant_streaming() {
    let c = Capability::Streaming;
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(json, r#""streaming""#);
}

#[test]
fn capability_variant_tool_read() {
    let json = serde_json::to_string(&Capability::ToolRead).unwrap();
    assert_eq!(json, r#""tool_read""#);
}

#[test]
fn capability_variant_tool_write() {
    let json = serde_json::to_string(&Capability::ToolWrite).unwrap();
    assert_eq!(json, r#""tool_write""#);
}

#[test]
fn capability_variant_tool_edit() {
    let json = serde_json::to_string(&Capability::ToolEdit).unwrap();
    assert_eq!(json, r#""tool_edit""#);
}

#[test]
fn capability_variant_tool_bash() {
    let json = serde_json::to_string(&Capability::ToolBash).unwrap();
    assert_eq!(json, r#""tool_bash""#);
}

#[test]
fn capability_variant_tool_glob() {
    let json = serde_json::to_string(&Capability::ToolGlob).unwrap();
    assert_eq!(json, r#""tool_glob""#);
}

#[test]
fn capability_variant_tool_grep() {
    let json = serde_json::to_string(&Capability::ToolGrep).unwrap();
    assert_eq!(json, r#""tool_grep""#);
}

#[test]
fn capability_variant_tool_web_search() {
    let json = serde_json::to_string(&Capability::ToolWebSearch).unwrap();
    assert_eq!(json, r#""tool_web_search""#);
}

#[test]
fn capability_variant_tool_web_fetch() {
    let json = serde_json::to_string(&Capability::ToolWebFetch).unwrap();
    assert_eq!(json, r#""tool_web_fetch""#);
}

#[test]
fn capability_variant_tool_ask_user() {
    let json = serde_json::to_string(&Capability::ToolAskUser).unwrap();
    assert_eq!(json, r#""tool_ask_user""#);
}

#[test]
fn capability_variant_hooks_pre_tool_use() {
    let json = serde_json::to_string(&Capability::HooksPreToolUse).unwrap();
    assert_eq!(json, r#""hooks_pre_tool_use""#);
}

#[test]
fn capability_variant_hooks_post_tool_use() {
    let json = serde_json::to_string(&Capability::HooksPostToolUse).unwrap();
    assert_eq!(json, r#""hooks_post_tool_use""#);
}

#[test]
fn capability_variant_session_resume() {
    let json = serde_json::to_string(&Capability::SessionResume).unwrap();
    assert_eq!(json, r#""session_resume""#);
}

#[test]
fn capability_variant_session_fork() {
    let json = serde_json::to_string(&Capability::SessionFork).unwrap();
    assert_eq!(json, r#""session_fork""#);
}

#[test]
fn capability_variant_checkpointing() {
    let json = serde_json::to_string(&Capability::Checkpointing).unwrap();
    assert_eq!(json, r#""checkpointing""#);
}

#[test]
fn capability_variant_structured_output_json_schema() {
    let json = serde_json::to_string(&Capability::StructuredOutputJsonSchema).unwrap();
    assert_eq!(json, r#""structured_output_json_schema""#);
}

#[test]
fn capability_variant_mcp_client() {
    let json = serde_json::to_string(&Capability::McpClient).unwrap();
    assert_eq!(json, r#""mcp_client""#);
}

#[test]
fn capability_variant_mcp_server() {
    let json = serde_json::to_string(&Capability::McpServer).unwrap();
    assert_eq!(json, r#""mcp_server""#);
}

#[test]
fn capability_variant_tool_use() {
    let json = serde_json::to_string(&Capability::ToolUse).unwrap();
    assert_eq!(json, r#""tool_use""#);
}

#[test]
fn capability_variant_extended_thinking() {
    let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
    assert_eq!(json, r#""extended_thinking""#);
}

#[test]
fn capability_variant_image_input() {
    let json = serde_json::to_string(&Capability::ImageInput).unwrap();
    assert_eq!(json, r#""image_input""#);
}

#[test]
fn capability_variant_pdf_input() {
    let json = serde_json::to_string(&Capability::PdfInput).unwrap();
    assert_eq!(json, r#""pdf_input""#);
}

#[test]
fn capability_variant_code_execution() {
    let json = serde_json::to_string(&Capability::CodeExecution).unwrap();
    assert_eq!(json, r#""code_execution""#);
}

#[test]
fn capability_variant_logprobs() {
    let json = serde_json::to_string(&Capability::Logprobs).unwrap();
    assert_eq!(json, r#""logprobs""#);
}

#[test]
fn capability_variant_seed_determinism() {
    let json = serde_json::to_string(&Capability::SeedDeterminism).unwrap();
    assert_eq!(json, r#""seed_determinism""#);
}

#[test]
fn capability_variant_stop_sequences() {
    let json = serde_json::to_string(&Capability::StopSequences).unwrap();
    assert_eq!(json, r#""stop_sequences""#);
}

// ===========================================================================
// 2. Serde round-trips (individual + collections)
// ===========================================================================

#[test]
fn serde_roundtrip_every_capability() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cap);
    }
}

#[test]
fn serde_roundtrip_capability_vec() {
    let caps = all_capabilities();
    let json = serde_json::to_string(&caps).unwrap();
    let back: Vec<Capability> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, caps);
}

#[test]
fn serde_roundtrip_capability_btreeset() {
    let set: BTreeSet<Capability> = all_capabilities().into_iter().collect();
    let json = serde_json::to_string(&set).unwrap();
    let back: BTreeSet<Capability> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, set);
}

#[test]
fn serde_roundtrip_capability_manifest() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolBash, SupportLevel::Unsupported),
        (
            Capability::ToolEdit,
            SupportLevel::Restricted {
                reason: "sandbox".into(),
            },
        ),
    ]);
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), m.len());
}

#[test]
fn serde_roundtrip_empty_manifest() {
    let m = CapabilityManifest::new();
    let json = serde_json::to_string(&m).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
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

// ===========================================================================
// 3. BTreeSet operations: union, intersection, difference
// ===========================================================================

#[test]
fn btreeset_union_two_disjoint_sets() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let b = cap_set(&[Capability::ToolWrite, Capability::ToolEdit]);
    let u: BTreeSet<_> = a.union(&b).cloned().collect();
    assert_eq!(u.len(), 4);
}

#[test]
fn btreeset_union_overlapping_sets() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let b = cap_set(&[Capability::ToolRead, Capability::ToolWrite]);
    let u: BTreeSet<_> = a.union(&b).cloned().collect();
    assert_eq!(u.len(), 3);
    assert!(u.contains(&Capability::ToolRead));
}

#[test]
fn btreeset_intersection_overlapping() {
    let a = cap_set(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolEdit,
    ]);
    let b = cap_set(&[
        Capability::ToolRead,
        Capability::ToolEdit,
        Capability::ToolBash,
    ]);
    let i: BTreeSet<_> = a.intersection(&b).cloned().collect();
    assert_eq!(i.len(), 2);
    assert!(i.contains(&Capability::ToolRead));
    assert!(i.contains(&Capability::ToolEdit));
}

#[test]
fn btreeset_intersection_disjoint() {
    let a = cap_set(&[Capability::Streaming]);
    let b = cap_set(&[Capability::ToolRead]);
    let i: BTreeSet<_> = a.intersection(&b).cloned().collect();
    assert!(i.is_empty());
}

#[test]
fn btreeset_difference() {
    let a = cap_set(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]);
    let b = cap_set(&[Capability::ToolRead]);
    let d: BTreeSet<_> = a.difference(&b).cloned().collect();
    assert_eq!(d.len(), 2);
    assert!(!d.contains(&Capability::ToolRead));
}

#[test]
fn btreeset_symmetric_difference() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let b = cap_set(&[Capability::ToolRead, Capability::ToolWrite]);
    let sd: BTreeSet<_> = a.symmetric_difference(&b).cloned().collect();
    assert_eq!(sd.len(), 2);
    assert!(sd.contains(&Capability::Streaming));
    assert!(sd.contains(&Capability::ToolWrite));
}

#[test]
fn btreeset_is_subset() {
    let small = cap_set(&[Capability::Streaming]);
    let big = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    assert!(small.is_subset(&big));
    assert!(!big.is_subset(&small));
}

#[test]
fn btreeset_is_superset() {
    let big = cap_set(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]);
    let small = cap_set(&[Capability::ToolRead]);
    assert!(big.is_superset(&small));
}

// ===========================================================================
// 4. SupportLevel semantics
// ===========================================================================

#[test]
fn support_native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn support_native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

#[test]
fn support_emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_restricted_does_not_satisfy_native() {
    let r = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn support_restricted_satisfies_emulated() {
    let r = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(r.satisfies(&MinSupport::Emulated));
}

#[test]
fn support_unsupported_satisfies_neither() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
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
        reason: "test".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Restricted { .. }));
}

// ===========================================================================
// 5. CapabilityNegotiator (from abp-core::negotiate)
// ===========================================================================

#[test]
fn negotiator_all_satisfied() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
    assert!(result.unsatisfied.is_empty());
}

#[test]
fn negotiator_partial_unsatisfied() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!result.is_compatible);
    assert_eq!(result.satisfied, vec![Capability::Streaming]);
    assert_eq!(result.unsatisfied, vec![Capability::ToolRead]);
}

#[test]
fn negotiator_preferred_bonus() {
    let m = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead, Capability::ToolWrite],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
    assert_eq!(result.bonus, vec![Capability::ToolRead]);
}

#[test]
fn negotiator_emulated_not_meeting_native_minimum() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(!result.is_compatible);
}

#[test]
fn negotiator_emulated_meeting_emulated_minimum() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
}

#[test]
fn negotiator_empty_requirements_always_compatible() {
    let m = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let req = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let result = CapabilityNegotiator::negotiate(&req, &m);
    assert!(result.is_compatible);
}

#[test]
fn negotiator_best_match_selects_highest_score() {
    let m1 = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let m2 = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead],
        minimum_support: SupportLevel::Native,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("a", m1), ("b", m2)]);
    assert!(best.is_some());
    assert_eq!(best.unwrap().0, "b");
}

#[test]
fn negotiator_best_match_none_when_incompatible() {
    let m = manifest(&[]);
    let req = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let best = CapabilityNegotiator::best_match(&req, &[("a", m)]);
    assert!(best.is_none());
}

// ===========================================================================
// 6. CapabilityDiff
// ===========================================================================

#[test]
fn diff_added_capabilities() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added, vec![Capability::ToolRead]);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_removed_capabilities() {
    let old = manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed, vec![Capability::ToolRead]);
}

#[test]
fn diff_upgraded_capability() {
    let old = manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let new = manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert_eq!(diff.upgraded[0].0, Capability::Streaming);
}

#[test]
fn diff_downgraded_capability() {
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

#[test]
fn diff_empty_manifests() {
    let diff = CapabilityDiff::diff(&CapabilityManifest::new(), &CapabilityManifest::new());
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
}

// ===========================================================================
// 7. Capability as JSON schema enum values
// ===========================================================================

#[test]
fn capability_json_schema_contains_all_variants() {
    let schema = schemars::schema_for!(Capability);
    let json = serde_json::to_value(&schema).unwrap();
    // The enum schema should list all variant strings
    let enum_values = json
        .pointer("/enum")
        .or_else(|| json.pointer("/oneOf"))
        .expect("schema must have enum or oneOf");
    let arr = enum_values.as_array().unwrap();
    assert_eq!(arr.len(), all_capabilities().len());
}

#[test]
fn capability_json_values_are_snake_case() {
    for cap in all_capabilities() {
        let json = serde_json::to_string(&cap).unwrap();
        let s = json.trim_matches('"');
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "variant {s} is not snake_case"
        );
    }
}

// ===========================================================================
// 8. Ordering and determinism
// ===========================================================================

#[test]
fn capability_ord_is_consistent_with_btreeset() {
    let mut v = all_capabilities();
    v.reverse();
    let set: BTreeSet<Capability> = v.into_iter().collect();
    let ordered: Vec<_> = set.into_iter().collect();
    // BTreeSet should produce the same order as the derive(Ord) order
    let mut check = all_capabilities();
    check.sort();
    assert_eq!(ordered, check);
}

#[test]
fn manifest_keys_deterministic() {
    let m = manifest(&[
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let keys1: Vec<_> = m.keys().cloned().collect();
    let keys2: Vec<_> = m.keys().cloned().collect();
    assert_eq!(keys1, keys2);
}

#[test]
fn manifest_json_deterministic() {
    let m = manifest(&[
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::Streaming, SupportLevel::Emulated),
    ]);
    let j1 = serde_json::to_string(&m).unwrap();
    let j2 = serde_json::to_string(&m).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn btreeset_iteration_is_sorted() {
    let set = cap_set(&[
        Capability::ToolWrite,
        Capability::Streaming,
        Capability::ToolRead,
    ]);
    let v: Vec<_> = set.into_iter().collect();
    let mut sorted = v.clone();
    sorted.sort();
    assert_eq!(v, sorted);
}

// ===========================================================================
// 9. Edge cases: empty / all / single
// ===========================================================================

#[test]
fn empty_capability_set() {
    let set: BTreeSet<Capability> = BTreeSet::new();
    assert!(set.is_empty());
    let json = serde_json::to_string(&set).unwrap();
    assert_eq!(json, "[]");
}

#[test]
fn all_capabilities_set() {
    let set: BTreeSet<Capability> = all_capabilities().into_iter().collect();
    assert_eq!(set.len(), 41);
}

#[test]
fn single_capability_set_operations() {
    let a = cap_set(&[Capability::Streaming]);
    let b = cap_set(&[Capability::Streaming]);
    let u: BTreeSet<_> = a.union(&b).cloned().collect();
    let i: BTreeSet<_> = a.intersection(&b).cloned().collect();
    let d: BTreeSet<_> = a.difference(&b).cloned().collect();
    assert_eq!(u.len(), 1);
    assert_eq!(i.len(), 1);
    assert!(d.is_empty());
}

#[test]
fn empty_set_union_identity() {
    let empty: BTreeSet<Capability> = BTreeSet::new();
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let u: BTreeSet<_> = a.union(&empty).cloned().collect();
    assert_eq!(u, a);
}

#[test]
fn empty_set_intersection_absorbing() {
    let empty: BTreeSet<Capability> = BTreeSet::new();
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let i: BTreeSet<_> = a.intersection(&empty).cloned().collect();
    assert!(i.is_empty());
}

// ===========================================================================
// 10. Cross-dialect capability mapping
// ===========================================================================

#[test]
fn claude_dialect_has_streaming_native() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn claude_dialect_has_tool_use_native() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::ToolUse),
        Some(DialectSupportLevel::Native)
    ));
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
fn claude_dialect_seed_determinism_unsupported() {
    let m = dialect_manifest("claude");
    assert!(matches!(
        m.get(&Capability::SeedDeterminism),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn openai_dialect_has_streaming_native() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn openai_dialect_has_code_execution_native() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::CodeExecution),
        Some(DialectSupportLevel::Native)
    ));
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
fn openai_dialect_pdf_unsupported() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::PdfInput),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn gemini_dialect_has_streaming_native() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn gemini_dialect_has_pdf_native() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::PdfInput),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn gemini_dialect_logprobs_unsupported() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn gemini_dialect_extended_thinking_emulated() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn unknown_dialect_returns_empty() {
    let m = dialect_manifest("foobar");
    assert!(m.is_empty());
}

#[test]
fn claude_and_openai_share_streaming() {
    let claude = dialect_manifest("claude");
    let openai = dialect_manifest("openai");
    assert!(matches!(
        claude.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        openai.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn openai_has_logprobs_native_claude_does_not() {
    let claude = dialect_manifest("claude");
    let openai = dialect_manifest("openai");
    assert!(matches!(
        openai.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        claude.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

// ===========================================================================
// 11. CapabilityReport (check_capabilities)
// ===========================================================================

#[test]
fn check_capabilities_report_structure() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert_eq!(report.source_dialect, "claude");
    assert_eq!(report.target_dialect, "openai");
    assert_eq!(report.entries.len(), 1);
}

#[test]
fn check_capabilities_native_in_target() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(matches!(
        report.entries[0].support,
        DialectSupportLevel::Native
    ));
}

#[test]
fn check_capabilities_unsupported_in_target() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "openai", "claude");
    assert!(matches!(
        report.entries[0].support,
        DialectSupportLevel::Unsupported { .. }
    ));
}

#[test]
fn check_capabilities_empty_requirements() {
    let wo = WorkOrderBuilder::new("test").build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.entries.is_empty());
    assert!(report.all_satisfiable());
}

#[test]
fn capability_report_all_satisfiable() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming, Capability::ToolUse]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
}

#[test]
fn capability_report_not_all_satisfiable() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "openai", "claude");
    assert!(!report.all_satisfiable());
}

#[test]
fn capability_report_native_filter() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming, Capability::Logprobs]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    let native = report.native_capabilities();
    assert!(native.iter().any(|e| e.capability == Capability::Streaming));
}

#[test]
fn capability_report_unsupported_filter() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::ExtendedThinking]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    let unsupported = report.unsupported_capabilities();
    assert_eq!(unsupported.len(), 1);
}

#[test]
fn capability_report_to_receipt_metadata() {
    let wo = WorkOrderBuilder::new("test")
        .requirements(reqs_native(&[Capability::Streaming]))
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    let meta = report.to_receipt_metadata();
    assert!(meta.is_object());
}

// ===========================================================================
// 12. Property tests: commutativity / associativity of set operations
// ===========================================================================

#[test]
fn union_is_commutative() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let b = cap_set(&[Capability::ToolWrite, Capability::ToolRead]);
    let ab: BTreeSet<_> = a.union(&b).cloned().collect();
    let ba: BTreeSet<_> = b.union(&a).cloned().collect();
    assert_eq!(ab, ba);
}

#[test]
fn intersection_is_commutative() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let b = cap_set(&[Capability::ToolWrite, Capability::ToolRead]);
    let ab: BTreeSet<_> = a.intersection(&b).cloned().collect();
    let ba: BTreeSet<_> = b.intersection(&a).cloned().collect();
    assert_eq!(ab, ba);
}

#[test]
fn union_is_associative() {
    let a = cap_set(&[Capability::Streaming]);
    let b = cap_set(&[Capability::ToolRead]);
    let c = cap_set(&[Capability::ToolWrite]);
    let ab: BTreeSet<_> = a.union(&b).cloned().collect();
    let ab_c: BTreeSet<_> = ab.union(&c).cloned().collect();
    let bc: BTreeSet<_> = b.union(&c).cloned().collect();
    let a_bc: BTreeSet<_> = a.union(&bc).cloned().collect();
    assert_eq!(ab_c, a_bc);
}

#[test]
fn intersection_is_associative() {
    let all = all_capabilities();
    let a = cap_set(&all[0..10]);
    let b = cap_set(&all[5..15]);
    let c = cap_set(&all[7..20]);
    let ab: BTreeSet<_> = a.intersection(&b).cloned().collect();
    let ab_c: BTreeSet<_> = ab.intersection(&c).cloned().collect();
    let bc: BTreeSet<_> = b.intersection(&c).cloned().collect();
    let a_bc: BTreeSet<_> = a.intersection(&bc).cloned().collect();
    assert_eq!(ab_c, a_bc);
}

#[test]
fn union_idempotent() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let aa: BTreeSet<_> = a.union(&a).cloned().collect();
    assert_eq!(aa, a);
}

#[test]
fn intersection_idempotent() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let aa: BTreeSet<_> = a.intersection(&a).cloned().collect();
    assert_eq!(aa, a);
}

#[test]
fn difference_with_self_is_empty() {
    let a = cap_set(&[Capability::Streaming, Capability::ToolRead]);
    let d: BTreeSet<_> = a.difference(&a).cloned().collect();
    assert!(d.is_empty());
}

#[test]
fn union_with_all_is_all() {
    let all_set: BTreeSet<_> = all_capabilities().into_iter().collect();
    let partial = cap_set(&[Capability::Streaming]);
    let u: BTreeSet<_> = partial.union(&all_set).cloned().collect();
    assert_eq!(u, all_set);
}

#[test]
fn intersection_with_empty_is_empty() {
    let empty: BTreeSet<Capability> = BTreeSet::new();
    let a = cap_set(&all_capabilities());
    let i: BTreeSet<_> = a.intersection(&empty).cloned().collect();
    assert!(i.is_empty());
}

// ===========================================================================
// 13. DialectSupportLevel serde
// ===========================================================================

#[test]
fn dialect_support_level_serde_native() {
    let l = DialectSupportLevel::Native;
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, DialectSupportLevel::Native);
}

#[test]
fn dialect_support_level_serde_emulated() {
    let l = DialectSupportLevel::Emulated {
        detail: "polyfill".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, l);
}

#[test]
fn dialect_support_level_serde_unsupported() {
    let l = DialectSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&l).unwrap();
    let back: DialectSupportLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, l);
}

// ===========================================================================
// 14. Capability Hash / Eq
// ===========================================================================

#[test]
fn capability_eq_reflexive() {
    for cap in all_capabilities() {
        assert_eq!(cap, cap);
    }
}

#[test]
fn capability_distinct_variants_not_equal() {
    let caps = all_capabilities();
    for (i, a) in caps.iter().enumerate() {
        for (j, b) in caps.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "{a:?} should not equal {b:?}");
            }
        }
    }
}

#[test]
fn capability_hash_consistent_with_eq() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    for cap in all_capabilities() {
        assert!(set.insert(cap), "duplicate hash/eq for capability");
    }
    assert_eq!(set.len(), all_capabilities().len());
}

// ===========================================================================
// 15. Manifest edge-case: duplicate insert replaces
// ===========================================================================

#[test]
fn manifest_insert_overwrites() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    m.insert(Capability::Streaming, SupportLevel::Native);
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
    assert_eq!(m.len(), 1);
}

#[test]
fn manifest_all_capabilities_inserted() {
    let mut m = CapabilityManifest::new();
    for cap in all_capabilities() {
        m.insert(cap, SupportLevel::Native);
    }
    assert_eq!(m.len(), 41);
}

// ===========================================================================
// 16. MinSupport serde
// ===========================================================================

#[test]
fn min_support_serde_native() {
    let json = serde_json::to_string(&MinSupport::Native).unwrap();
    let back: MinSupport = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, MinSupport::Native));
}

#[test]
fn min_support_serde_emulated() {
    let json = serde_json::to_string(&MinSupport::Emulated).unwrap();
    let back: MinSupport = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, MinSupport::Emulated));
}
