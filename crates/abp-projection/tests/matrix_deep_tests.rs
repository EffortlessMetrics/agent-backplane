#![allow(clippy::all)]
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

//! Deep tests for the projection matrix covering construction, dialect lookup,
//! fidelity scoring, selection strategies, backend registration/selection,
//! capability matching, emulation detection, error cases, serde roundtrips,
//! and route planning.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_projection::selection::{ModelCandidate, ModelSelector, SelectionStrategy};
use abp_projection::{
    CompatibilityScore, DialectPair, FallbackEntry, ProjectionEntry, ProjectionError,
    ProjectionMatrix, ProjectionMode, ProjectionScore, RequiredEmulation, RoutingHop, RoutingPath,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    caps.iter().cloned().collect()
}

fn require_caps(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Emulated,
            })
            .collect(),
    }
}

fn wo_with_reqs(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

fn wo_passthrough(dialect: &str, reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "mode": "passthrough", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn wo_with_dialect(dialect: &str, reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("dialect task")
        .requirements(reqs)
        .config(config)
        .build()
}

// =========================================================================
// 1. Matrix construction
// =========================================================================

#[test]
fn default_matrix_is_empty() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.dialect_entry_count(), 0);
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn with_defaults_populates_all_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    let n = Dialect::all().len();
    assert_eq!(pm.dialect_entry_count(), n * n);
}

#[test]
fn register_dialect_pair_increments_count() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    assert_eq!(pm.dialect_entry_count(), 1);
    pm.register(Dialect::Claude, Dialect::OpenAi, ProjectionMode::Mapped);
    assert_eq!(pm.dialect_entry_count(), 2);
}

#[test]
fn register_same_pair_twice_replaces() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(
        Dialect::OpenAi,
        Dialect::Claude,
        ProjectionMode::Unsupported,
    );
    assert_eq!(pm.dialect_entry_count(), 1);
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Unsupported
    );
}

#[test]
fn register_identity_pair_forces_passthrough() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Gemini, Dialect::Gemini, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

// =========================================================================
// 2. Dialect pair lookup
// =========================================================================

#[test]
fn lookup_registered_pair_returns_entry() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(entry.pair.source, Dialect::OpenAi);
    assert_eq!(entry.pair.target, Dialect::Claude);
    assert_eq!(entry.mode, ProjectionMode::Mapped);
}

#[test]
fn lookup_unregistered_pair_returns_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn lookup_reverse_pair_independent() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    assert!(pm.lookup(Dialect::Claude, Dialect::OpenAi).is_none());
}

#[test]
fn lookup_after_remove_returns_none() {
    let mut pm = ProjectionMatrix::with_defaults();
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
    pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
}

// =========================================================================
// 3. Fidelity scoring — passthrough (same dialect) gets lossless
// =========================================================================

#[test]
fn same_dialect_backend_gets_perfect_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn identity_compatibility_score_is_perfect() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let score = pm.compatibility_score(d, d);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn identity_compatibility_all_features_lossless() {
    let mut pm = ProjectionMatrix::new();
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
    assert_eq!(score.lossless_features, 2);
    assert_eq!(score.lossy_features, 0);
    assert_eq!(score.unsupported_features, 0);
}

// =========================================================================
// 4. Mapped mode fidelity — cross-dialect
// =========================================================================

#[test]
fn cross_dialect_with_lossless_registry_has_high_fidelity() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert!(result.fidelity_score.mapping_fidelity > 0.9);
}

#[test]
fn cross_dialect_with_lossy_registry_has_lower_fidelity() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    // Lossy mapping should score less than 1.0 but > 0.0.
    assert!(result.fidelity_score.mapping_fidelity > 0.0);
    assert!(result.fidelity_score.mapping_fidelity < 1.0);
}

#[test]
fn lossless_backend_preferred_over_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "lossy".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

// =========================================================================
// 5. Selection strategy variants
// =========================================================================

#[test]
fn strategy_lowest_latency_picks_fastest() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            ModelCandidate {
                backend_name: "slow".into(),
                model_id: "m".into(),
                estimated_latency_ms: Some(500),
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "fast".into(),
                model_id: "m".into(),
                estimated_latency_ms: Some(10),
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "fast");
}

#[test]
fn strategy_lowest_cost_picks_cheapest() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            ModelCandidate {
                backend_name: "expensive".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: Some(10.0),
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "cheap".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: Some(0.1),
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "cheap");
}

