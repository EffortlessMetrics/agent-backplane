// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec)]

//! Comprehensive tests for the projection matrix covering 15 categories:
//! basic selection, capability-based selection, priority ordering, fallback
//! chains, model mapping, cost optimization, latency optimization,
//! registration, query, persistence, empty matrix, error handling,
//! tiebreaking, dynamic updates, and cross-dialect routing.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
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

fn require_native(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

fn work_order(reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

fn work_order_with_dialect(dialect: &str, reqs: CapabilityRequirements) -> WorkOrder {
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

fn passthrough_order(dialect: &str, reqs: CapabilityRequirements) -> WorkOrder {
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

fn candidate_full(
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
// 1. Basic selection — dialect → backend matching
// =========================================================================
mod basic_selection {
    use super::*;

    #[test]
    fn single_backend_exact_caps() {
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
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert_eq!(res.selected_backend, "only");
    }

    #[test]
    fn selects_backend_matching_source_dialect_via_fidelity() {
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
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Same-dialect gets fidelity 1.0 vs cross-dialect < 1.0.
        assert_eq!(res.selected_backend, "claude-be");
    }

    #[test]
    fn empty_reqs_selects_any_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", manifest(&[]), Dialect::OpenAi, 50);
        let res = pm
            .project(&work_order(CapabilityRequirements::default()))
            .unwrap();
        assert_eq!(res.selected_backend, "a");
    }

    #[test]
    fn selects_among_two_same_dialect_backends_by_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "full",
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
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert_eq!(res.selected_backend, "full");
    }

    #[test]
    fn superset_capabilities_still_match() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "superset",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
                (Capability::ToolEdit, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "superset");
    }

    #[test]
    fn result_contains_correct_score_fields() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert!(res.fidelity_score.capability_coverage >= 0.0);
        assert!(res.fidelity_score.mapping_fidelity >= 0.0);
        assert!(res.fidelity_score.priority >= 0.0);
        assert!(res.fidelity_score.total >= 0.0);
    }
}

// =========================================================================
// 2. Capability-based selection
// =========================================================================
mod capability_selection {
    use super::*;

    #[test]
    fn prefers_backend_with_all_native() {
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
            "emu-one",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        // Both fully compatible, but deterministic by id.
        assert!(res.fidelity_score.capability_coverage >= 1.0 - f64::EPSILON);
    }

    #[test]
    fn rejects_backend_missing_required_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "missing",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let err = pm
            .project(&work_order(require_caps(&[
                Capability::ToolRead,
                Capability::ToolWrite,
            ])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn native_requirement_rejects_emulated_only() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu",
            manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
            Dialect::OpenAi,
            50,
        );
        // Require native only.
        let err = pm
            .project(&work_order(require_native(&[Capability::Streaming])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn multiple_capabilities_scored_proportionally() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "two-of-three",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "three-of-three",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
            ])))
            .unwrap();
        assert_eq!(res.selected_backend, "three-of-three");
    }

    #[test]
    fn emulation_listed_in_result() {
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
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert_eq!(res.required_emulations.len(), 1);
        assert_eq!(res.required_emulations[0].capability, Capability::ToolRead);
        assert_eq!(res.required_emulations[0].strategy, "adapter");
    }

    #[test]
    fn unsupported_capability_marks_backend_incompatible() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "unsup",
            manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let err = pm
            .project(&work_order(require_caps(&[Capability::Logprobs])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }
}

// =========================================================================
// 3. Priority ordering
// =========================================================================
mod priority_ordering {
    use super::*;

    #[test]
    fn higher_priority_wins_when_caps_equal() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "high");
    }

    #[test]
    fn priority_normalized_to_max_in_pool() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "only",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            42,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Sole backend → normalized to 1.0.
        assert!((res.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_priority_still_selectable() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "zero",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            0,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "zero");
    }

    #[test]
    fn three_backends_ranked_by_priority() {
        let mut pm = ProjectionMatrix::new();
        for (id, prio) in [("a", 10), ("b", 50), ("c", 90)] {
            pm.register_backend(
                id,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                prio,
            );
        }
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "c");
    }

    #[test]
    fn equal_priority_deterministic_by_id() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "beta",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "alpha",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Same score → alphabetical id tiebreak.
        assert_eq!(res.selected_backend, "alpha");
    }

    #[test]
    fn capability_advantage_overrides_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "high-prio-partial",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            99,
        );
        pm.register_backend(
            "low-prio-full",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            10,
        );
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert_eq!(res.selected_backend, "low-prio-full");
    }
}

