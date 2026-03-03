// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for capability negotiation across the Agent Backplane.

use std::collections::BTreeMap;

use abp_backend_core::ensure_capability_requirements;
use abp_capability::{
    NegotiationResult as CapNegotiationResult, SupportLevel as CapSupportLevel, check_capability,
    generate_report, negotiate as cap_negotiate,
};
use abp_core::negotiate::{
    CapabilityDiff, CapabilityNegotiator, CapabilityReport as NegotiateCapabilityReport,
    CapabilityReportEntry, DialectSupportLevel, NegotiationRequest, check_capabilities,
    dialect_manifest,
};
use abp_core::{
    BackendIdentity, Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, SupportLevel, WorkOrderBuilder,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationStrategy, FidelityLabel, can_emulate,
    compute_fidelity, default_strategy,
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
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Logprobs,
                min_support: MinSupport::Native,
            },
        ],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert_eq!(result.native, vec![Capability::Streaming]);
    assert_eq!(result.emulated, vec![Capability::ToolRead]);
    assert_eq!(result.unsupported, vec![Capability::Logprobs]);
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
    assert_eq!(result.unsupported, vec![Capability::Streaming]);
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
            min_support: MinSupport::Native,
        }],
    };
    let result = cap_negotiate(&manifest, &reqs);
    assert_eq!(result.emulated, vec![Capability::ToolBash]);
    assert!(result.is_compatible());
}

#[test]
fn cap_negotiate_total_counts_all_buckets() {
    let result = CapNegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![Capability::ToolRead],
        unsupported: vec![Capability::Logprobs],
    };
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
    assert!(matches!(level, CapSupportLevel::Emulated { strategy } if strategy == "adapter"));
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
        CapSupportLevel::Emulated { strategy } => {
            assert!(strategy.contains("restricted"));
            assert!(strategy.contains("policy"));
        }
        other => panic!("expected Emulated, got {other:?}"),
    }
}

#[test]
fn check_capability_missing_returns_unsupported() {
    let manifest = CapabilityManifest::new();
    assert_eq!(
        check_capability(&manifest, &Capability::Streaming),
        CapSupportLevel::Unsupported
    );
}

#[test]
fn check_capability_explicit_unsupported_returns_unsupported() {
    let manifest = make_manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]);
    assert_eq!(
        check_capability(&manifest, &Capability::Logprobs),
        CapSupportLevel::Unsupported
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
    let result = CapNegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![Capability::ToolRead],
        unsupported: vec![Capability::Logprobs],
    };
    let report = generate_report(&result);
    assert!(!report.compatible);
    assert_eq!(report.native_count, 1);
    assert_eq!(report.emulated_count, 1);
    assert_eq!(report.unsupported_count, 1);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn generate_report_fully_compatible_when_no_unsupported() {
    let result = CapNegotiationResult {
        native: vec![Capability::Streaming],
        emulated: vec![Capability::ToolRead],
        unsupported: vec![],
    };
    let report = generate_report(&result);
    assert!(report.compatible);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn generate_report_incompatible_when_unsupported_present() {
    let result = CapNegotiationResult {
        native: vec![],
        emulated: vec![],
        unsupported: vec![Capability::Logprobs],
    };
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
    assert!(
        ensure_capability_requirements(
            &CapabilityRequirements::default(),
            &CapabilityManifest::new()
        )
        .is_ok()
    );
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
    assert!(labels.get(&Capability::CodeExecution).is_none());
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