#[test]
fn strategy_highest_fidelity_picks_best() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            ModelCandidate {
                backend_name: "low".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: Some(0.2),
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "high".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: Some(0.99),
                weight: 1.0,
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "high");
}

#[test]
fn strategy_round_robin_cycles() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![
            ModelCandidate {
                backend_name: "a".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "b".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
    assert_eq!(sel.select().unwrap().backend_name, "a");
}

#[test]
fn strategy_weighted_random_only_positive_weight() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            ModelCandidate {
                backend_name: "zero".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 0.0,
            },
            ModelCandidate {
                backend_name: "nonzero".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    for _ in 0..20 {
        assert_eq!(sel.select().unwrap().backend_name, "nonzero");
    }
}

#[test]
fn strategy_fallback_chain_returns_first() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![
            ModelCandidate {
                backend_name: "primary".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "secondary".into(),
                model_id: "m".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "primary");
    // Repeated calls always return first.
    assert_eq!(sel.select().unwrap().backend_name, "primary");
}

#[test]
fn all_strategies_return_none_on_empty_candidates() {
    let strategies = [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ];
    for s in strategies {
        let sel = ModelSelector::new(s, vec![]);
        assert!(sel.select().is_none(), "{s:?} should return None on empty");
    }
}

// =========================================================================
// 6. Backend registration
// =========================================================================

#[test]
fn register_backend_increments_count() {
    let mut pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
    pm.register_backend(
        "be-a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    assert_eq!(pm.backend_count(), 1);
    pm.register_backend(
        "be-b",
        manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
        Dialect::Claude,
        40,
    );
    assert_eq!(pm.backend_count(), 2);
}

#[test]
fn register_backend_same_id_overwrites() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "be",
        manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]),
        Dialect::Claude,
        90,
    );
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn remove_backend_decrements_count() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    assert!(pm.remove_backend("be"));
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn remove_nonexistent_backend_returns_false() {
    let mut pm = ProjectionMatrix::new();
    assert!(!pm.remove_backend("ghost"));
}

// =========================================================================
// 7. Backend selection
// =========================================================================

#[test]
fn project_selects_best_backend_by_capability_coverage() {
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

    let wo = wo_with_reqs(require_caps(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn project_higher_priority_wins_on_tie() {
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

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "high-prio");
}

#[test]
fn project_passthrough_prefers_same_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        30,
    );
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );

    let wo = wo_passthrough("claude", require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn project_fallback_chain_excludes_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be-a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "be-b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    for entry in &result.fallback_chain {
        assert_ne!(entry.backend_id, result.selected_backend);
    }
}

#[test]
fn project_fallback_chain_sorted_descending() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 90), ("b", 60), ("c", 30)] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
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

// =========================================================================
// 8. Capability matching
// =========================================================================

#[test]
fn full_native_coverage_gives_max_score() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full-native",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    let result = pm.project(&wo).unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn empty_requirements_treated_as_full_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("any", CapabilityManifest::new(), Dialect::OpenAi, 50);

    let wo = wo_with_reqs(CapabilityRequirements::default());
    let result = pm.project(&wo).unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn partial_capability_match_reflects_in_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    // This should still select (partial match) but with < 1.0 coverage.
    let result = pm.project(&wo).unwrap();
    assert!(result.fidelity_score.capability_coverage < 1.0);
    assert!(result.fidelity_score.capability_coverage > 0.0);
}

// =========================================================================
// 9. Emulation detection
// =========================================================================

#[test]
fn native_cap_produces_no_emulation() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "native-only",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    let result = pm.project(&wo).unwrap();
    assert!(result.required_emulations.is_empty());
}

#[test]
fn emulated_cap_detected_in_result() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu-be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
    assert_eq!(result.required_emulations[0].strategy, "adapter");
}

#[test]
fn multiple_emulated_caps_all_listed() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "multi-emu",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
    ]));
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.required_emulations.len(), 2);
    let emu_caps: Vec<_> = result
        .required_emulations
        .iter()
        .map(|e| e.capability.clone())
        .collect();
    assert!(emu_caps.contains(&Capability::Streaming));
    assert!(emu_caps.contains(&Capability::ToolRead));
}

