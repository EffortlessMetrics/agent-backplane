// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for capability negotiation across the Agent Backplane.

use std::collections::BTreeMap;

use abp_backend_core::ensure_capability_requirements;
use abp_core::negotiate::{
    check_capabilities, dialect_manifest, CapabilityDiff, CapabilityNegotiator,
    CapabilityReport as NegotiateCapabilityReport, CapabilityReportEntry, DialectSupportLevel,
    NegotiationRequest,
};
use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, SupportLevel, WorkOrderBuilder,
};
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use abp_protocol::{Envelope, JsonlCodec};

// =========================================================================
// 1. Capability Declaration and Parsing
// =========================================================================

#[test]
fn capability_enum_variants_serialize_snake_case() {
    let cap = Capability::ToolRead;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, r#""tool_read""#);
}

#[test]
fn capability_enum_all_variants_roundtrip() {
    let caps = vec![
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
    ];
    for cap in &caps {
        let json = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, &back);
    }
}

#[test]
fn capability_deserialize_from_snake_case_string() {
    let cap: Capability = serde_json::from_str(r#""tool_bash""#).unwrap();
    assert_eq!(cap, Capability::ToolBash);
}

#[test]
fn capability_invalid_string_fails_deserialize() {
    let result = serde_json::from_str::<Capability>(r#""nonexistent_cap""#);
    assert!(result.is_err());
}

#[test]
fn capability_ord_is_deterministic() {
    let mut caps = vec![
        Capability::ToolWrite,
        Capability::Streaming,
        Capability::ToolRead,
    ];
    caps.sort();
    let mut caps2 = caps.clone();
    caps2.sort();
    assert_eq!(caps, caps2);
}

#[test]
fn capability_manifest_is_btreemap() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    assert_eq!(manifest.len(), 2);
    assert!(manifest.contains_key(&Capability::ToolRead));
}

#[test]
fn capability_manifest_empty_by_default() {
    let manifest = CapabilityManifest::new();
    assert!(manifest.is_empty());
}

#[test]
fn capability_manifest_insert_replaces_existing() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    assert!(matches!(
        manifest.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
}

// =========================================================================
// 2. Capability Matching (native, emulated, unsupported levels)
// =========================================================================

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
        reason: "sandbox".into(),
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

#[test]
fn support_level_serialization_roundtrip_native() {
    let level = SupportLevel::Native;
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Native));
}

#[test]
fn support_level_serialization_roundtrip_emulated() {
    let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Emulated));
}

#[test]
fn support_level_serialization_roundtrip_unsupported() {
    let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Unsupported));
}

#[test]
fn support_level_serialization_roundtrip_restricted() {
    let level = SupportLevel::Restricted {
        reason: "policy limit".into(),
    };
    let json = serde_json::to_string(&level).unwrap();
    let back: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SupportLevel::Restricted { reason } if reason == "policy limit"));
}

#[test]
fn min_support_serialization_roundtrip() {
    for min in [MinSupport::Native, MinSupport::Emulated] {
        let json = serde_json::to_string(&min).unwrap();
        let _back: MinSupport = serde_json::from_str(&json).unwrap();
    }
}

// =========================================================================
// 3. Backend Capability Reporting (CapabilityMatrix)
// =========================================================================

#[test]
fn matrix_empty_by_default() {
    let m = CapabilityMatrix::new();
    assert!(m.is_empty());
    assert_eq!(m.backend_count(), 0);
}

#[test]
fn matrix_register_and_query() {
    let mut m = CapabilityMatrix::new();
    m.register("claude", vec![Capability::Streaming, Capability::ToolRead]);
    assert!(m.supports("claude", &Capability::Streaming));
    assert!(!m.supports("claude", &Capability::ToolBash));
}

#[test]
fn matrix_register_merges_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("openai", vec![Capability::Streaming]);
    m.register("openai", vec![Capability::ToolUse]);
    assert!(m.supports("openai", &Capability::Streaming));
    assert!(m.supports("openai", &Capability::ToolUse));
}

