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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for capability negotiation across the Agent Backplane.

use std::collections::BTreeMap;

use abp_backend_core::ensure_capability_requirements;
use abp_capability::negotiate::{apply_policy, pre_negotiate, NegotiationError, NegotiationPolicy};
use abp_capability::{
    check_capability, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    default_emulation_strategy as cap_default_emulation, gemini_15_pro_manifest, generate_report,
    kimi_manifest, negotiate as cap_negotiate, negotiate_capabilities as cap_negotiate_caps,
    openai_gpt4o_manifest, CapabilityRegistry, CompatibilityReport,
    EmulationStrategy as CapEmulationStrategy, NegotiationResult as CapNegotiationResult,
    SupportLevel as CapSupportLevel,
};
use abp_core::negotiate::{
    check_capabilities, dialect_manifest, CapabilityDiff, CapabilityNegotiator,
    CapabilityReport as NegotiateCapabilityReport, CapabilityReportEntry, DialectSupportLevel,
    NegotiationRequest,
};
use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, SupportLevel, WorkOrderBuilder,
};
use abp_emulation::{
    can_emulate, compute_fidelity, default_strategy, EmulationConfig, EmulationEngine,
    EmulationStrategy, FidelityLabel,
};
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use abp_protocol::{Envelope, JsonlCodec};

// =========================================================================
// 1. Capability Declaration Parsing and Validation
// =========================================================================

#[test]
fn capability_enum_variants_serialize_snake_case() {
    let cap = Capability::ToolRead;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, r#""tool_read""#);
}

#[test]
fn capability_enum_all_variants_roundtrip() {
    let caps = all_capability_variants();
    assert!(caps.len() >= 41, "Expected at least 41 capability variants");
    for cap in &caps {
        let json = serde_json::to_string(cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, &back);
    }
}

fn all_capability_variants() -> Vec<Capability> {
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

#[test]
fn capability_requirements_default_is_empty() {
    let reqs = CapabilityRequirements::default();
    assert!(reqs.required.is_empty());
}

#[test]
fn capability_requirement_pairs_cap_with_min_support() {
    let req = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    };
    assert_eq!(req.capability, Capability::Streaming);
}

// =========================================================================
// 2. Capability Intersection (what both sides support)
// =========================================================================

fn make_manifest(entries: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    entries.iter().cloned().collect()
}

#[test]
fn cap_negotiate_all_native() {
    let manifest = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
    ]);
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            },
        ],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert_eq!(result.native.len(), 2);
    assert!(result.emulated.is_empty());
    assert!(result.unsupported.is_empty());
    assert!(result.is_compatible());
}

#[test]
fn cap_negotiate_mixed_native_emulated_unsupported() {
    let manifest = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
    ]);
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::Logprobs,
                min_support: MinSupport::Native,
            },
        ],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(result.unsupported_caps(), vec![Capability::Logprobs]);
    assert!(!result.is_compatible());
}

#[test]
fn cap_negotiate_empty_requirements_always_compatible() {
    let result = cap_negotiate(
        &CapabilityManifest::new(),
        &CapabilityRequirements::default(),
    );
    assert!(result.is_compatible());
    assert_eq!(result.total(), 0);
}

#[test]
fn cap_negotiate_empty_manifest_all_unsupported() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let result = cap_negotiate(&CapabilityManifest::new(), &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported_caps(), vec![Capability::Streaming]);
}

#[test]
fn cap_negotiate_restricted_counts_as_emulatable() {
    let manifest = make_manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    )]);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert_eq!(result.emulated_caps(), vec![Capability::ToolBash]);
    assert!(result.is_compatible());
}

#[test]
fn cap_negotiate_total_counts_all_buckets() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.total(), 3);
}