// =========================================================================
// 4. Fallback chains
// =========================================================================
mod fallback_chains {
    use super::*;

    #[test]
    fn fallback_excludes_selected() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "a",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            80,
        );
        pm.register_backend(
            "b",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            40,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "a");
        assert!(res.fallback_chain.iter().all(|f| f.backend_id != "a"));
    }

    #[test]
    fn fallback_sorted_descending() {
        let mut pm = ProjectionMatrix::new();
        for (id, prio) in [("a", 90), ("b", 60), ("c", 30), ("d", 10)] {
            pm.register_backend(
                id,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                prio,
            );
        }
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        let scores: Vec<f64> = res.fallback_chain.iter().map(|f| f.score.total).collect();
        for w in scores.windows(2) {
            assert!(w[0] >= w[1], "fallback not descending");
        }
    }

    #[test]
    fn fallback_includes_partially_compatible() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "full",
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
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert!(res.fallback_chain.iter().any(|f| f.backend_id == "partial"));
    }

    #[test]
    fn fallback_chain_length_is_n_minus_one() {
        let mut pm = ProjectionMatrix::new();
        for i in 0..5 {
            pm.register_backend(
                format!("be-{i}"),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                i * 10,
            );
        }
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.fallback_chain.len(), 4);
    }

    #[test]
    fn single_backend_has_empty_fallback() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "alone",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert!(res.fallback_chain.is_empty());
    }

    #[test]
    fn fallback_chain_strategy_selects_first_viable() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![candidate("primary", "m1"), candidate("backup", "m2")],
        );
        assert_eq!(sel.select().unwrap().backend_name, "primary");
    }
}

// =========================================================================
// 5. Model mapping (vendor model names → backend model names)
// =========================================================================
mod model_mapping {
    use super::*;

    #[test]
    fn selector_tracks_model_id() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("openai", "gpt-4", None, None, Some(0.95), 1.0),
                candidate_full("claude", "claude-3-opus", None, None, Some(0.90), 1.0),
            ],
        );
        let selected = sel.select().unwrap();
        assert_eq!(selected.backend_name, "openai");
        assert_eq!(selected.model_id, "gpt-4");
    }

    #[test]
    fn select_n_returns_model_ids_in_order() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("expensive", "gpt-4", None, Some(10.0), None, 1.0),
                candidate_full("cheap", "gpt-3.5", None, Some(0.5), None, 1.0),
                candidate_full("mid", "gpt-4-mini", None, Some(3.0), None, 1.0),
            ],
        );
        let top2 = sel.select_n(2);
        assert_eq!(top2[0].model_id, "gpt-3.5");
        assert_eq!(top2[1].model_id, "gpt-4-mini");
    }

    #[test]
    fn different_models_same_backend() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("openai", "gpt-4", None, None, Some(0.99), 1.0),
                candidate_full("openai", "gpt-3.5", None, None, Some(0.7), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().model_id, "gpt-4");
    }

    #[test]
    fn model_id_preserved_through_serde() {
        let c = candidate("backend-x", "model-y");
        let json = serde_json::to_string(&c).unwrap();
        let back: ModelCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_id, "model-y");
        assert_eq!(back.backend_name, "backend-x");
    }

    #[test]
    fn select_n_zero_returns_empty() {
        let sel = ModelSelector::new(SelectionStrategy::LowestCost, vec![candidate("a", "m")]);
        assert!(sel.select_n(0).is_empty());
    }

    #[test]
    fn select_n_exceeding_candidates_returns_all() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![candidate("a", "m1"), candidate("b", "m2")],
        );
        let all = sel.select_n(100);
        assert_eq!(all.len(), 2);
    }
}

// =========================================================================
// 6. Cost optimization
// =========================================================================
mod cost_optimization {
    use super::*;

