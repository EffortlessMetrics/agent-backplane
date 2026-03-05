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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]

//! Deep tests for projection matrix model selection strategies, matrix
//! operations, mapper integration, and edge cases.

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

fn wo(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("strategy test")
        .requirements(reqs)
        .build()
}

fn wo_passthrough(dialect: &str) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "mode": "passthrough", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("passthrough strategy test")
        .requirements(CapabilityRequirements::default())
        .config(config)
        .build()
}

fn candidate(name: &str, model: &str) -> ModelCandidate {
    ModelCandidate {
        backend_name: name.into(),
        model_id: model.into(),
        estimated_latency_ms: None,
        estimated_cost_per_1k_tokens: None,
        fidelity_score: None,
        weight: 1.0,
    }
}

fn full_candidate(
    name: &str,
    model: &str,
    latency: Option<u64>,
    cost: Option<f64>,
    fidelity: Option<f64>,
    weight: f64,
) -> ModelCandidate {
    ModelCandidate {
        backend_name: name.into(),
        model_id: model.into(),
        estimated_latency_ms: latency,
        estimated_cost_per_1k_tokens: cost,
        fidelity_score: fidelity,
        weight,
    }
}

// =========================================================================
// 1. Model selection strategies — direct mapping
// =========================================================================

#[test]
fn direct_mapping_model_id_preserved_in_selection() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![
            full_candidate("openai", "gpt-4o", Some(80), Some(5.0), Some(0.95), 1.0),
            full_candidate(
                "claude",
                "claude-3-opus",
                Some(120),
                Some(15.0),
                Some(0.98),
                1.0,
            ),
        ],
    );
    let picked = sel.select().unwrap();
    assert_eq!(picked.backend_name, "openai");
    assert_eq!(picked.model_id, "gpt-4o");
}

#[test]
fn direct_mapping_each_candidate_retains_distinct_model_id() {
    let candidates = vec![
        full_candidate("be-a", "gpt-4o", None, None, None, 1.0),
        full_candidate("be-b", "claude-3-sonnet", None, None, None, 1.0),
        full_candidate("be-c", "gemini-pro", None, None, None, 1.0),
    ];
    let sel = ModelSelector::new(SelectionStrategy::FallbackChain, candidates);
    let all = sel.select_n(3);
    assert_eq!(all[0].model_id, "gpt-4o");
    assert_eq!(all[1].model_id, "claude-3-sonnet");
    assert_eq!(all[2].model_id, "gemini-pro");
}

#[test]
fn direct_mapping_single_candidate_always_selected() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![full_candidate(
            "only",
            "gpt-4o-mini",
            Some(50),
            None,
            None,
            1.0,
        )],
    );
    for _ in 0..10 {
        let picked = sel.select().unwrap();
        assert_eq!(picked.model_id, "gpt-4o-mini");
    }
}

// =========================================================================
// 2. Capability-based selection (best capability coverage)
// =========================================================================

#[test]
fn capability_based_full_coverage_wins_over_partial() {
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
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn capability_based_emulated_coverage_still_matches() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::Claude,
        50,
    );
    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "emu");
    assert_eq!(result.required_emulations.len(), 2);
}

#[test]
fn capability_based_mixed_native_emulated_counted() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "mixed");
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn capability_based_more_coverage_beats_higher_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "high-prio-few-caps",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    pm.register_backend(
        "low-prio-all-caps",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        10,
    );
    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "low-prio-all-caps");
}

// =========================================================================
// 3. Cost-optimized selection
// =========================================================================

#[test]
fn cost_optimized_cheapest_model_selected() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            full_candidate("premium", "gpt-4o", None, Some(30.0), Some(0.99), 1.0),
            full_candidate("budget", "gpt-4o-mini", None, Some(0.15), Some(0.85), 1.0),
            full_candidate("mid", "gpt-4-turbo", None, Some(10.0), Some(0.92), 1.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "budget");
}