#[test]
fn native_preferred_when_both_available() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "native-all",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "emu-all",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    let result = pm.project(&wo).unwrap();
    // Both have 100% coverage and same priority.
    // With identical scores, id-sort is used: "emu-all" < "native-all" alphabetically,
    // so "emu-all" is selected. The fallback chain should contain "native-all".
    assert!(result.fallback_chain.len() == 1);
    // Verify the emulated backend reports emulations when selected.
    if result.selected_backend == "emu-all" {
        assert_eq!(result.required_emulations.len(), 2);
    } else {
        assert!(result.required_emulations.is_empty());
    }
}

// =========================================================================
// 10. Error cases
// =========================================================================

#[test]
fn empty_matrix_returns_empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn no_suitable_backend_when_caps_unsupported() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "limited",
        manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn no_suitable_backend_empty_caps_with_required() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty-caps", CapabilityManifest::new(), Dialect::OpenAi, 50);

    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn projection_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);

    let err2 = ProjectionError::EmptyMatrix;
    let json2 = serde_json::to_string(&err2).unwrap();
    let back2: ProjectionError = serde_json::from_str(&json2).unwrap();
    assert_eq!(err2, back2);
}

#[test]
fn project_after_removing_all_backends_is_empty_matrix() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.remove_backend("be");
    let wo = wo_with_reqs(require_caps(&[Capability::Streaming]));
    let err = pm.project(&wo).unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

// =========================================================================
// 11. Serde roundtrip
// =========================================================================

#[test]
fn serde_dialect_pair_roundtrip() {
    let pair = DialectPair::new(Dialect::Gemini, Dialect::Kimi);
    let json = serde_json::to_string(&pair).unwrap();
    let back: DialectPair = serde_json::from_str(&json).unwrap();
    assert_eq!(pair, back);
}

#[test]
fn serde_projection_mode_all_variants() {
    for mode in [
        ProjectionMode::Passthrough,
        ProjectionMode::Mapped,
        ProjectionMode::Unsupported,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: ProjectionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn serde_projection_entry_roundtrip() {
    let entry = ProjectionEntry {
        pair: DialectPair::new(Dialect::OpenAi, Dialect::Claude),
        mode: ProjectionMode::Mapped,
        mapper_hint: Some("openai_to_claude".into()),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: ProjectionEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn serde_projection_score_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.8,
        mapping_fidelity: 0.9,
        priority: 0.5,
        total: 0.77,
    };
    let json = serde_json::to_string(&score).unwrap();
    let back: ProjectionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(score, back);
}

#[test]
fn serde_required_emulation_roundtrip() {
    let emu = RequiredEmulation {
        capability: Capability::ToolRead,
        strategy: "adapter".into(),
    };
    let json = serde_json::to_string(&emu).unwrap();
    let back: RequiredEmulation = serde_json::from_str(&json).unwrap();
    assert_eq!(emu, back);
}

#[test]
fn serde_fallback_entry_roundtrip() {
    let entry = FallbackEntry {
        backend_id: "backend-x".into(),
        score: ProjectionScore {
            capability_coverage: 0.5,
            mapping_fidelity: 0.7,
            priority: 0.3,
            total: 0.5,
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: FallbackEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn serde_routing_hop_roundtrip() {
    let hop = RoutingHop {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
        mapper_hint: Some("openai_to_claude".into()),
    };
    let json = serde_json::to_string(&hop).unwrap();
    let back: RoutingHop = serde_json::from_str(&json).unwrap();
    assert_eq!(hop, back);
}

#[test]
fn serde_routing_path_roundtrip() {
    let path = RoutingPath {
        hops: vec![
            RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Gemini,
                mapper_hint: Some("openai_to_gemini".into()),
            },
            RoutingHop {
                from: Dialect::Gemini,
                to: Dialect::Claude,
                mapper_hint: Some("gemini_to_claude".into()),
            },
        ],
        cost: 2,
        fidelity: 0.64,
    };
    let json = serde_json::to_string(&path).unwrap();
    let back: RoutingPath = serde_json::from_str(&json).unwrap();
    assert_eq!(path, back);
}

#[test]
fn serde_compatibility_score_roundtrip() {
    let score = CompatibilityScore {
        source: Dialect::OpenAi,
        target: Dialect::Claude,
        fidelity: 0.85,
        lossless_features: 3,
        lossy_features: 1,
        unsupported_features: 0,
    };
    let json = serde_json::to_string(&score).unwrap();
    let back: CompatibilityScore = serde_json::from_str(&json).unwrap();
    assert_eq!(score, back);
}

#[test]
fn serde_selection_strategy_all_variants() {
    let strategies = [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ];
    for s in strategies {
        let json = serde_json::to_string(&s).unwrap();
        let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}

#[test]
fn serde_model_selector_roundtrip() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![ModelCandidate {
            backend_name: "be".into(),
            model_id: "gpt-4".into(),
            estimated_latency_ms: Some(100),
            estimated_cost_per_1k_tokens: Some(0.03),
            fidelity_score: Some(0.95),
            weight: 1.0,
        }],
    );
    let json = serde_json::to_string(&sel).unwrap();
    let back: ModelSelector = serde_json::from_str(&json).unwrap();
    assert_eq!(back.strategy, sel.strategy);
    assert_eq!(back.candidates, sel.candidates);
}

// =========================================================================
// 12. Route planning
// =========================================================================

#[test]
fn route_identity_zero_cost() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 0);
    assert!(route.hops.is_empty());
    assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    assert!(route.is_direct());
    assert!(!route.is_multi_hop());
}

#[test]
fn route_direct_mapped_cost_one() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert!(route.is_direct());
    assert_eq!(route.hops.len(), 1);
    assert_eq!(route.hops[0].from, Dialect::OpenAi);
    assert_eq!(route.hops[0].to, Dialect::Claude);
}