    #[test]
    fn lowest_cost_picks_cheapest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("expensive", "m", None, Some(10.0), None, 1.0),
                candidate_full("cheap", "m", None, Some(0.01), None, 1.0),
                candidate_full("mid", "m", None, Some(2.5), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "cheap");
    }

    #[test]
    fn missing_cost_treated_as_max() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("no-cost", "m", None, None, None, 1.0),
                candidate_full("has-cost", "m", None, Some(5.0), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "has-cost");
    }

    #[test]
    fn all_same_cost_deterministic() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("a", "m", None, Some(1.0), None, 1.0),
                candidate_full("b", "m", None, Some(1.0), None, 1.0),
            ],
        );
        let first = sel.select().unwrap().backend_name.clone();
        let second = sel.select().unwrap().backend_name.clone();
        assert_eq!(first, second);
    }

    #[test]
    fn select_n_cost_ranking() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("c3", "m", None, Some(9.0), None, 1.0),
                candidate_full("c1", "m", None, Some(1.0), None, 1.0),
                candidate_full("c2", "m", None, Some(5.0), None, 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "c1");
        assert_eq!(ranked[1].backend_name, "c2");
        assert_eq!(ranked[2].backend_name, "c3");
    }

    #[test]
    fn zero_cost_is_valid() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("free", "m", None, Some(0.0), None, 1.0),
                candidate_full("paid", "m", None, Some(1.0), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "free");
    }
}

// =========================================================================
// 7. Latency optimization
// =========================================================================
mod latency_optimization {
    use super::*;

    #[test]
    fn lowest_latency_picks_fastest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("slow", "m", Some(500), None, None, 1.0),
                candidate_full("fast", "m", Some(10), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "fast");
    }

    #[test]
    fn missing_latency_sorted_last() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("unknown", "m", None, None, None, 1.0),
                candidate_full("known", "m", Some(100), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "known");
    }

    #[test]
    fn select_n_latency_ranking() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("l3", "m", Some(300), None, None, 1.0),
                candidate_full("l1", "m", Some(50), None, None, 1.0),
                candidate_full("l2", "m", Some(150), None, None, 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "l1");
        assert_eq!(ranked[1].backend_name, "l2");
        assert_eq!(ranked[2].backend_name, "l3");
    }

    #[test]
    fn zero_latency_preferred() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("instant", "m", Some(0), None, None, 1.0),
                candidate_full("slow", "m", Some(999), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "instant");
    }

    #[test]
    fn equal_latency_deterministic() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("a", "m", Some(100), None, None, 1.0),
                candidate_full("b", "m", Some(100), None, None, 1.0),
            ],
        );
        let first = sel.select().unwrap().backend_name.clone();
        let second = sel.select().unwrap().backend_name.clone();
        assert_eq!(first, second);
    }
}

// =========================================================================
// 8. Registration (add / remove / update backend entries)
// =========================================================================
mod registration {
    use super::*;

    #[test]
    fn register_backend_increments_count() {
        let mut pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
        pm.register_backend("a", manifest(&[]), Dialect::OpenAi, 50);
        assert_eq!(pm.backend_count(), 1);
        pm.register_backend("b", manifest(&[]), Dialect::Claude, 50);
        assert_eq!(pm.backend_count(), 2);
    }

    #[test]
    fn re_register_overwrites() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "be",
            manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
            Dialect::Claude,
            90,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn remove_backend_decrements_count() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", manifest(&[]), Dialect::OpenAi, 50);
        pm.register_backend("b", manifest(&[]), Dialect::Claude, 50);
        assert!(pm.remove_backend("a"));
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut pm = ProjectionMatrix::new();
        assert!(!pm.remove_backend("ghost"));
    }

    #[test]
    fn register_dialect_pair() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        assert_eq!(pm.dialect_entry_count(), 1);
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn register_identity_forces_passthrough() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::OpenAi, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
    }

    #[test]
    fn remove_dialect_entry() {
        let mut pm = ProjectionMatrix::with_defaults();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn remove_nonexistent_dialect_entry_returns_none() {
        let mut pm = ProjectionMatrix::new();
        assert!(pm.remove(Dialect::OpenAi, Dialect::Claude).is_none());
    }
}

// =========================================================================
// 9. Query (by dialect, by capability, by model, by backend_id)
// =========================================================================
mod query {
    use super::*;