#[test]
fn matrix_common_capabilities_is_intersection() {
    let mut m = CapabilityMatrix::new();
    m.register(
        "a",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    m.register(
        "b",
        vec![
            Capability::Streaming,
            Capability::ToolWrite,
            Capability::Logprobs,
        ],
    );
    m.register("c", vec![Capability::Streaming, Capability::ToolWrite]);

    let common = m.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(common.contains(&Capability::ToolWrite));
    assert!(!common.contains(&Capability::ToolRead));
    assert!(!common.contains(&Capability::Logprobs));
}

#[test]
fn negotiator_intersection_required_vs_manifest() {
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
fn negotiator_preferred_in_bonus_not_required() {
    let request = NegotiationRequest {
        required: vec![Capability::Streaming],
        preferred: vec![Capability::ExtendedThinking, Capability::Logprobs],
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
fn negotiator_best_match_among_multiple_manifests() {
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
    assert!(CapabilityNegotiator::best_match(&request, &[("a", m1)]).is_none());
}

// =========================================================================
// 3. Native vs Emulated vs Unsupported Feature Classification
// =========================================================================

#[test]
fn check_capability_native_returns_native() {
    let manifest = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert_eq!(
        check_capability(&manifest, &Capability::Streaming),
        CapSupportLevel::Native
    );
}

#[test]
fn check_capability_emulated_returns_emulated() {
    let manifest = make_manifest(&[(Capability::Streaming, SupportLevel::Emulated)]);
    let level = check_capability(&manifest, &Capability::Streaming);
    assert!(matches!(level, CapSupportLevel::Emulated { method } if method == "adapter"));
}

#[test]
fn check_capability_restricted_returns_emulated_with_reason() {
    let manifest = make_manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    )]);
    let level = check_capability(&manifest, &Capability::ToolBash);
    match level {
        CapSupportLevel::Restricted { reason } => {
            assert!(reason.contains("policy"));
        }
        other => panic!("expected Restricted, got {other:?}"),
    }
}

#[test]
fn check_capability_missing_returns_unsupported() {
    let manifest = CapabilityManifest::new();
    assert_eq!(
        check_capability(&manifest, &Capability::Streaming),
        CapSupportLevel::Unsupported {
            reason: "not declared in manifest".into()
        }
    );
}

#[test]
fn check_capability_explicit_unsupported_returns_unsupported() {
    let manifest = make_manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    assert_eq!(
        check_capability(&manifest, &Capability::Logprobs),
        CapSupportLevel::Unsupported {
            reason: "explicitly marked unsupported".into()
        }
    );
}

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
fn support_level_restricted_satisfies_emulated_but_not_native() {
    let restricted = SupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert!(restricted.satisfies(&MinSupport::Emulated));
    assert!(!restricted.satisfies(&MinSupport::Native));
}

#[test]
fn dialect_support_level_native_emulated_unsupported_classification() {
    let claude = dialect_manifest("claude");
    assert!(matches!(
        claude.get(&Capability::Streaming),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        claude.get(&Capability::StructuredOutputJsonSchema),
        Some(DialectSupportLevel::Emulated { .. })
    ));
    assert!(matches!(
        claude.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn generate_report_classifies_all_three_levels() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn generate_report_fully_compatible_when_no_unsupported() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    let report = generate_report(&result);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_incompatible_when_unsupported_present() {
    let result = CapNegotiationResult::from_simple(vec![], vec![], vec![Capability::Logprobs]);
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert!(report.summary.contains("incompatible"));
}

// =========================================================================
// 4. Early Failure When Fidelity Cannot Be Preserved
// =========================================================================

#[test]
fn ensure_requirements_passes_when_all_met() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let manifest = make_manifest(&[(Capability::Streaming, SupportLevel::Native)]);
    assert!(ensure_capability_requirements(&reqs, &manifest).is_ok());
}

#[test]
fn ensure_requirements_fails_when_capability_missing() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let err = ensure_capability_requirements(&reqs, &CapabilityManifest::new());
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
    let manifest = make_manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    assert!(ensure_capability_requirements(&reqs, &manifest).is_err());
}

#[test]
fn ensure_requirements_passes_with_empty_requirements() {
    assert!(ensure_capability_requirements(
        &CapabilityRequirements::default(),
        &CapabilityManifest::new()
    )
    .is_ok());
}

#[test]
fn ensure_requirements_restricted_satisfies_emulated_min() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };
    let manifest = make_manifest(&[(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    )]);
    assert!(ensure_capability_requirements(&reqs, &manifest).is_ok());
}

#[test]
fn ensure_requirements_multiple_unsatisfied_all_reported() {
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
    let err = ensure_capability_requirements(&reqs, &CapabilityManifest::new()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Streaming"));
    assert!(msg.contains("ToolBash"));
}

#[test]
fn negotiation_incompatible_blocks_execution() {
    let request = NegotiationRequest {
        required: vec![Capability::Logprobs, Capability::SeedDeterminism],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let claude = dialect_manifest("claude");
    let core_manifest: CapabilityManifest = claude
        .iter()
        .filter_map(|(cap, level)| match level {
            DialectSupportLevel::Native => Some((cap.clone(), SupportLevel::Native)),
            DialectSupportLevel::Emulated { .. } => Some((cap.clone(), SupportLevel::Emulated)),
            DialectSupportLevel::Unsupported { .. } => None,
        })
        .collect();
    let result = CapabilityNegotiator::negotiate(&request, &core_manifest);
    assert!(!result.is_compatible);
    assert!(result.unsatisfied.contains(&Capability::Logprobs));
    assert!(result.unsatisfied.contains(&Capability::SeedDeterminism));
}

#[test]
fn check_capabilities_report_blocks_unsupported_route() {
    let wo = WorkOrderBuilder::new("needs logprobs")
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
fn negotiator_native_minimum_rejects_emulated_manifest() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Native,
    };
    let manifest = make_manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(!result.is_compatible);
}

// =========================================================================
// 5. Emulation Labeling in Receipts
// =========================================================================

#[test]
fn fidelity_label_native_for_native_capabilities() {
    let native_caps = vec![Capability::Streaming, Capability::ToolRead];
    let empty_report = abp_emulation::EmulationReport::default();
    let labels = compute_fidelity(&native_caps, &empty_report);
    assert_eq!(
        labels.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
    assert_eq!(
        labels.get(&Capability::ToolRead),
        Some(&FidelityLabel::Native)
    );
}

#[test]
fn fidelity_label_emulated_for_emulated_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    let labels = compute_fidelity(&[], &report);
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn fidelity_labels_mixed_native_and_emulated() {
    let native = vec![Capability::Streaming];
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    let labels = compute_fidelity(&native, &report);

    assert_eq!(
        labels.get(&Capability::Streaming),
        Some(&FidelityLabel::Native)
    );
    assert!(matches!(
        labels.get(&Capability::ExtendedThinking),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn fidelity_labels_omit_disabled_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    let labels = compute_fidelity(&[], &report);
    // CodeExecution default strategy is Disabled, so it appears in warnings, not applied
    assert!(!labels.contains_key(&Capability::CodeExecution));
    assert!(report.has_unemulatable());
}

#[test]
fn emulation_report_tracks_applied_and_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
    ]);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("CodeExecution"));
}