#[test]
fn matrix_backends_for_capability() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("c", vec![Capability::ToolRead]);

    let backends = m.backends_for(&Capability::Streaming);
    assert_eq!(backends.len(), 2);
    assert!(backends.contains(&"a".to_string()));
    assert!(backends.contains(&"b".to_string()));
}

#[test]
fn matrix_common_capabilities_intersection() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("b", vec![Capability::Streaming, Capability::ToolWrite]);

    let common = m.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
    assert!(!common.contains(&Capability::ToolWrite));
}

#[test]
fn matrix_common_capabilities_empty_when_no_backends() {
    let m = CapabilityMatrix::new();
    assert!(m.common_capabilities().is_empty());
}

#[test]
fn matrix_evaluate_perfect_score() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);

    let report = m.evaluate("a", &[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(report.score, 1.0);
    assert!(report.missing.is_empty());
}

#[test]
fn matrix_evaluate_partial_score() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);

    let report = m.evaluate("a", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn matrix_evaluate_zero_score_unknown_backend() {
    let m = CapabilityMatrix::new();
    let report = m.evaluate("unknown", &[Capability::Streaming]);
    assert_eq!(report.score, 0.0);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn matrix_evaluate_empty_requirements() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    let report = m.evaluate("a", &[]);
    assert_eq!(report.score, 1.0);
}

#[test]
fn matrix_best_backend_picks_highest_score() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register(
        "b",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    let best = m.best_backend(&[Capability::Streaming, Capability::ToolRead]);
    assert_eq!(best.as_deref(), Some("b"));
}

#[test]
fn matrix_all_capabilities_returns_none_for_unknown() {
    let m = CapabilityMatrix::new();
    assert!(m.all_capabilities("ghost").is_none());
}

// =========================================================================
// 4. Sidecar Hello Envelope Capability Exchange
// =========================================================================

fn sample_hello_envelope(caps: CapabilityManifest) -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        caps,
    )
}

#[test]
fn hello_envelope_roundtrip_empty_capabilities() {
    let hello = sample_hello_envelope(CapabilityManifest::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => assert!(capabilities.is_empty()),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_roundtrip_with_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    );

    let hello = sample_hello_envelope(caps);
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), 3);
            assert!(matches!(
                capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ));
            assert!(matches!(
                capabilities.get(&Capability::ToolBash),
                Some(SupportLevel::Restricted { .. })
            ));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_envelope_contains_contract_version() {
    let hello = sample_hello_envelope(CapabilityManifest::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(&format!(
        r#""contract_version":"{}""#,
        abp_core::CONTRACT_VERSION
    )));
}

#[test]
fn hello_envelope_contains_backend_identity() {
    let hello = sample_hello_envelope(CapabilityManifest::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""id":"test-sidecar""#));
}

#[test]
fn hello_envelope_discriminator_is_t() {
    let hello = sample_hello_envelope(CapabilityManifest::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn hello_envelope_all_capabilities_preserved() {
    let all_caps: Vec<Capability> = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
    ];
    let mut manifest = CapabilityManifest::new();
    for cap in &all_caps {
        manifest.insert(cap.clone(), SupportLevel::Native);
    }
    let hello = sample_hello_envelope(manifest);
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello { capabilities, .. } => {
            assert_eq!(capabilities.len(), all_caps.len());
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 5. Capability-Based Routing Decisions (BackendSelector)
// =========================================================================

fn make_candidate(name: &str, caps: Vec<Capability>, priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.into(),
        capabilities: caps,
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn selector_first_match_picks_first_capable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    sel.add_candidate(make_candidate(
        "b",
        vec![Capability::ToolRead, Capability::Streaming],
        1,
    ));
    let chosen = sel.select(&[Capability::ToolRead]).unwrap();
    assert_eq!(chosen.name, "a");
}

#[test]
fn selector_best_fit_picks_most_matches() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    sel.add_candidate(make_candidate(
        "b",
        vec![Capability::ToolRead, Capability::Streaming],
        1,
    ));
    let chosen = sel
        .select(&[Capability::ToolRead, Capability::Streaming])
        .unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_priority_picks_lowest_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 10));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_returns_none_when_no_candidate_satisfies() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn selector_skips_disabled_candidates() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = make_candidate("a", vec![Capability::Streaming], 1);
    c.enabled = false;
    sel.add_candidate(c);
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn selector_select_all_returns_all_capable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    sel.add_candidate(make_candidate("c", vec![Capability::ToolRead], 1));
    let all = sel.select_all(&[Capability::Streaming]);
    assert_eq!(all.len(), 2);
}

#[test]
fn selector_round_robin_rotates() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));

    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn selector_select_with_result_reports_unmet() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    let result = sel.select_with_result(&[Capability::ToolRead, Capability::McpClient]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn selector_select_with_result_success() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate(
        "a",
        vec![Capability::ToolRead, Capability::Streaming],
        1,
    ));
    let result = sel.select_with_result(&[Capability::ToolRead]);
    assert_eq!(result.selected, "a");
    assert!(result.unmet_capabilities.is_empty());
}