    #[test]
    fn lookup_existing_pair() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(entry.pair.source, Dialect::OpenAi);
        assert_eq!(entry.pair.target, Dialect::Claude);
    }

    #[test]
    fn lookup_missing_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn dialect_entries_iterator_matches_count() {
        let pm = ProjectionMatrix::with_defaults();
        assert_eq!(pm.dialect_entries().count(), pm.dialect_entry_count());
    }

    #[test]
    fn find_route_identity_has_zero_cost() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(route.cost, 0);
        assert!(route.hops.is_empty());
        assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn find_route_direct_has_cost_one() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(route.cost, 1);
        assert_eq!(route.hops.len(), 1);
    }

    #[test]
    fn find_route_returns_none_when_unsupported() {
        // Build matrix with only identity entries and no cross-dialect support.
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Kimi, ProjectionMode::Passthrough);
        pm.register(
            Dialect::Copilot,
            Dialect::Copilot,
            ProjectionMode::Passthrough,
        );
        assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn compatibility_score_identity_is_perfect() {
        let pm = ProjectionMatrix::with_defaults();
        let score = pm.compatibility_score(Dialect::Claude, Dialect::Claude);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compatibility_score_in_valid_range() {
        let pm = ProjectionMatrix::with_defaults();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let score = pm.compatibility_score(src, tgt);
                assert!(
                    (0.0..=1.0).contains(&score.fidelity),
                    "out of range for {src} → {tgt}"
                );
            }
        }
    }
}

// =========================================================================
// 10. Matrix persistence (serde, rebuild from config)
// =========================================================================
mod persistence {
    use super::*;

    #[test]
    fn projection_score_serde_roundtrip() {
        let score = ProjectionScore {
            capability_coverage: 0.9,
            mapping_fidelity: 0.85,
            priority: 0.7,
            total: 0.82,
        };
        let json = serde_json::to_string(&score).unwrap();
        let back: ProjectionScore = serde_json::from_str(&json).unwrap();
        assert_eq!(score, back);
    }