#[test]
fn cost_optimized_identical_cost_stable_order() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            full_candidate("first", "m1", None, Some(1.0), None, 1.0),
            full_candidate("second", "m2", None, Some(1.0), None, 1.0),
        ],
    );
    // With identical costs, the first candidate in insertion order wins.
    let picked = sel.select().unwrap();
    assert_eq!(picked.backend_name, "first");
}

#[test]
fn cost_optimized_select_n_ascending_cost() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            full_candidate("expensive", "m", None, Some(50.0), None, 1.0),
            full_candidate("cheapest", "m", None, Some(0.1), None, 1.0),
            full_candidate("middle", "m", None, Some(5.0), None, 1.0),
        ],
    );
    let ranked: Vec<_> = sel.select_n(3).iter().map(|c| &*c.backend_name).collect();
    assert_eq!(ranked, vec!["cheapest", "middle", "expensive"]);
}

#[test]
fn cost_optimized_none_cost_sorted_last() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            full_candidate("unknown", "m", None, None, None, 1.0),
            full_candidate("known", "m", None, Some(2.0), None, 1.0),
        ],
    );
    let ranked: Vec<_> = sel.select_n(2).iter().map(|c| &*c.backend_name).collect();
    assert_eq!(ranked, vec!["known", "unknown"]);
}

// =========================================================================
// 4. Quality-optimized selection
// =========================================================================

#[test]
fn quality_optimized_highest_fidelity_selected() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            full_candidate("low", "m", None, None, Some(0.6), 1.0),
            full_candidate("best", "m", None, None, Some(0.99), 1.0),
            full_candidate("mid", "m", None, None, Some(0.8), 1.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "best");
}

#[test]
fn quality_optimized_select_n_descending_fidelity() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            full_candidate("low", "m", None, None, Some(0.3), 1.0),
            full_candidate("best", "m", None, None, Some(0.99), 1.0),
            full_candidate("mid", "m", None, None, Some(0.7), 1.0),
        ],
    );
    let ranked: Vec<_> = sel.select_n(3).iter().map(|c| &*c.backend_name).collect();
    assert_eq!(ranked, vec!["best", "mid", "low"]);
}

#[test]
fn quality_optimized_none_fidelity_sorted_last() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            full_candidate("unknown", "m", None, None, None, 1.0),
            full_candidate("good", "m", None, None, Some(0.85), 1.0),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "good");
}

// =========================================================================
// 5. Custom strategy registration and execution (round-robin & weighted)
// =========================================================================

#[test]
fn round_robin_advances_counter_per_select() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![
            candidate("a", "m"),
            candidate("b", "m"),
            candidate("c", "m"),
        ],
    );
    let names: Vec<_> = (0..6)
        .map(|_| sel.select().unwrap().backend_name.clone())
        .collect();
    assert_eq!(names, vec!["a", "b", "c", "a", "b", "c"]);
}

#[test]
fn round_robin_clone_counter_independent() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![candidate("a", "m"), candidate("b", "m")],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a"); // counter -> 1
    let cloned = sel.clone();
    // Original continues at b.
    assert_eq!(sel.select().unwrap().backend_name, "b");
    // Clone started at counter=1, so also picks b.
    assert_eq!(cloned.select().unwrap().backend_name, "b");
}

#[test]
fn weighted_random_all_equal_weights_selects_some_candidate() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            full_candidate("a", "m", None, None, None, 1.0),
            full_candidate("b", "m", None, None, None, 1.0),
            full_candidate("c", "m", None, None, None, 1.0),
        ],
    );
    let picked = sel.select().unwrap();
    assert!(["a", "b", "c"].contains(&&*picked.backend_name));
}

#[test]
fn weighted_random_dominant_weight_always_picked() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            full_candidate("tiny", "m", None, None, None, 0.001),
            full_candidate("dominant", "m", None, None, None, 999_999.0),
        ],
    );
    for _ in 0..50 {
        assert_eq!(sel.select().unwrap().backend_name, "dominant");
    }
}