#[test]
fn emulation_config_override_changes_fidelity() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution.".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::CodeExecution]);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());

    let labels = compute_fidelity(&[], &report);
    assert!(matches!(
        labels.get(&Capability::CodeExecution),
        Some(FidelityLabel::Emulated { .. })
    ));
}

#[test]
fn can_emulate_distinguishes_emulatable_from_disabled() {
    assert!(can_emulate(&Capability::ExtendedThinking));
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    assert!(can_emulate(&Capability::ImageInput));
    assert!(can_emulate(&Capability::StopSequences));
    assert!(!can_emulate(&Capability::CodeExecution));
    assert!(!can_emulate(&Capability::Streaming));
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn default_strategy_returns_expected_types() {
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn fidelity_label_serde_roundtrip() {
    let native = FidelityLabel::Native;
    let json = serde_json::to_string(&native).unwrap();
    let back: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, native);

    let emulated = FidelityLabel::Emulated {
        strategy: EmulationStrategy::SystemPromptInjection {
            prompt: "think".into(),
        },
    };
    let json = serde_json::to_string(&emulated).unwrap();
    let back: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(back, emulated);
}

// =========================================================================
// 6. Capability Negotiation Before Execution
// =========================================================================

#[test]
fn check_capabilities_streaming_native_claude_to_claude() {
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
fn check_capabilities_empty_requirements_always_satisfiable() {
    let wo = WorkOrderBuilder::new("task").build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
    assert!(report.entries.is_empty());
}

#[test]
fn check_capabilities_unknown_dialect_returns_all_unsupported() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let report = check_capabilities(&wo, "claude", "nonexistent");
    assert!(!report.all_satisfiable());
    assert_eq!(report.unsupported_capabilities().len(), 1);
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

#[test]
fn capability_report_filters_by_support_level() {
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

#[test]
fn pre_execution_check_combined_with_emulation_decision() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ExtendedThinking,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    let unsupported: Vec<Capability> = report
        .unsupported_capabilities()
        .iter()
        .map(|e| e.capability.clone())
        .collect();
    let emulated: Vec<Capability> = report
        .emulated_capabilities()
        .iter()
        .map(|e| e.capability.clone())
        .collect();

    // ExtendedThinking is unsupported in OpenAI
    assert!(unsupported.contains(&Capability::ExtendedThinking));
    // Streaming is native in OpenAI
    assert!(emulated.is_empty() || !emulated.contains(&Capability::Streaming));
}

// =========================================================================
// 7. Backend Capability Reporting and Comparison
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
    assert_eq!(report.missing, vec![Capability::ToolRead]);
}

#[test]
fn matrix_evaluate_zero_score_unknown_backend() {
    let m = CapabilityMatrix::new();
    let report = m.evaluate("unknown", &[Capability::Streaming]);
    assert_eq!(report.score, 0.0);
}

#[test]
fn matrix_evaluate_empty_requirements_full_score() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    assert_eq!(m.evaluate("a", &[]).score, 1.0);
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
    assert_eq!(
        m.best_backend(&[Capability::Streaming, Capability::ToolRead])
            .as_deref(),
        Some("b")
    );
}

#[test]
fn matrix_all_capabilities_returns_none_for_unknown() {
    let m = CapabilityMatrix::new();
    assert!(m.all_capabilities("ghost").is_none());
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
    assert_eq!(sel.select(&[Capability::ToolRead]).unwrap().name, "a");
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
    assert_eq!(
        sel.select(&[Capability::ToolRead, Capability::Streaming])
            .unwrap()
            .name,
        "b"
    );
}

#[test]
fn selector_priority_picks_lowest_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 10));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    assert_eq!(sel.select(&[Capability::Streaming]).unwrap().name, "b");
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
    let disabled = BackendCandidate {
        enabled: false,
        ..make_candidate("a", vec![Capability::Streaming], 1)
    };
    sel.add_candidate(disabled);
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    assert_eq!(sel.select(&[Capability::Streaming]).unwrap().name, "b");
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
    assert_eq!(sel.select_all(&[Capability::Streaming]).len(), 2);
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
fn selector_select_with_result_success_with_alternatives() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(make_candidate("b", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.alternatives.contains(&"b".to_string()));
    assert!(result.unmet_capabilities.is_empty());
}

#[test]
fn selector_least_loaded_picks_lowest_priority_as_proxy() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(make_candidate("heavy", vec![Capability::Streaming], 100));
    sel.add_candidate(make_candidate("light", vec![Capability::Streaming], 1));
    assert_eq!(sel.select(&[Capability::Streaming]).unwrap().name, "light");
}

#[test]
fn selector_empty_requirements_matches_all() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![Capability::ToolRead], 1));
    assert_eq!(sel.select(&[]).unwrap().name, "a");
}

#[test]
fn selector_candidate_and_enabled_counts() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(make_candidate("a", vec![], 1));
    let disabled = BackendCandidate {
        enabled: false,
        ..make_candidate("b", vec![], 1)
    };
    sel.add_candidate(disabled);
    assert_eq!(sel.candidate_count(), 2);
    assert_eq!(sel.enabled_count(), 1);
}

// =========================================================================
// 8. Feature Matrix Validation Across SDK Pairs
// =========================================================================