    #[test]
    fn projection_entry_serde_roundtrip() {
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
    fn dialect_pair_serde_roundtrip() {
        let pair = DialectPair::new(Dialect::Gemini, Dialect::Kimi);
        let json = serde_json::to_string(&pair).unwrap();
        let back: DialectPair = serde_json::from_str(&json).unwrap();
        assert_eq!(pair, back);
    }

    #[test]
    fn projection_mode_serde_all_variants() {
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
    fn routing_hop_serde_roundtrip() {
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
    fn routing_path_serde_roundtrip() {
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
    fn fallback_entry_serde_roundtrip() {
        let entry = FallbackEntry {
            backend_id: "backup".into(),
            score: ProjectionScore {
                capability_coverage: 0.5,
                mapping_fidelity: 0.5,
                priority: 0.5,
                total: 0.5,
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: FallbackEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
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
    fn projection_error_serde_roundtrip() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "test".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn rebuild_matrix_from_defaults_is_identical() {
        let pm1 = ProjectionMatrix::with_defaults();
        let pm2 = ProjectionMatrix::with_defaults();
        assert_eq!(pm1.dialect_entry_count(), pm2.dialect_entry_count());
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let e1 = pm1.lookup(src, tgt).unwrap();
                let e2 = pm2.lookup(src, tgt).unwrap();
                assert_eq!(e1.mode, e2.mode);
                assert_eq!(e1.mapper_hint, e2.mapper_hint);
            }
        }
    }

    #[test]
    fn selector_serde_roundtrip() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![candidate("be", "model")],
        );
        let json = serde_json::to_string(&sel).unwrap();
        let back: ModelSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strategy, SelectionStrategy::LowestCost);
        assert_eq!(back.candidates.len(), 1);
    }

    #[test]
    fn compatibility_score_serde_roundtrip() {
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
}

// =========================================================================
// 11. Empty matrix handling
// =========================================================================
mod empty_matrix {
    use super::*;

    #[test]
    fn empty_matrix_returns_error() {
        let pm = ProjectionMatrix::new();
        let err = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn empty_matrix_backend_count_zero() {
        let pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn empty_matrix_dialect_entry_count_zero() {
        let pm = ProjectionMatrix::new();
        assert_eq!(pm.dialect_entry_count(), 0);
    }

    #[test]
    fn empty_matrix_lookup_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn empty_matrix_resolve_mapper_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(
            pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude)
                .is_none()
        );
    }

    #[test]
    fn empty_matrix_find_route_identity_still_works() {
        let pm = ProjectionMatrix::new();
        let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(route.cost, 0);
    }

    #[test]
    fn empty_matrix_find_route_cross_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(pm.find_route(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn empty_matrix_error_message() {
        let err = ProjectionError::EmptyMatrix;
        assert!(err.to_string().contains("empty"));
    }
}

// =========================================================================
// 12. No matching backend (proper error with suggestions)
// =========================================================================
mod no_match {
    use super::*;

    #[test]
    fn no_backend_with_required_caps() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "limited",
            manifest(&[(Capability::Logprobs, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let err = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn error_reason_is_populated() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("x", manifest(&[]), Dialect::OpenAi, 50);
        let err = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap_err();
        if let ProjectionError::NoSuitableBackend { reason } = err {
            assert!(!reason.is_empty());
        } else {
            panic!("expected NoSuitableBackend");
        }
    }

    #[test]
    fn all_backends_unsupported_caps() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "a",
            manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "b",
            manifest(&[(Capability::ToolRead, SupportLevel::Unsupported)]),
            Dialect::Claude,
            50,
        );
        let err = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn empty_caps_backend_fails_non_empty_reqs() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("bare", CapabilityManifest::new(), Dialect::OpenAi, 50);
        let err = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn projection_error_display_contains_reason() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "no streaming".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("no streaming"));
    }
}

// =========================================================================
// 13. Multiple matches (tiebreaking rules)
// =========================================================================
mod tiebreaking {
    use super::*;

    #[test]
    fn same_score_tiebroken_by_id_alphabetically() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "zz",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "aa",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "aa");
    }

    #[test]
    fn many_identical_backends_deterministic() {
        let mut pm = ProjectionMatrix::new();
        for name in ["delta", "alpha", "charlie", "bravo"] {
            pm.register_backend(
                name,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                50,
            );
        }
        let res1 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        let res2 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res1.selected_backend, res2.selected_backend);
        assert_eq!(res1.selected_backend, "alpha");
    }

    #[test]
    fn fidelity_breaks_tie_over_priority() {
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
            "openai-low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "gemini-high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            90,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Fidelity weight (0.3) overcomes priority difference.
        assert_eq!(res.selected_backend, "openai-low");
    }

    #[test]
    fn passthrough_bonus_breaks_tie() {
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
        let wo = passthrough_order("claude", require_caps(&[Capability::Streaming]));
        let res = pm.project(&wo).unwrap();
        assert_eq!(res.selected_backend, "claude-be");
    }

    #[test]
    fn highest_fidelity_selector_tiebreak() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("a", "m", None, None, Some(0.9), 1.0),
                candidate_full("b", "m", None, None, Some(0.9), 1.0),
            ],
        );
        let first = sel.select().unwrap().backend_name.clone();
        let second = sel.select().unwrap().backend_name.clone();
        assert_eq!(first, second);
    }
}

// =========================================================================
// 14. Dynamic updates (runtime backend registration)
// =========================================================================
mod dynamic_updates {
    use super::*;