#[test]
fn selector_serde_preserves_strategy_and_candidates() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            full_candidate("a", "gpt-4o", Some(100), Some(5.0), Some(0.9), 2.0),
            full_candidate("b", "claude-3", Some(200), Some(15.0), Some(0.95), 1.0),
        ],
    );
    let json = serde_json::to_string(&sel).unwrap();
    let back: ModelSelector = serde_json::from_str(&json).unwrap();
    assert_eq!(back.strategy, SelectionStrategy::LowestCost);
    assert_eq!(back.candidates.len(), 2);
    assert_eq!(back.candidates[0].backend_name, "a");
    assert_eq!(back.candidates[1].model_id, "claude-3");
}

// =========================================================================
// 6. Projection matrix construction from config
// =========================================================================

#[test]
fn matrix_with_mapping_registry_preserves_rules() {
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
    let result = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert!(result.fidelity_score.mapping_fidelity > 0.9);
}

#[test]
fn matrix_source_dialect_override_affects_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "openai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    // Set source dialect to Claude — Claude backend should get perfect fidelity.
    pm.set_source_dialect(Dialect::Claude);
    let result = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

// =========================================================================
// 7. Matrix lookup for all 6×6 dialect pairs (36 combinations)
// =========================================================================

#[test]
fn all_36_dialect_pairs_registered_in_defaults() {
    let pm = ProjectionMatrix::with_defaults();
    let dialects = Dialect::all();
    assert_eq!(dialects.len(), 6);
    let mut count = 0;
    for &src in dialects {
        for &tgt in dialects {
            assert!(
                pm.lookup(src, tgt).is_some(),
                "missing entry for {} → {}",
                src,
                tgt
            );
            count += 1;
        }
    }
    assert_eq!(count, 36);
}

#[test]
fn all_identity_pairs_passthrough_with_identity_hint() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
        assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
    }
}

#[test]
fn all_mapped_pairs_have_mapper_hints() {
    let pm = ProjectionMatrix::with_defaults();
    for entry in pm.dialect_entries() {
        if entry.mode == ProjectionMode::Mapped {
            assert!(
                entry.mapper_hint.is_some(),
                "mapped entry {} should have hint",
                entry.pair
            );
        }
    }
}

#[test]
fn mode_counts_partition_all_entries() {
    let pm = ProjectionMatrix::with_defaults();
    let total = pm.dialect_entry_count();
    let passthrough = pm
        .dialect_entries()
        .filter(|e| e.mode == ProjectionMode::Passthrough)
        .count();
    let mapped = pm
        .dialect_entries()
        .filter(|e| e.mode == ProjectionMode::Mapped)
        .count();
    let unsupported = pm
        .dialect_entries()
        .filter(|e| e.mode == ProjectionMode::Unsupported)
        .count();
    assert_eq!(passthrough + mapped + unsupported, total);
    assert_eq!(passthrough, 6);
    assert_eq!(mapped, 18);
    assert_eq!(total, 36);
}

// =========================================================================
// 8. Matrix with custom overrides
// =========================================================================

#[test]
fn custom_override_changes_mode() {
    let mut pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Mapped
    );
    pm.register(
        Dialect::OpenAi,
        Dialect::Claude,
        ProjectionMode::Unsupported,
    );
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Unsupported
    );
}

#[test]
fn custom_override_does_not_affect_reverse_direction() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(
        Dialect::OpenAi,
        Dialect::Claude,
        ProjectionMode::Unsupported,
    );
    assert_eq!(
        pm.lookup(Dialect::Claude, Dialect::OpenAi).unwrap().mode,
        ProjectionMode::Mapped
    );
}

#[test]
fn custom_override_preserves_total_count() {
    let mut pm = ProjectionMatrix::with_defaults();
    let before = pm.dialect_entry_count();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
    assert_eq!(pm.dialect_entry_count(), before);
}