#[test]
fn claude_openai_streaming_both_native() {
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
fn claude_lacks_logprobs_openai_has_it() {
    let claude = dialect_manifest("claude");
    let openai = dialect_manifest("openai");
    assert!(matches!(
        claude.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
    assert!(matches!(
        openai.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn openai_lacks_extended_thinking_claude_has_native() {
    let claude = dialect_manifest("claude");
    let openai = dialect_manifest("openai");
    assert!(matches!(
        claude.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        openai.get(&Capability::ExtendedThinking),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn gemini_has_pdf_native_openai_does_not() {
    let gemini = dialect_manifest("gemini");
    let openai = dialect_manifest("openai");
    assert!(matches!(
        gemini.get(&Capability::PdfInput),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        openai.get(&Capability::PdfInput),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn claude_gemini_tool_use_both_native() {
    let claude = dialect_manifest("claude");
    let gemini = dialect_manifest("gemini");
    assert!(matches!(
        claude.get(&Capability::ToolUse),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        gemini.get(&Capability::ToolUse),
        Some(DialectSupportLevel::Native)
    ));
}

#[test]
fn openai_structured_output_native_claude_emulated() {
    let openai = dialect_manifest("openai");
    let claude = dialect_manifest("claude");
    assert!(matches!(
        openai.get(&Capability::StructuredOutputJsonSchema),
        Some(DialectSupportLevel::Native)
    ));
    assert!(matches!(
        claude.get(&Capability::StructuredOutputJsonSchema),
        Some(DialectSupportLevel::Emulated { .. })
    ));
}

#[test]
fn gemini_lacks_logprobs_and_seed() {
    let gemini = dialect_manifest("gemini");
    assert!(matches!(
        gemini.get(&Capability::Logprobs),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
    assert!(matches!(
        gemini.get(&Capability::SeedDeterminism),
        Some(DialectSupportLevel::Unsupported { .. })
    ));
}

#[test]
fn cross_dialect_matrix_all_pairs_streaming() {
    for dialect in ["claude", "openai", "gemini"] {
        let m = dialect_manifest(dialect);
        assert!(
            matches!(
                m.get(&Capability::Streaming),
                Some(DialectSupportLevel::Native)
            ),
            "{dialect} should natively support streaming"
        );
    }
}

#[test]
fn cross_dialect_matrix_stop_sequences_all_native() {
    for dialect in ["claude", "openai", "gemini"] {
        let m = dialect_manifest(dialect);
        assert!(
            matches!(
                m.get(&Capability::StopSequences),
                Some(DialectSupportLevel::Native)
            ),
            "{dialect} should natively support stop sequences"
        );
    }
}

#[test]
fn capability_matrix_cross_sdk_comparison() {
    let mut matrix = CapabilityMatrix::new();
    for (name, dialect) in [
        ("claude", "claude"),
        ("openai", "openai"),
        ("gemini", "gemini"),
    ] {
        let dm = dialect_manifest(dialect);
        let native_caps: Vec<Capability> = dm
            .iter()
            .filter(|(_, level)| matches!(level, DialectSupportLevel::Native))
            .map(|(cap, _)| cap.clone())
            .collect();
        matrix.register(name, native_caps);
    }

    // All three support streaming natively
    assert_eq!(matrix.backends_for(&Capability::Streaming).len(), 3);

    // Only OpenAI has native logprobs
    let logprobs_backends = matrix.backends_for(&Capability::Logprobs);
    assert_eq!(logprobs_backends.len(), 1);
    assert!(logprobs_backends.contains(&"openai".to_string()));

    // Common capabilities across all three
    let common = matrix.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(common.contains(&Capability::StopSequences));
    assert!(!common.contains(&Capability::Logprobs));
}

#[test]
fn best_backend_for_logprobs_is_openai() {
    let mut matrix = CapabilityMatrix::new();
    let openai = dialect_manifest("openai");
    let claude = dialect_manifest("claude");
    let openai_native: Vec<Capability> = openai
        .iter()
        .filter(|(_, l)| matches!(l, DialectSupportLevel::Native))
        .map(|(c, _)| c.clone())
        .collect();
    let claude_native: Vec<Capability> = claude
        .iter()
        .filter(|(_, l)| matches!(l, DialectSupportLevel::Native))
        .map(|(c, _)| c.clone())
        .collect();
    matrix.register("openai", openai_native);
    matrix.register("claude", claude_native);

    assert_eq!(
        matrix
            .best_backend(&[Capability::Streaming, Capability::Logprobs])
            .as_deref(),
        Some("openai")
    );
}

#[test]
fn cross_dialect_check_capabilities_claude_to_openai() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolUse,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::StructuredOutputJsonSchema,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();
    let report = check_capabilities(&wo, "claude", "openai");
    assert!(report.all_satisfiable());
    assert_eq!(report.native_capabilities().len(), 3);
}

#[test]
fn cross_dialect_check_capabilities_openai_to_gemini() {
    let wo = WorkOrderBuilder::new("task")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ImageInput,
                    min_support: MinSupport::Native,
                },
            ],
        })
        .build();
    let report = check_capabilities(&wo, "openai", "gemini");
    assert!(report.all_satisfiable());
}

// =========================================================================
// Sidecar Hello Envelope Capability Exchange
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
fn hello_envelope_discriminator_is_t() {
    let hello = sample_hello_envelope(CapabilityManifest::new());
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
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

// =========================================================================
// Serialization Roundtrips
// =========================================================================

#[test]
fn support_level_all_variants_roundtrip() {
    let levels = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "policy limit".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let _back: SupportLevel = serde_json::from_str(&json).unwrap();
    }
}

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
            reason: "sandbox".into(),
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
fn manifest_deterministic_serialization_btreemap() {
    let mut m1 = CapabilityManifest::new();
    m1.insert(Capability::ToolWrite, SupportLevel::Native);
    m1.insert(Capability::Streaming, SupportLevel::Native);
    m1.insert(Capability::ToolRead, SupportLevel::Native);

    let mut m2 = CapabilityManifest::new();
    m2.insert(Capability::Streaming, SupportLevel::Native);
    m2.insert(Capability::ToolRead, SupportLevel::Native);
    m2.insert(Capability::ToolWrite, SupportLevel::Native);

    assert_eq!(
        serde_json::to_string(&m1).unwrap(),
        serde_json::to_string(&m2).unwrap(),
        "BTreeMap ensures deterministic key order"
    );
}

#[test]
fn emulation_strategy_serde_roundtrip() {
    let strategies = vec![
        EmulationStrategy::SystemPromptInjection {
            prompt: "think".into(),
        },
        EmulationStrategy::PostProcessing {
            detail: "validate".into(),
        },
        EmulationStrategy::Disabled {
            reason: "nope".into(),
        },
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, back);
    }
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
fn diff_detects_upgrade_and_downgrade() {
    let old = make_manifest(&[
        (Capability::ToolRead, SupportLevel::Emulated),
        (Capability::ToolWrite, SupportLevel::Native),
    ]);
    let new = make_manifest(&[
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Emulated),
    ]);
    let diff = CapabilityDiff::diff(&old, &new);
    assert_eq!(diff.upgraded.len(), 1);
    assert_eq!(diff.upgraded[0].0, Capability::ToolRead);
    assert_eq!(diff.downgraded.len(), 1);
    assert_eq!(diff.downgraded[0].0, Capability::ToolWrite);
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
// Edge Cases
// =========================================================================

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
fn negotiator_unsupported_minimum_accepts_everything() {
    let request = NegotiationRequest {
        required: vec![Capability::ToolRead],
        preferred: vec![],
        minimum_support: SupportLevel::Unsupported,
    };
    let manifest = make_manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]);
    let result = CapabilityNegotiator::negotiate(&request, &manifest);
    assert!(result.is_compatible);
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
    let chosen = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(chosen.name, "b");
}

#[test]
fn full_negotiation_pipeline_from_work_order() {
    // Build a work order with requirements
    let wo = WorkOrderBuilder::new("full pipeline test")
        .requirements(CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolUse,
                    min_support: MinSupport::Emulated,
                },
            ],
        })
        .build();

    // Step 1: Pre-execution check against target dialect
    let report = check_capabilities(&wo, "claude", "claude");
    assert!(report.all_satisfiable());

    // Step 2: Build a core manifest and check with ensure_capability_requirements
    let manifest = make_manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
    ]);
    assert!(ensure_capability_requirements(&wo.requirements, &manifest).is_ok());

    // Step 3: Run cap-level negotiation
    let cap_result = cap_negotiate(&manifest, &wo.requirements);
    assert!(cap_result.is_compatible());
    assert_eq!(cap_result.native.len(), 2);

    // Step 4: Generate a compatibility report
    let compat_report = generate_report(&cap_result);
    assert!(compat_report.compatible);
    assert!(compat_report.summary.contains("fully compatible"));
}

// =========================================================================
// 10. CapabilityRegistry Tests
// =========================================================================

#[test]
fn registry_new_is_empty() {
    let reg = CapabilityRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
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
fn registry_names_returns_all_registered() {
    let reg = CapabilityRegistry::with_defaults();
    let names = reg.names();
    assert_eq!(names.len(), 6);
    assert!(names.contains(&"openai/gpt-4o"));
    assert!(names.contains(&"github/copilot"));
}

#[test]
fn registry_register_and_get() {
    let mut reg = CapabilityRegistry::new();
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    reg.register("custom", m);
    assert!(reg.contains("custom"));
    let got = reg.get("custom").unwrap();
    assert!(got.contains_key(&Capability::Streaming));
}

#[test]
fn registry_unregister_removes_entry() {
    let mut reg = CapabilityRegistry::with_defaults();
    assert!(reg.contains("openai/gpt-4o"));
    assert!(reg.unregister("openai/gpt-4o"));
    assert!(!reg.contains("openai/gpt-4o"));
    assert_eq!(reg.len(), 5);
}

#[test]
fn registry_unregister_nonexistent_returns_false() {
    let mut reg = CapabilityRegistry::new();
    assert!(!reg.unregister("ghost"));
}

#[test]
fn registry_get_nonexistent_returns_none() {
    let reg = CapabilityRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn registry_negotiate_by_name_known_backend() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::Streaming])
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 1);
}