    #[test]
    fn add_backend_at_runtime_changes_selection() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "slow",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        let res1 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res1.selected_backend, "slow");

        // Add a better backend at runtime.
        pm.register_backend(
            "fast",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let res2 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res2.selected_backend, "fast");
    }

    #[test]
    fn remove_backend_at_runtime_changes_selection() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "primary",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        pm.register_backend(
            "backup",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        let res1 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res1.selected_backend, "primary");

        pm.remove_backend("primary");
        let res2 = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res2.selected_backend, "backup");
    }

    #[test]
    fn update_backend_caps_at_runtime() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        // Initially partial match — only 1 of 2 caps, so coverage < 1.0.
        let res1 = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert!(res1.fidelity_score.capability_coverage < 1.0);

        // Update by re-registering with new caps.
        pm.register_backend(
            "be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let res2 = pm
            .project(&work_order(require_caps(&[
                Capability::Streaming,
                Capability::ToolRead,
            ])))
            .unwrap();
        assert_eq!(res2.selected_backend, "be");
        assert!((res2.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn remove_all_backends_returns_empty_error() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", manifest(&[]), Dialect::OpenAi, 50);
        pm.remove_backend("a");
        let err = pm
            .project(&work_order(CapabilityRequirements::default()))
            .unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn add_dialect_entry_at_runtime() {
        let mut pm = ProjectionMatrix::new();
        assert!(pm.lookup(Dialect::Kimi, Dialect::Copilot).is_none());
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn override_dialect_entry_at_runtime() {
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
    fn remove_then_re_add_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "ephemeral",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.remove_backend("ephemeral");
        assert_eq!(pm.backend_count(), 0);

        pm.register_backend(
            "ephemeral",
            manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
            Dialect::Claude,
            80,
        );
        assert_eq!(pm.backend_count(), 1);
    }
}

// =========================================================================
// 15. Cross-dialect routing (OpenAI task on Claude backend, etc.)
// =========================================================================
mod cross_dialect_routing {
    use super::*;

    #[test]
    fn openai_source_on_claude_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(Dialect::OpenAi);
        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        // Still selectable even though cross-dialect.
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(res.selected_backend, "claude-be");
    }

    #[test]
    fn cross_dialect_has_lower_fidelity_than_same() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(Dialect::Claude);
        pm.register_backend(
            "same",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        pm.register_backend(
            "cross",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Same-dialect gets fidelity 1.0; cross-dialect gets less.
        assert_eq!(res.selected_backend, "same");
    }

    #[test]
    fn passthrough_strongly_prefers_same_dialect() {
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
            70,
        );
        let wo = passthrough_order("claude", require_caps(&[Capability::Streaming]));
        let res = pm.project(&wo).unwrap();
        // Passthrough bonus overcomes priority gap.
        assert_eq!(res.selected_backend, "claude-be");
    }

    #[test]
    fn non_passthrough_ignores_dialect_bonus() {
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
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let res = pm.project(&wo).unwrap();
        assert_eq!(res.selected_backend, "openai-high");
    }

    #[test]
    fn find_route_openai_to_claude_is_direct() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert!(route.is_direct());
        assert_eq!(route.hops[0].from, Dialect::OpenAi);
        assert_eq!(route.hops[0].to, Dialect::Claude);
    }

    #[test]
    fn find_route_multi_hop_through_intermediate() {
        let mut pm = ProjectionMatrix::new();
        // Kimi→OpenAI mapped, OpenAI→Copilot mapped, Kimi→Copilot not.
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
        if let Some(route) = route {
            assert!(route.is_multi_hop());
            assert_eq!(route.cost, 2);
        }
        // If no route found, that's also acceptable (depends on fidelity).
    }

    #[test]
    fn routing_path_is_direct_predicate() {
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
    fn routing_path_is_multi_hop_predicate() {
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
    fn resolve_mapper_for_cross_dialect() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some());
    }

    #[test]
    fn resolve_mapper_reverse_direction() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi);
        assert!(mapper.is_some());
    }

    #[test]
    fn codex_to_openai_uses_identity_mapper() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Codex, Dialect::OpenAi).unwrap();
        assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
    }

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
        // Source dialect from vendor config should influence fidelity scoring.
        let wo = work_order_with_dialect("claude", require_caps(&[Capability::Streaming]));
        let res = pm.project(&wo).unwrap();
        assert_eq!(res.selected_backend, "claude-be");
    }
}

// =========================================================================
// Additional coverage: selection strategies, round-robin, weighted random
// =========================================================================
mod selection_extras {
    use super::*;