#[test]
fn selector_empty_requirements_matches_all() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    let chosen = sel.select(&[]).unwrap();
    assert_eq!(chosen.name, "a");
}

#[test]
fn selector_candidate_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert_eq!(sel.candidate_count(), 0);
    sel.add_candidate(make_candidate("a", vec![], 1));
    assert_eq!(sel.candidate_count(), 1);
}

#[test]
fn selector_enabled_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![], 1));
    let mut c = make_candidate("b", vec![], 1);
    c.enabled = false;
    sel.add_candidate(c);
    assert_eq!(sel.enabled_count(), 1);
}

// =========================================================================
// 6. Capability Intersection Between Requested and Available (Negotiator)
// =========================================================================

fn make_manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

#[test]
fn negotiator_all_required_satisfied() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let manifest = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert_eq!(result.satisfied.len(), 2);
    assert!(result.unsatisfied.is_empty());
}

#[test]
fn negotiator_partial_unsatisfied() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming, Capability::Logprobs],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let manifest = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
    assert_eq!(result.unsatisfied, vec![Capability::Logprobs]);
}

#[test]
fn negotiator_preferred_capabilities_in_bonus() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ExtendedThinking],
        minimum_support: SupportLevel::Emulated,
    };
    let manifest = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
    ]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
    assert_eq!(result.bonus, vec![Capability::ExtendedThinking]);
}

#[test]
fn negotiator_minimum_support_native_rejects_emulated() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let manifest = make_manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
}

#[test]
fn negotiator_empty_request_always_compatible() {
    let request = NegotiationRequest {
        required: vec![],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let manifest = CapabilityManifest::new();
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
}

#[test]
fn negotiator_best_match_picks_highest_score() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ToolRead, Capability::ToolWrite],
        minimum_support: SupportLevel::Emulated,
    };
    let m1 = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let m2 = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let best = CapabilityNegotiator::best_match(&request, &[("basic", m1), ("rich", m2)]);
    assert_eq!(best.unwrap().0, "rich");
}

#[test]
fn negotiator_best_match_none_when_all_incompatible() {
    let request = NegotiationRequest {
        required: vec![Capability::McpServer],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let m1 = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let best = CapabilityNegotiator::best_match(&request, &[("a", m1)]);
    assert!(best.is_none());
}

#[test]
fn negotiator_best_match_empty_manifests() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![],
        minimum_support: SupportLevel::Emulated,
    };
    let best = CapabilityNegotiator::best_match(&request, &[]);
    assert!(best.is_none());
}

// =========================================================================
// 7. Missing Capability Handling (ensure_capability_requirements)
// =========================================================================

#[test]
fn ensure_requirements_passes_when_all_met() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    assert!(ensure_capability_requirements(&reqs, &manifest).is_ok());
}