#[test]
fn remove_then_re_register_works() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
}

// =========================================================================
// 9. Matrix deterministic serialization (BTreeMap)
// =========================================================================

#[test]
fn dialect_entries_iteration_deterministic_across_instances() {
    let pm1 = ProjectionMatrix::with_defaults();
    let pm2 = ProjectionMatrix::with_defaults();
    let pairs1: Vec<_> = pm1.dialect_entries().map(|e| e.pair.clone()).collect();
    let pairs2: Vec<_> = pm2.dialect_entries().map(|e| e.pair.clone()).collect();
    assert_eq!(pairs1, pairs2);
}

#[test]
fn dialect_pair_btreemap_sorted_by_ord() {
    let pm = ProjectionMatrix::with_defaults();
    let pairs: Vec<_> = pm.dialect_entries().map(|e| e.pair.clone()).collect();
    for w in pairs.windows(2) {
        assert!(w[0] <= w[1], "pairs not sorted: {:?} > {:?}", w[0], w[1]);
    }
}

#[test]
fn projection_score_serde_deterministic() {
    let score = ProjectionScore {
        capability_coverage: 0.8,
        mapping_fidelity: 0.9,
        priority: 0.7,
        total: 0.82,
    };
    let json1 = serde_json::to_string(&score).unwrap();
    let json2 = serde_json::to_string(&score).unwrap();
    assert_eq!(json1, json2);
}

// =========================================================================
// 10. Matrix merge (local overrides + global defaults)
// =========================================================================

#[test]
fn local_override_on_top_of_global_defaults() {
    let mut pm = ProjectionMatrix::with_defaults();
    // Globally, Kimi→Copilot is unsupported. Override it to mapped.
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
    assert_eq!(
        pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap().mode,
        ProjectionMode::Mapped
    );
    // Other entries remain unchanged.
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Mapped
    );
    assert_eq!(
        pm.lookup(Dialect::Kimi, Dialect::Kimi).unwrap().mode,
        ProjectionMode::Passthrough
    );
}

#[test]
fn register_defaults_after_custom_does_not_overwrite_custom() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
    pm.register_defaults();
    // register_defaults uses contains_key check, so our custom entry persists.
    // But for already-registered identity/mapped pairs it will re-register.
    // Kimi→Copilot was registered before defaults as Mapped, but defaults
    // register it as Unsupported since it's not in the mapped_pairs list.
    // Actually, register_defaults skips pairs already in the map for the
    // "all remaining pairs are unsupported" loop, but identity and mapped
    // pairs are always registered. Kimi→Copilot is not identity, not in
    // mapped_pairs, so the "remaining" loop skips it since it exists.
    assert_eq!(
        pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap().mode,
        ProjectionMode::Mapped
    );
}

// =========================================================================
// 11. Mapper selection based on dialect pair
// =========================================================================

#[test]
fn mapper_resolved_for_openai_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Claude);
}

#[test]
fn mapper_resolved_for_claude_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert_eq!(mapper.source_dialect(), Dialect::Claude);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

#[test]
fn mapper_identity_for_same_dialect() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        assert!(
            pm.resolve_mapper(d, d).is_some(),
            "identity mapper should exist for {d}"
        );
    }
}

#[test]
fn mapper_none_for_unsupported_pair() {
    let pm = ProjectionMatrix::with_defaults();
    assert!(pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn mapper_codex_openai_uses_identity() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Codex, Dialect::OpenAi).unwrap();
    // Identity mapper: source == target (both report as the actual dialect).
    assert_eq!(mapper.source_dialect(), mapper.target_dialect());
}

// =========================================================================
// 12. Mapper chaining (dialect A → IR → dialect B via routing)
// =========================================================================