#[test]
fn registry_negotiate_by_name_unknown_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg
        .negotiate_by_name("ghost", &[Capability::Streaming])
        .is_none());
}

#[test]
fn registry_negotiate_by_name_unsupported_cap() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name("openai/gpt-4o", &[Capability::ExtendedThinking])
        .unwrap();
    assert!(!result.is_viable());
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn registry_compare_openai_vs_claude() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("openai/gpt-4o", "anthropic/claude-3.5-sonnet")
        .unwrap();
    // Both support streaming natively
    assert!(result.native.contains(&Capability::Streaming));
    // OpenAI has Audio native, Claude doesn't
    assert!(
        result.unsupported_caps().contains(&Capability::Audio)
            || result.emulated_caps().contains(&Capability::Audio)
    );
}

#[test]
fn registry_compare_unknown_source_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("ghost", "openai/gpt-4o").is_none());
}

#[test]
fn registry_compare_unknown_target_returns_none() {
    let reg = CapabilityRegistry::with_defaults();
    assert!(reg.compare("openai/gpt-4o", "ghost").is_none());
}

#[test]
fn registry_query_capability_streaming_across_backends() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Streaming);
    assert_eq!(results.len(), 6);
    for (name, level) in &results {
        assert!(
            matches!(level, CapSupportLevel::Native),
            "{name} should natively support streaming"
        );
    }
}