#[test]
fn ensure_requirements_fails_when_missing() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let manifest = CapabilityManifest::new();
    let err = ensure_capability_requirements(&reqs, &manifest);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unsatisfied"));
}

#[test]
fn ensure_requirements_fails_when_level_insufficient() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    assert!(ensure_capability_requirements(&reqs, &manifest).is_err());
}

#[test]
fn ensure_requirements_passes_with_empty_requirements() {
    let reqs = CapabilityRequirements::default();
    let manifest = CapabilityManifest::new();
    assert!(ensure_capability_requirements(&reqs, &manifest).is_ok());
}

#[test]
fn ensure_requirements_restricted_satisfies_emulated_min() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    assert!(ensure_capability_requirements(&reqs, &manifest).is_ok());
}

#[test]
fn ensure_requirements_multiple_unsatisfied_reported() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolBash,
                min_support: MinSupport::Native,
            },
        ],
    };
    let manifest = CapabilityManifest::new();
    let err = ensure_capability_requirements(&reqs, &manifest).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Streaming"));
    assert!(msg.contains("ToolBash"));
}

// =========================================================================
// 8. Capability Serialization/Deserialization Roundtrip
// =========================================================================

#[test]
fn capability_requirements_json_roundtrip() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolEdit,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let json = serde_json::to_string(&reqs).unwrap();
    let back: CapabilityRequirements = serde_json::from_str(&json).unwrap();
    assert_eq!(back.required.len(), 2);
}

#[test]
fn capability_manifest_json_roundtrip() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    manifest.insert(Capability::ToolBash, SupportLevel::Unsupported);
    manifest.insert(
        Capability::CodeExecution,
        SupportLevel::Restricted {
            reason: "sandbox only".into(),
        },
    );
    let json = serde_json::to_string(&manifest).unwrap();
    let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 4);
    assert!(matches!(
        back.get(&Capability::CodeExecution),
        Some(SupportLevel::Restricted { .. })
    ));
}

#[test]
fn work_order_with_requirements_roundtrip() {
    let wo = WorkOrderBuilder::new("test task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.requirements.required.len(), 1);
}

#[test]
fn backend_identity_json_roundtrip() {
    let id = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("2.0.0".into()),
        adapter_version: Some("0.1.0".into()),
    };
    let json = serde_json::to_string(&id).unwrap();
    let back: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "sidecar:node");
    assert_eq!(back.backend_version.as_deref(), Some("2.0.0"));
    assert_eq!(back.adapter_version.as_deref(), Some("0.1.0"));
}

#[test]
fn full_hello_envelope_json_roundtrip() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "feature-gated".into(),
        },
    );

    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        caps,
    );

    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();

    match decoded {
        Envelope::Hello {
            contract_version,
            backend,
            capabilities,
            ..
        } => {
            assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
            assert_eq!(backend.id, "test");
            assert_eq!(capabilities.len(), 4);
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// Dialect-Aware Negotiation
// =========================================================================

#[test]
fn dialect_manifest_claude_has_streaming() {
    let m = dialect_manifest("claude");
    assert!(m.contains_key(&Capability::Streaming));
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_openai_has_logprobs() {
    let m = dialect_manifest("openai");
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_gemini_has_pdf_native() {
    let m = dialect_manifest("gemini");
    assert!(matches!(
        m.get(&Capability::PdfInput),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn dialect_manifest_unknown_returns_empty() {
    let m = dialect_manifest("nonexistent");
    assert!(m.is_empty());
}

#[test]
fn check_capabilities_all_native_for_claude() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let report = check_capabilities(&wo, "claude", "claude");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 1);
}

#[test]
fn check_capabilities_unsupported_logprobs_for_claude() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Logprobs,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let report = check_capabilities(&wo, "openai", "claude");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
}

#[test]
fn check_capabilities_empty_requirements() {
    let wo = WorkOrderBuilder::new("task").build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
    assert!(report.entries.is_empty());
}

#[test]
fn capability_report_to_receipt_metadata_is_json() {
    let report = NegotiateCapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![CapabilityReportEntry {
            capability: Capability::Streaming,
            support: DialectSupportLevel::Native,
        }],
    };
    let meta = report.to_receipt_metadata();
    assert!(meta.is_object());
    assert!(meta["source_dialect"] == "claude");
}

// =========================================================================
// CapabilityDiff
// =========================================================================

#[test]
fn diff_detects_added_capabilities() {
    let old = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let new = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.added, vec![Capability::ToolRead]);
    assert!(diff.removed.is_empty());
}

#[test]
fn diff_detects_removed_capabilities() {
    let old = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let new = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed, vec![Capability::ToolRead]);
}

#[test]
fn diff_detects_upgrade() {
    let old = make_manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let new = make_manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert_eq!(diff.upgraded[0].0, Capability::ToolRead);
}

#[test]
fn diff_detects_downgrade() {
    let old = make_manifest(&[(Capability::ToolRead, SupportLevel::Native)]);
    let new = make_manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.downgraded.len(), 1);
}