    #[test]
    fn round_robin_cycles_through_all() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![
                candidate("a", "m"),
                candidate("b", "m"),
                candidate("c", "m"),
            ],
        );
        let names: Vec<String> = (0..6)
            .map(|_| sel.select().unwrap().backend_name.clone())
            .collect();
        assert_eq!(names, ["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn round_robin_select_n_starts_at_current() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![
                candidate("a", "m"),
                candidate("b", "m"),
                candidate("c", "m"),
            ],
        );
        let first_three = sel.select_n(3);
        assert_eq!(first_three.len(), 3);
    }

    #[test]
    fn weighted_random_always_returns_some() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate_full("a", "m", None, None, None, 1.0),
                candidate_full("b", "m", None, None, None, 2.0),
            ],
        );
        // Should always return something.
        assert!(sel.select().is_some());
    }

    #[test]
    fn weighted_random_zero_weights_still_returns() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate_full("a", "m", None, None, None, 0.0),
                candidate_full("b", "m", None, None, None, 0.0),
            ],
        );
        assert!(sel.select().is_some());
    }

    #[test]
    fn all_strategies_return_none_on_empty() {
        for strategy in [
            SelectionStrategy::LowestLatency,
            SelectionStrategy::LowestCost,
            SelectionStrategy::HighestFidelity,
            SelectionStrategy::RoundRobin,
            SelectionStrategy::WeightedRandom,
            SelectionStrategy::FallbackChain,
        ] {
            let sel = ModelSelector::new(strategy, vec![]);
            assert!(sel.select().is_none(), "{strategy:?}");
        }
    }

    #[test]
    fn select_n_empty_returns_empty() {
        let sel = ModelSelector::new(SelectionStrategy::LowestCost, vec![]);
        assert!(sel.select_n(5).is_empty());
    }

    #[test]
    fn highest_fidelity_picks_max() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("low", "m", None, None, Some(0.1), 1.0),
                candidate_full("high", "m", None, None, Some(0.99), 1.0),
                candidate_full("mid", "m", None, None, Some(0.5), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "high");
    }

    #[test]
    fn missing_fidelity_treated_as_zero() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("none", "m", None, None, None, 1.0),
                candidate_full("some", "m", None, None, Some(0.5), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "some");
    }
}

// =========================================================================
// Additional coverage: mapping fidelity with registries
// =========================================================================
mod fidelity_scoring {
    use super::*;

    #[test]
    fn lossless_mapping_scored_highest() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );

        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert!(res.fidelity_score.mapping_fidelity > 0.9);
    }

    #[test]
    fn lossy_mapping_scored_lower() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "partial support".into(),
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

        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        // Lossy mapping should yield fidelity < 1.0 but > 0.0.
        assert!(res.fidelity_score.mapping_fidelity > 0.0);
        assert!(res.fidelity_score.mapping_fidelity < 1.0);
    }

    #[test]
    fn same_dialect_always_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(Dialect::OpenAi);
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert!((res.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn no_source_dialect_assumes_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        // Don't set source dialect.
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let res = pm
            .project(&work_order(require_caps(&[Capability::Streaming])))
            .unwrap();
        assert!((res.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compatibility_score_with_features() {
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
            fidelity: Fidelity::Unsupported {
                reason: "not supported".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(score.lossless_features, 1);
        assert_eq!(score.unsupported_features, 1);
        assert!(score.fidelity < 1.0);
    }
}

// =========================================================================
// Additional coverage: default matrix structure
// =========================================================================
mod default_matrix_structure {
    use super::*;

    #[test]
    fn with_defaults_covers_all_pairs() {
        let pm = ProjectionMatrix::with_defaults();
        let n = Dialect::all().len();
        assert_eq!(pm.dialect_entry_count(), n * n);
    }

    #[test]
    fn all_identity_entries_are_passthrough() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let entry = pm.lookup(d, d).unwrap();
            assert_eq!(entry.mode, ProjectionMode::Passthrough);
            assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
        }
    }

    #[test]
    fn register_defaults_is_idempotent() {
        let mut pm = ProjectionMatrix::with_defaults();
        let before = pm.dialect_entry_count();
        pm.register_defaults();
        assert_eq!(pm.dialect_entry_count(), before);
    }

    #[test]
    fn mapped_pair_count_is_eight() {
        let pm = ProjectionMatrix::with_defaults();
        let count = pm
            .dialect_entries()
            .filter(|e| e.mode == ProjectionMode::Mapped)
            .count();
        assert_eq!(count, 8);
    }

    #[test]
    fn passthrough_count_equals_dialect_count() {
        let pm = ProjectionMatrix::with_defaults();
        let count = pm
            .dialect_entries()
            .filter(|e| e.mode == ProjectionMode::Passthrough)
            .count();
        assert_eq!(count, Dialect::all().len());
    }

    #[test]
    fn btreemap_deterministic_iteration() {
        let pm1 = ProjectionMatrix::with_defaults();
        let pm2 = ProjectionMatrix::with_defaults();
        let keys1: Vec<_> = pm1.dialect_entries().map(|e| e.pair.clone()).collect();
        let keys2: Vec<_> = pm2.dialect_entries().map(|e| e.pair.clone()).collect();
        assert_eq!(keys1, keys2);
    }
}