#[test]
fn registry_query_capability_extended_thinking() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::ExtendedThinking);
    let native_backends: Vec<&str> = results
        .iter()
        .filter(|(_, l)| matches!(l, CapSupportLevel::Native))
        .map(|(n, _)| *n)
        .collect();
    assert!(native_backends.contains(&"anthropic/claude-3.5-sonnet"));
    assert!(!native_backends.contains(&"openai/gpt-4o"));
}

// =========================================================================
// 11. Pre-Negotiation and Policy Application
// =========================================================================

#[test]
fn pre_negotiate_all_native_viable() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolUse, SupportLevel::Native);
    let result = pre_negotiate(&[Capability::Streaming, Capability::ToolUse], &manifest);
    assert_eq!(result.native.len(), 2);
    assert!(result.is_viable());
}

#[test]
fn pre_negotiate_mixed_support() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolUse, SupportLevel::Emulated);
    let result = pre_negotiate(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &manifest,
    );
    assert_eq!(result.native.len(), 1);
    assert_eq!(result.emulated.len(), 1);
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn pre_negotiate_empty_both() {
    let result = pre_negotiate(&[], &CapabilityManifest::new());
    assert!(result.is_viable());
    assert_eq!(result.total(), 0);
}

#[test]
fn pre_negotiate_restricted_counts_as_emulated() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandboxed".into(),
        },
    );
    let result = pre_negotiate(&[Capability::ToolBash], &manifest);
    assert_eq!(result.emulated.len(), 1);
    assert!(result.is_viable());
}

#[test]
fn apply_policy_strict_passes_when_all_supported() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    let result = pre_negotiate(&[Capability::Streaming], &manifest);
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn apply_policy_strict_fails_with_unsupported() {
    let result = pre_negotiate(&[Capability::Vision], &CapabilityManifest::new());
    let err = apply_policy(&result, NegotiationPolicy::Strict).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::Strict);
    assert_eq!(err.unsupported.len(), 1);
}

#[test]
fn apply_policy_strict_allows_emulated() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolUse, SupportLevel::Emulated);
    let result = pre_negotiate(&[Capability::ToolUse], &manifest);
    assert!(apply_policy(&result, NegotiationPolicy::Strict).is_ok());
}

#[test]
fn apply_policy_best_effort_fails_with_unsupported() {
    let result = pre_negotiate(&[Capability::Audio], &CapabilityManifest::new());
    let err = apply_policy(&result, NegotiationPolicy::BestEffort).unwrap_err();
    assert_eq!(err.policy, NegotiationPolicy::BestEffort);
}

#[test]
fn apply_policy_permissive_always_ok() {
    let result = pre_negotiate(
        &[Capability::Streaming, Capability::Vision, Capability::Audio],
        &CapabilityManifest::new(),
    );
    assert!(!result.is_viable());
    assert!(apply_policy(&result, NegotiationPolicy::Permissive).is_ok());
}

#[test]
fn negotiation_policy_default_is_strict() {
    assert_eq!(NegotiationPolicy::default(), NegotiationPolicy::Strict);
}

#[test]
fn negotiation_policy_display() {
    assert_eq!(NegotiationPolicy::Strict.to_string(), "strict");
    assert_eq!(NegotiationPolicy::BestEffort.to_string(), "best-effort");
    assert_eq!(NegotiationPolicy::Permissive.to_string(), "permissive");
}

#[test]
fn negotiation_error_display_includes_policy_and_count() {
    let err = NegotiationError {
        policy: NegotiationPolicy::Strict,
        unsupported: vec![
            (Capability::Vision, "not available".into()),
            (Capability::Audio, "not available".into()),
        ],
        warnings: vec![],
    };
    let msg = err.to_string();
    assert!(msg.contains("strict"));
    assert!(msg.contains("2 unsupported"));
    assert!(msg.contains("Vision"));
    assert!(msg.contains("Audio"));
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
fn negotiation_policy_serde_roundtrip() {
    for policy in [
        NegotiationPolicy::Strict,
        NegotiationPolicy::BestEffort,
        NegotiationPolicy::Permissive,
    ] {
        let json = serde_json::to_string(&policy).unwrap();
        let back: NegotiationPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}

// =========================================================================
// 12. Default Capability Manifests for All 6 Backends
// =========================================================================

#[test]
fn openai_manifest_streaming_native() {
    let m = openai_gpt4o_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolUse),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Vision),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn claude_manifest_extended_thinking_native() {
    let m = claude_35_sonnet_manifest();
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::CacheControl),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::PdfInput),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Audio),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn gemini_manifest_pdf_and_code_native() {
    let m = gemini_15_pro_manifest();
    assert!(matches!(
        m.get(&Capability::PdfInput),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::CodeExecution),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Audio),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Logprobs),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn kimi_manifest_vision_native() {
    let m = kimi_manifest();
    assert!(matches!(
        m.get(&Capability::Vision),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ImageInput),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Audio),
        Some(SupportLevel::Unsupported)
    ));
    assert!(matches!(
        m.get(&Capability::ExtendedThinking),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn codex_manifest_tool_capabilities_native() {
    let m = codex_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolGlob),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolGrep),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::CodeExecution),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn copilot_manifest_web_tools_native() {
    let m = copilot_manifest();
    assert!(matches!(
        m.get(&Capability::ToolWebSearch),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolWebFetch),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolAskUser),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::Audio),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn all_default_manifests_support_streaming_natively() {
    let manifests = [
        ("openai", openai_gpt4o_manifest()),
        ("claude", claude_35_sonnet_manifest()),
        ("gemini", gemini_15_pro_manifest()),
        ("kimi", kimi_manifest()),
        ("codex", codex_manifest()),
        ("copilot", copilot_manifest()),
    ];
    for (name, m) in &manifests {
        assert!(
            matches!(m.get(&Capability::Streaming), Some(SupportLevel::Native)),
            "{name} should natively support streaming"
        );
    }
}