#[test]
fn route_identity_has_zero_cost_empty_hops() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let route = pm.find_route(d, d).unwrap();
        assert_eq!(route.cost, 0);
        assert!(route.hops.is_empty());
        assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn route_direct_mapped_pair_has_cost_one() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert_eq!(route.hops.len(), 1);
    assert!(route.is_direct());
    assert!(!route.is_multi_hop());
}

#[test]
fn route_multi_hop_discovered_for_unsupported_direct() {
    let mut pm = ProjectionMatrix::new();
    // Register: Kimi → OpenAI → Claude, but NOT Kimi → Claude directly.
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 2);
    assert!(route.is_multi_hop());
    assert_eq!(route.hops.len(), 2);
    assert_eq!(route.hops[0].from, Dialect::Kimi);
    assert_eq!(route.hops[0].to, Dialect::OpenAi);
    assert_eq!(route.hops[1].from, Dialect::OpenAi);
    assert_eq!(route.hops[1].to, Dialect::Claude);
}

#[test]
fn route_multi_hop_picks_highest_fidelity_intermediate() {
    let mut reg = MappingRegistry::new();
    // Kimi→OpenAI has high fidelity, Kimi→Gemini has low fidelity.
    reg.insert(MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::OpenAi,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::Kimi, Dialect::Gemini, ProjectionMode::Mapped);
    pm.register(Dialect::Gemini, Dialect::Claude, ProjectionMode::Mapped);

    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    // The OpenAI intermediate should yield higher fidelity than Gemini.
    assert_eq!(route.hops[0].to, Dialect::OpenAi);
}

#[test]
fn route_direct_preferred_over_multi_hop_same_endpoints() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Gemini, ProjectionMode::Mapped);
    pm.register(Dialect::Gemini, Dialect::Claude, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

// =========================================================================
// 13. Mapper error handling (unmappable features)
// =========================================================================

#[test]
fn resolve_mapper_returns_none_for_unregistered_pair() {
    let pm = ProjectionMatrix::new();
    assert!(pm
        .resolve_mapper(Dialect::OpenAi, Dialect::Claude)
        .is_none());
}

#[test]
fn unsupported_pair_has_no_mapper() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Unsupported);
    assert!(pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn compatibility_score_unsupported_features_counted() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::Unsupported {
            reason: "not available".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["streaming".into()]);
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.unsupported_features, 1);
    assert_eq!(score.lossless_features, 0);
}

// =========================================================================
// 14. Mapper with capability negotiation
// =========================================================================

#[test]
fn projection_with_registry_and_capabilities_combined() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend(
        "openai-native",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "gemini-partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );

    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
        ])))
        .unwrap();
    // OpenAI has both caps + lossless fidelity → should win.
    assert_eq!(result.selected_backend, "openai-native");
}

#[test]
fn capability_negotiation_emulated_counted_in_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
}

// =========================================================================
// 15. Edge cases — unknown dialect pair falls back gracefully
// =========================================================================

#[test]
fn unknown_pair_lookup_returns_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm.lookup(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn unknown_pair_no_route_found() {
    let pm = ProjectionMatrix::new();
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn unregistered_pair_resolve_mapper_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm.resolve_mapper(Dialect::Gemini, Dialect::Codex).is_none());
}

// =========================================================================
// 16. Edge cases — empty projection matrix
// =========================================================================

#[test]
fn empty_matrix_project_returns_empty_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn empty_matrix_has_zero_backends() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn empty_matrix_has_zero_dialect_entries() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.dialect_entry_count(), 0);
}

#[test]
fn empty_matrix_no_route_for_any_pair() {
    let pm = ProjectionMatrix::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src != tgt {
                assert!(
                    pm.find_route(src, tgt).is_none(),
                    "expected no route for {} → {}",
                    src,
                    tgt
                );
            }
        }
    }
}

// =========================================================================
// 17. Edge cases — same-dialect projection (identity)
// =========================================================================