#[test]
fn diff_identical_manifests_empty() {
    let m = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    let diff = CapabilityDiff::diff(&m, &m);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.upgraded.is_empty());
    assert!(diff.downgraded.is_empty());
}

#[test]
fn diff_both_empty() {
    let diff = CapabilityDiff::diff(&CapabilityManifest::new(), &CapabilityManifest::new());
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
}

// =========================================================================
// Edge Cases and Integration
// =========================================================================

#[test]
fn selector_least_loaded_picks_lowest_priority_as_proxy() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(make_candidate("heavy", vec![Capability::Streaming], 100));
    sel.add_candidate(make_candidate("light", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "light");
}

#[test]
fn negotiator_restricted_counted_when_minimum_is_restricted() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolBash],
        preferred: vec![],
        minimum_support: SupportLevel::Restricted {
            reason: String::new(),
        },
    };
    let manifest = make_manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
}

#[test]
fn manifest_deterministic_serialization_btreemap() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolWrite, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Native);
    m1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Native);
    m2.insert(Capability::ToolRead, SupportLevel::Native);
    m2.insert(Capability::ToolWrite, SupportLevel::Native);

    let j1 = serde_json::to_string(&m1).unwrap();
    let j2 = serde_json::to_string(&m2).unwrap();
    assert_eq!(j1, j2, "BTreeMap ensures deterministic key order");
}

#[test]
fn selector_multiple_requirements_all_must_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    assert!(sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .is_some());
    let chosen = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn hello_envelope_with_mode_passthrough() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "pt".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        abp_core::ExecutionMode::Passthrough,
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains("passthrough"));
}

#[test]
fn capability_requirement_json_roundtrip() {
    let req = CapabilityRequirement {
        capability: Capability::McpServer,
        min_support: MinSupport::Emulated,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CapabilityRequirement = serde_json::from_str(&json).unwrap();
    assert_eq!(back.capability, Capability::McpServer);
}

#[test]
fn dialect_report_emulated_capabilities() {
    let report = NegotiateCapabilityReport {
        source_dialect: "claude".into(),
        target_dialect: "openai".into(),
        entries: vec![
            CapabilityReportEntry {
                capability: Capability::Streaming,
                support: DialectSupportLevel::Native,
            },
            CapabilityReportEntry {
                capability: Capability::PdfInput,
                support: DialectSupportLevel::Emulated {
                    detail: "via conversion".into(),
                },
            },
        ],
    };
    assert_eq!(report.native_capabilities().len(), 1);
    assert_eq!(report.emulated_capabilities().len(), 1);
    assert!(report.all_satisfiable());
}

#[test]
fn dialect_support_level_json_roundtrip() {
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
        assert_eq!(level, &back);
    }
}

#[test]
fn selection_result_reports_alternatives() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.alternatives.contains(&"b".to_string()));
}

#[test]
fn negotiator_unsupported_minimum_rejects_restricted() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Unsupported,
    };
    let manifest = make_manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
}