#[test]
fn all_default_manifests_support_system_message() {
    let manifests = [
        ("openai", openai_gpt4o_manifest()),
        ("claude", claude_35_sonnet_manifest()),
        ("gemini", gemini_15_pro_manifest()),
        ("kimi", kimi_manifest()),
        ("codex", codex_manifest()),
        ("copilot", copilot_manifest()),
    ];
    for (name, m) in &manifests {
        assert!(
            matches!(
                m.get(&Capability::SystemMessage),
                Some(SupportLevel::Native)
            ),
            "{name} should natively support system messages"
        );
    }
}

// =========================================================================
// 13. Default Emulation Strategy for Each Capability
// =========================================================================

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
            cap_default_emulation(cap),
            CapEmulationStrategy::ClientSide,
            "{cap:?} should have ClientSide emulation strategy"
        );
    }
}

#[test]
fn default_emulation_strategy_server_fallback_caps() {
    let server_fallback_caps = [
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
    for cap in &server_fallback_caps {
        assert_eq!(
            cap_default_emulation(cap),
            CapEmulationStrategy::ServerFallback,
            "{cap:?} should have ServerFallback emulation strategy"
        );
    }
}

#[test]
fn default_emulation_strategy_approximate_caps() {
    let approximate_caps = [
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
    for cap in &approximate_caps {
        assert_eq!(
            cap_default_emulation(cap),
            CapEmulationStrategy::Approximate,
            "{cap:?} should have Approximate emulation strategy"
        );
    }
}

#[test]
fn default_emulation_strategy_covers_all_41_variants() {
    let all = all_capability_variants();
    for cap in &all {
        // Should not panic for any variant
        let _ = cap_default_emulation(cap);
    }
}

// =========================================================================
// 14. NegotiationResult Edge Cases
// =========================================================================

#[test]
fn negotiation_result_is_viable_alias_is_compatible() {
    let result = CapNegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    assert_eq!(result.is_viable(), result.is_compatible());

    let result2 = CapNegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    assert_eq!(result2.is_viable(), result2.is_compatible());
}

#[test]
fn negotiation_result_display_viable() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![],
    );
    let display = format!("{result}");
    assert!(display.contains("1 native"));
    assert!(display.contains("1 emulated"));
    assert!(display.contains("0 unsupported"));
    assert!(display.contains("viable"));
}

#[test]
fn negotiation_result_display_not_viable() {
    let result = CapNegotiationResult::from_simple(vec![], vec![], vec![Capability::Vision]);
    let display = format!("{result}");
    assert!(display.contains("not viable"));
}

#[test]
fn negotiation_result_warnings_only_approximate() {
    let result = CapNegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![
            (Capability::ToolRead, CapEmulationStrategy::ClientSide),
            (Capability::Vision, CapEmulationStrategy::Approximate),
        ],
        unsupported: vec![],
    };
    let warnings = result.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].0, Capability::Vision);
}

#[test]
fn negotiation_result_from_simple_defaults() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated_caps(), vec![Capability::ToolRead]);
    assert_eq!(result.unsupported_caps(), vec![Capability::Logprobs]);
    // from_simple uses ClientSide for emulated
    assert!(matches!(
        result.emulated[0].1,
        CapEmulationStrategy::ClientSide
    ));
}

#[test]
fn negotiate_capabilities_basic_flow() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);
    manifest.insert(Capability::ToolUse, SupportLevel::Emulated);
    let result = cap_negotiate_caps(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Vision,
        ],
        &manifest,
    );
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated_caps(), vec![Capability::ToolUse]);
    assert_eq!(result.unsupported_caps(), vec![Capability::Vision]);
}

// =========================================================================
// 15. CompatibilityReport Edge Cases
// =========================================================================

#[test]
fn compatibility_report_empty_result() {
    let result = CapNegotiationResult::from_simple(vec![], vec![], vec![]);
    let report = generate_report(&result);
    assert!(report.compatible);
    assert_eq!(report.native_count, 0);
    assert_eq!(report.emulated_count, 0);
    assert_eq!(report.unsupported_count, 0);
    assert!(report.details.is_empty());
}

#[test]
fn compatibility_report_display_uses_summary() {
    let result = CapNegotiationResult::from_simple(vec![Capability::Streaming], vec![], vec![]);
    let report = generate_report(&result);
    assert_eq!(format!("{report}"), report.summary);
}

#[test]
fn compatibility_report_details_match_counts() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming, Capability::ToolUse],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    assert_eq!(report.native_count, 2);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert_eq!(report.details.len(), 4);
}