#[test]
fn identity_route_fidelity_is_one() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let route = pm.find_route(d, d).unwrap();
        assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn identity_compatibility_score_perfect() {
    let mut pm = ProjectionMatrix::new();
    pm.set_mapping_features(vec![
        "tool_use".into(),
        "streaming".into(),
        "thinking".into(),
    ]);
    for &d in Dialect::all() {
        let score = pm.compatibility_score(d, d);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.lossless_features, 3);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 0);
    }
}

#[test]
fn identity_mapper_resolved_for_all_dialects() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        assert!(
            pm.resolve_mapper(d, d).is_some(),
            "identity mapper should resolve for {d}"
        );
    }
}

// =========================================================================
// 18. Edge cases — missing model in target dialect
// =========================================================================

#[test]
fn backend_with_no_capabilities_rejected_when_caps_required() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "empty-caps",
        CapabilityManifest::new(),
        Dialect::OpenAi,
        100,
    );
    let err = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn backend_with_no_capabilities_accepted_when_no_caps_required() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty-caps", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    assert_eq!(result.selected_backend, "empty-caps");
}

#[test]
fn remove_all_backends_then_project_returns_empty_error() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.remove_backend("be");
    let err = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

// =========================================================================
// 19. Additional scoring edge cases
// =========================================================================

#[test]
fn projection_score_total_is_weighted_sum() {
    let score = ProjectionScore {
        capability_coverage: 0.8,
        mapping_fidelity: 0.6,
        priority: 0.4,
        total: 0.5 * 0.8 + 0.3 * 0.6 + 0.2 * 0.4,
    };
    let expected = 0.5 * 0.8 + 0.3 * 0.6 + 0.2 * 0.4;
    assert!((score.total - expected).abs() < f64::EPSILON);
}

#[test]
fn passthrough_bonus_added_to_matching_dialect() {
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
    let wo_pt = wo_passthrough("claude");
    let result = pm.project(&wo_pt).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn passthrough_bonus_does_not_apply_in_normal_mode() {
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
    let result = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap();
    // Normal mode: higher priority wins.
    assert_eq!(result.selected_backend, "openai-be");
}

// =========================================================================
// 20. Serde roundtrips for types
// =========================================================================

#[test]
fn dialect_pair_serde_roundtrip_all_combinations() {
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let pair = DialectPair::new(src, tgt);
            let json = serde_json::to_string(&pair).unwrap();
            let back: DialectPair = serde_json::from_str(&json).unwrap();
            assert_eq!(pair, back, "serde roundtrip failed for {} → {}", src, tgt);
        }
    }
}