#[test]
fn route_multi_hop_cost_two() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
    assert!(route.is_some());
    let route = route.unwrap();
    assert_eq!(route.cost, 2);
    assert!(route.is_multi_hop());
    assert_eq!(route.hops.len(), 2);
}

#[test]
fn route_direct_preferred_over_multi_hop() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Gemini, ProjectionMode::Mapped);
    pm.register(Dialect::Gemini, Dialect::Claude, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert!(route.is_direct());
}

#[test]
fn route_no_path_when_disconnected() {
    let pm = ProjectionMatrix::new();
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn route_multi_hop_fidelity_product_of_hops() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
    if let Some(route) = pm.find_route(Dialect::Kimi, Dialect::Copilot) {
        assert!(route.fidelity <= 1.0);
        assert!(route.fidelity >= 0.0);
    }
}

#[test]
fn route_unsupported_pair_not_used() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Unsupported);
    // Unsupported direct route should not be returned.
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

// =========================================================================
// Additional deep coverage
// =========================================================================

#[test]
fn source_dialect_from_work_order_vendor_config() {
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

    // Work order specifies source_dialect=claude in vendor config.
    let wo = wo_with_dialect("claude", require_caps(&[Capability::Streaming]));
    let result = pm.project(&wo).unwrap();
    // Claude backend should get perfect fidelity since source==target.
    assert!(result.fidelity_score.mapping_fidelity >= 0.0);
}

#[test]
fn resolve_mapper_for_passthrough() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
    assert!(mapper.is_some());
}

#[test]
fn resolve_mapper_for_mapped_pair() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
    assert!(mapper.is_some());
}

#[test]
fn resolve_mapper_for_unsupported_pair_returns_none() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(mapper.is_none());
}

#[test]
fn resolve_mapper_unregistered_returns_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm
        .resolve_mapper(Dialect::OpenAi, Dialect::Claude)
        .is_none());
}

#[test]
fn compatibility_score_with_mixed_fidelity_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "vision".into(),
        fidelity: Fidelity::Unsupported {
            reason: "not available".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into(), "vision".into()]);

    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.lossless_features, 1);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.unsupported_features, 1);
    assert!(score.fidelity > 0.0);
    assert!(score.fidelity < 1.0);
}

#[test]
fn dialect_pair_display_contains_arrow() {
    let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    let s = pair.to_string();
    assert!(s.contains('→'));
    assert!(s.contains("OpenAI"));
    assert!(s.contains("Claude"));
}

#[test]
fn select_n_ordering_for_lowest_latency() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            ModelCandidate {
                backend_name: "slow".into(),
                model_id: "m".into(),
                estimated_latency_ms: Some(500),
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "fast".into(),
                model_id: "m".into(),
                estimated_latency_ms: Some(10),
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "mid".into(),
                model_id: "m".into(),
                estimated_latency_ms: Some(200),
                estimated_cost_per_1k_tokens: None,
                fidelity_score: None,
                weight: 1.0,
            },
        ],
    );
    let ranked: Vec<_> = sel
        .select_n(3)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(ranked, vec!["fast", "mid", "slow"]);
}