#[test]
fn compatibility_report_serde_roundtrip() {
    let result = CapNegotiationResult::from_simple(
        vec![Capability::Streaming],
        vec![Capability::ToolRead],
        vec![Capability::Logprobs],
    );
    let report = generate_report(&result);
    let json = serde_json::to_string(&report).unwrap();
    let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.compatible, report.compatible);
    assert_eq!(back.native_count, report.native_count);
    assert_eq!(back.summary, report.summary);
}

// =========================================================================
// 16. Emulation Strategy Types (abp-capability)
// =========================================================================

#[test]
fn cap_emulation_strategy_display() {
    assert_eq!(
        CapEmulationStrategy::ClientSide.to_string(),
        "client-side emulation"
    );
    assert_eq!(
        CapEmulationStrategy::ServerFallback.to_string(),
        "server fallback"
    );
    assert_eq!(CapEmulationStrategy::Approximate.to_string(), "approximate");
}

#[test]
fn cap_emulation_strategy_fidelity_loss() {
    assert!(!CapEmulationStrategy::ClientSide.has_fidelity_loss());
    assert!(!CapEmulationStrategy::ServerFallback.has_fidelity_loss());
    assert!(CapEmulationStrategy::Approximate.has_fidelity_loss());
}

#[test]
fn cap_emulation_strategy_serde_roundtrip() {
    for s in [
        CapEmulationStrategy::ClientSide,
        CapEmulationStrategy::ServerFallback,
        CapEmulationStrategy::Approximate,
    ] {
        let json = serde_json::to_string(&s).unwrap();
        let back: CapEmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}

// =========================================================================
// 17. SupportLevel (abp-capability) Classification
// =========================================================================

#[test]
fn cap_support_level_display_all_variants() {
    assert_eq!(CapSupportLevel::Native.to_string(), "native");
    let emulated = CapSupportLevel::Emulated {
        method: "polyfill".into(),
    };
    assert_eq!(emulated.to_string(), "emulated (polyfill)");
    let restricted = CapSupportLevel::Restricted {
        reason: "sandbox".into(),
    };
    assert_eq!(restricted.to_string(), "restricted (sandbox)");
    let unsupported = CapSupportLevel::Unsupported {
        reason: "not available".into(),
    };
    assert_eq!(unsupported.to_string(), "unsupported (not available)");
}

#[test]
fn cap_support_level_serde_roundtrip() {
    let levels = [
        CapSupportLevel::Native,
        CapSupportLevel::Emulated {
            method: "adapter".into(),
        },
        CapSupportLevel::Restricted {
            reason: "policy".into(),
        },
        CapSupportLevel::Unsupported {
            reason: "gone".into(),
        },
    ];
    for level in &levels {
        let json = serde_json::to_string(level).unwrap();
        let back: CapSupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, level);
    }
}

// =========================================================================
// 18. Cross-Backend Comparison via Registry
// =========================================================================

#[test]
fn registry_compare_codex_vs_copilot() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg.compare("openai/codex", "github/copilot").unwrap();
    // Both support ToolRead, ToolWrite, ToolEdit, ToolBash natively
    for cap in [
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
    ] {
        assert!(
            result.native.contains(&cap),
            "{cap:?} should be native in both codex and copilot"
        );
    }
}

#[test]
fn registry_compare_claude_vs_gemini() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .compare("anthropic/claude-3.5-sonnet", "google/gemini-1.5-pro")
        .unwrap();
    // Claude has ExtendedThinking native, Gemini doesn't
    assert!(
        result
            .unsupported_caps()
            .contains(&Capability::ExtendedThinking),
        "ExtendedThinking should be unsupported when comparing Claude to Gemini"
    );
}

#[test]
fn registry_negotiate_codex_with_tool_requirements() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name(
            "openai/codex",
            &[
                Capability::ToolRead,
                Capability::ToolWrite,
                Capability::ToolBash,
                Capability::CodeExecution,
                Capability::Streaming,
            ],
        )
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 5);
}

#[test]
fn registry_negotiate_copilot_web_tools() {
    let reg = CapabilityRegistry::with_defaults();
    let result = reg
        .negotiate_by_name(
            "github/copilot",
            &[
                Capability::ToolWebSearch,
                Capability::ToolWebFetch,
                Capability::ToolAskUser,
            ],
        )
        .unwrap();
    assert!(result.is_viable());
    assert_eq!(result.native.len(), 3);
}

#[test]
fn registry_query_audio_shows_limited_support() {
    let reg = CapabilityRegistry::with_defaults();
    let results = reg.query_capability(&Capability::Audio);
    let native_backends: Vec<&str> = results
        .iter()
        .filter(|(_, l)| matches!(l, CapSupportLevel::Native))
        .map(|(n, _)| *n)
        .collect();
    // Only OpenAI and Gemini support audio natively
    assert!(native_backends.contains(&"openai/gpt-4o"));
    assert!(native_backends.contains(&"google/gemini-1.5-pro"));
    assert!(!native_backends.contains(&"anthropic/claude-3.5-sonnet"));
}

// =========================================================================
// 19. MinSupport Threshold with negotiate()
// =========================================================================

#[test]
fn negotiate_native_min_rejects_emulated_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert!(!result.is_compatible());
    assert_eq!(result.unsupported.len(), 1);
}

#[test]
fn negotiate_emulated_min_accepts_native_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.native.len(), 1);
}

#[test]
fn negotiate_emulated_min_accepts_restricted_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "sandbox".into(),
        },
    );
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolBash,
            min_support: MinSupport::Emulated,
        }],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert!(result.is_compatible());
    assert_eq!(result.emulated.len(), 1);
}