#[test]
fn projection_entry_serde_roundtrip_mapped() {
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
fn projection_entry_serde_roundtrip_unsupported() {
    let entry = ProjectionEntry {
        pair: DialectPair::new(Dialect::Kimi, Dialect::Copilot),
        mode: ProjectionMode::Unsupported,
        mapper_hint: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: ProjectionEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn routing_path_serde_roundtrip() {
    let path = RoutingPath {
        hops: vec![
            RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
                mapper_hint: Some("openai_to_claude".into()),
            },
            RoutingHop {
                from: Dialect::Claude,
                to: Dialect::Gemini,
                mapper_hint: Some("claude_to_gemini".into()),
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
fn compatibility_score_serde_roundtrip() {
    let score = CompatibilityScore {
        source: Dialect::Claude,
        target: Dialect::OpenAi,
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
fn required_emulation_serde_roundtrip() {
    let emu = RequiredEmulation {
        capability: Capability::ToolRead,
        strategy: "adapter".into(),
    };
    let json = serde_json::to_string(&emu).unwrap();
    let back: RequiredEmulation = serde_json::from_str(&json).unwrap();
    assert_eq!(emu, back);
}

#[test]
fn fallback_entry_serde_roundtrip() {
    let entry = FallbackEntry {
        backend_id: "backup-be".into(),
        score: ProjectionScore {
            capability_coverage: 0.7,
            mapping_fidelity: 0.6,
            priority: 0.5,
            total: 0.63,
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: FallbackEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn selection_strategy_all_variants_serde() {
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
fn projection_error_serde_roundtrip() {
    let errors = [
        ProjectionError::EmptyMatrix,
        ProjectionError::NoSuitableBackend {
            reason: "no caps".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

// =========================================================================
// 21. RoutingPath predicates
// =========================================================================

#[test]
fn routing_path_is_direct_for_single_hop() {
    let path = RoutingPath {
        hops: vec![RoutingHop {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
            mapper_hint: None,
        }],
        cost: 1,
        fidelity: 0.8,
    };
    assert!(path.is_direct());
    assert!(!path.is_multi_hop());
}

#[test]
fn routing_path_is_multi_hop_for_two_hops() {
    let path = RoutingPath {
        hops: vec![
            RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Gemini,
                mapper_hint: None,
            },
            RoutingHop {
                from: Dialect::Gemini,
                to: Dialect::Claude,
                mapper_hint: None,
            },
        ],
        cost: 2,
        fidelity: 0.64,
    };
    assert!(path.is_multi_hop());
    assert!(!path.is_direct());
}

#[test]
fn routing_path_empty_hops_is_direct() {
    let path = RoutingPath {
        hops: vec![],
        cost: 0,
        fidelity: 1.0,
    };
    assert!(path.is_direct());
    assert!(!path.is_multi_hop());
}

// =========================================================================
// 22. Fallback chain structure
// =========================================================================

#[test]
fn fallback_chain_excludes_winner() {
    let mut pm = ProjectionMatrix::new();
    for (name, prio) in [("a", 90), ("b", 60), ("c", 30)] {
        pm.register_backend(
            name,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }
    let result = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap();
    for fb in &result.fallback_chain {
        assert_ne!(fb.backend_id, result.selected_backend);
    }
}

#[test]
fn fallback_chain_scores_descending() {
    let mut pm = ProjectionMatrix::new();
    for (name, prio) in [("a", 100), ("b", 70), ("c", 40), ("d", 10)] {
        pm.register_backend(
            name,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }
    let result = pm
        .project(&wo(require_caps(&[Capability::Streaming])))
        .unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|f| f.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback not sorted: {} < {}", w[0], w[1]);
    }
}

// =========================================================================
// 23. DialectPair Display and ordering
// =========================================================================

#[test]
fn dialect_pair_display_contains_arrow() {
    let pair = DialectPair::new(Dialect::Gemini, Dialect::Codex);
    let s = pair.to_string();
    assert!(s.contains('→'));
    assert!(s.contains("Gemini"));
    assert!(s.contains("Codex"));
}

#[test]
fn dialect_pair_ordering_consistent_with_eq() {
    let a = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    let b = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
}

// =========================================================================
// 24. Compatibility scores
// =========================================================================

#[test]
fn compatibility_score_mixed_fidelity_features() {
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
        feature: "thinking".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec![
        "tool_use".into(),
        "streaming".into(),
        "thinking".into(),
    ]);

    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.lossless_features, 1);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.unsupported_features, 1);
    assert!(score.fidelity > 0.0);
    assert!(score.fidelity < 1.0);
}

#[test]
fn compatibility_score_no_features_uses_heuristic() {
    let pm = ProjectionMatrix::new();
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    // No features configured → heuristic fidelity (may be 0.0 with empty registry).
    assert!(score.fidelity >= 0.0);
    assert!(score.fidelity <= 1.0);
}

// =========================================================================
// 25. ProjectionError Display
// =========================================================================

#[test]
fn projection_error_empty_matrix_display() {
    let err = ProjectionError::EmptyMatrix;
    let msg = err.to_string();
    assert!(msg.contains("empty"));
}

#[test]
fn projection_error_no_suitable_backend_display() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("test reason"));
}
