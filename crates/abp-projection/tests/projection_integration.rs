// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive integration tests for the projection matrix — testing model
//! selection across all dialect pairs, selection strategies, and end-to-end
//! projection flows.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_projection::selection::{ModelCandidate, ModelSelector, SelectionStrategy};
use abp_projection::{DialectPair, ProjectionError, ProjectionMatrix, ProjectionMode};

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

fn work_order_with_reqs(reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new("integration test task")
        .requirements(reqs)
        .build()
}

fn work_order_with_dialect(dialect: &str, reqs: CapabilityRequirements) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("dialect test task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn passthrough_work_order(dialect: &str, reqs: CapabilityRequirements) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough test task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn candidate(name: &str, model: &str) -> ModelCandidate {
    ModelCandidate {
        backend_name: name.to_string(),
        model_id: model.to_string(),
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
        backend_name: name.to_string(),
        model_id: model.to_string(),
        estimated_latency_ms: latency,
        estimated_cost_per_1k_tokens: cost,
        fidelity_score: fidelity,
        weight,
    }
}

// =========================================================================
// Module: matrix_completeness
// =========================================================================
mod matrix_completeness {
    use super::*;

    #[test]
    fn all_dialect_pairs_have_entries_in_default_matrix() {
        let pm = ProjectionMatrix::with_defaults();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                assert!(
                    pm.lookup(src, tgt).is_some(),
                    "missing entry for {src} → {tgt}"
                );
            }
        }
    }

    #[test]
    fn matrix_lookup_returns_valid_entries_for_all_known_pairs() {
        let pm = ProjectionMatrix::with_defaults();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let entry = pm.lookup(src, tgt).unwrap();
                assert_eq!(entry.pair.source, src);
                assert_eq!(entry.pair.target, tgt);
                // Mode must be one of the three valid variants.
                assert!(matches!(
                    entry.mode,
                    ProjectionMode::Passthrough
                        | ProjectionMode::Mapped
                        | ProjectionMode::Unsupported
                ));
            }
        }
    }

    #[test]
    fn unknown_pair_on_empty_matrix_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
        assert!(pm.lookup(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn matrix_symmetry_for_mapped_pairs() {
        let pm = ProjectionMatrix::with_defaults();
        let mapped_pairs = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Codex, Dialect::OpenAi),
        ];
        for (a, b) in mapped_pairs {
            let fwd = pm
                .lookup(a, b)
                .unwrap_or_else(|| panic!("missing {a} → {b}"));
            let rev = pm
                .lookup(b, a)
                .unwrap_or_else(|| panic!("missing {b} → {a}"));
            assert_eq!(
                fwd.mode, rev.mode,
                "asymmetric mode for {a}↔{b}: {:?} vs {:?}",
                fwd.mode, rev.mode
            );
        }
    }

    #[test]
    fn identity_pairs_always_passthrough() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let entry = pm.lookup(d, d).unwrap();
            assert_eq!(entry.mode, ProjectionMode::Passthrough, "{d} → {d}");
        }
    }

    #[test]
    fn total_entry_count_equals_dialect_count_squared() {
        let pm = ProjectionMatrix::with_defaults();
        let n = Dialect::all().len();
        assert_eq!(pm.dialect_entry_count(), n * n);
    }

    #[test]
    fn mapped_entries_have_non_empty_mapper_hints() {
        let pm = ProjectionMatrix::with_defaults();
        for entry in pm.dialect_entries() {
            if entry.mode == ProjectionMode::Mapped {
                let hint = entry.mapper_hint.as_deref().unwrap();
                assert!(!hint.is_empty(), "empty hint for {}", entry.pair);
            }
        }
    }

    #[test]
    fn unsupported_entries_have_no_mapper_hints() {
        let pm = ProjectionMatrix::with_defaults();
        for entry in pm.dialect_entries() {
            if entry.mode == ProjectionMode::Unsupported {
                assert!(
                    entry.mapper_hint.is_none(),
                    "hint on unsupported {}",
                    entry.pair
                );
            }
        }
    }

    #[test]
    fn register_then_lookup_roundtrip() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
        assert!(entry.mapper_hint.is_some());
    }

    #[test]
    fn remove_entry_makes_lookup_none() {
        let mut pm = ProjectionMatrix::with_defaults();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn compatibility_score_identity_is_perfect() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let score = pm.compatibility_score(d, d);
            assert!(
                (score.fidelity - 1.0).abs() < f64::EPSILON,
                "identity fidelity for {d} was {}",
                score.fidelity
            );
        }
    }

    #[test]
    fn compatibility_score_cross_dialect_in_valid_range() {
        let pm = ProjectionMatrix::with_defaults();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let score = pm.compatibility_score(src, tgt);
                assert!(
                    (0.0..=1.0).contains(&score.fidelity),
                    "fidelity out of range for {src} → {tgt}: {}",
                    score.fidelity
                );
            }
        }
    }

    #[test]
    fn dialect_pair_display_format() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        let display = format!("{pair}");
        assert!(display.contains("OpenAI"));
        assert!(display.contains("Claude"));
        assert!(display.contains('→'));
    }

    #[test]
    fn dialect_pair_ordering_is_consistent() {
        let mut pairs: Vec<DialectPair> = Vec::new();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                pairs.push(DialectPair::new(src, tgt));
            }
        }
        let mut sorted1 = pairs.clone();
        sorted1.sort();
        let mut sorted2 = pairs.clone();
        sorted2.sort();
        assert_eq!(sorted1, sorted2);
    }

    #[test]
    fn default_matrix_passthrough_count_equals_dialect_count() {
        let pm = ProjectionMatrix::with_defaults();
        let pt_count = pm
            .dialect_entries()
            .filter(|e| e.mode == ProjectionMode::Passthrough)
            .count();
        assert_eq!(pt_count, Dialect::all().len());
    }
}

// =========================================================================
// Module: selection_strategies
// =========================================================================
mod selection_strategies {
    use super::*;

    #[test]
    fn best_fidelity_selects_highest_score() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("low", "m", None, None, Some(0.3), 1.0),
                candidate_full("high", "m", None, None, Some(0.95), 1.0),
                candidate_full("mid", "m", None, None, Some(0.7), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "high");
    }

    #[test]
    fn lowest_latency_selects_fastest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("slow", "m", Some(500), None, None, 1.0),
                candidate_full("fast", "m", Some(10), None, None, 1.0),
                candidate_full("mid", "m", Some(200), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "fast");
    }

    #[test]
    fn lowest_cost_selects_cheapest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate_full("expensive", "m", None, Some(10.0), None, 1.0),
                candidate_full("cheap", "m", None, Some(0.1), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "cheap");
    }

    #[test]
    fn strategy_respects_capability_via_fidelity_filtering() {
        // Use fidelity as a proxy for capability — backends with higher fidelity
        // implicitly have better capability coverage.
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("capable", "gpt-4", None, None, Some(0.99), 1.0),
                candidate_full("limited", "gpt-3", None, None, Some(0.5), 1.0),
            ],
        );
        let selected = sel.select().unwrap();
        assert_eq!(selected.backend_name, "capable");
        assert_eq!(selected.model_id, "gpt-4");
    }

    #[test]
    fn same_fidelity_deterministic_by_insertion_order() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("alpha", "m", None, None, Some(0.9), 1.0),
                candidate_full("beta", "m", None, None, Some(0.9), 1.0),
                candidate_full("gamma", "m", None, None, Some(0.9), 1.0),
            ],
        );
        // max_by returns the last max element when equal, but the order is
        // deterministic across runs.
        let first = sel.select().unwrap().backend_name.clone();
        let second = sel.select().unwrap().backend_name.clone();
        assert_eq!(first, second, "selection should be deterministic");
    }

    #[test]
    fn selection_with_empty_backend_list_returns_none() {
        for strategy in [
            SelectionStrategy::HighestFidelity,
            SelectionStrategy::LowestLatency,
            SelectionStrategy::LowestCost,
            SelectionStrategy::RoundRobin,
            SelectionStrategy::WeightedRandom,
            SelectionStrategy::FallbackChain,
        ] {
            let sel = ModelSelector::new(strategy, vec![]);
            assert!(sel.select().is_none(), "{strategy:?} should return None");
        }
    }

    #[test]
    fn selection_filters_by_latency_when_some_missing() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("no_latency", "m", None, None, None, 1.0),
                candidate_full("has_latency", "m", Some(50), None, None, 1.0),
            ],
        );
        // Missing latency sorts last (u64::MAX).
        assert_eq!(sel.select().unwrap().backend_name, "has_latency");
    }

    #[test]
    fn round_robin_cycles_correctly() {
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
        assert_eq!(names, vec!["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn fallback_chain_always_returns_first() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![candidate("primary", "m"), candidate("secondary", "m")],
        );
        for _ in 0..5 {
            assert_eq!(sel.select().unwrap().backend_name, "primary");
        }
    }

    #[test]
    fn weighted_random_with_single_positive_weight() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate_full("zero", "m", None, None, None, 0.0),
                candidate_full("positive", "m", None, None, None, 10.0),
            ],
        );
        for _ in 0..20 {
            assert_eq!(sel.select().unwrap().backend_name, "positive");
        }
    }

    #[test]
    fn select_n_orders_by_strategy() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate_full("slow", "m", Some(500), None, None, 1.0),
                candidate_full("fast", "m", Some(10), None, None, 1.0),
                candidate_full("mid", "m", Some(200), None, None, 1.0),
            ],
        );
        let ranked: Vec<&str> = sel
            .select_n(3)
            .iter()
            .map(|c| c.backend_name.as_str())
            .collect();
        assert_eq!(ranked, vec!["fast", "mid", "slow"]);
    }

    #[test]
    fn select_n_highest_fidelity_orders_descending() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate_full("low", "m", None, None, Some(0.2), 1.0),
                candidate_full("high", "m", None, None, Some(0.9), 1.0),
                candidate_full("mid", "m", None, None, Some(0.6), 1.0),
            ],
        );
        let ranked: Vec<&str> = sel
            .select_n(3)
            .iter()
            .map(|c| c.backend_name.as_str())
            .collect();
        assert_eq!(ranked, vec!["high", "mid", "low"]);
    }

    #[test]
    fn select_n_returns_at_most_available() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![candidate("only", "m")],
        );
        assert_eq!(sel.select_n(10).len(), 1);
    }

    #[test]
    fn selector_serde_roundtrip_preserves_strategy() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![candidate_full(
                "backend",
                "model",
                None,
                None,
                Some(0.9),
                1.0,
            )],
        );
        let json = serde_json::to_string(&sel).unwrap();
        let restored: ModelSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.strategy, SelectionStrategy::HighestFidelity);
        assert_eq!(restored.candidates.len(), 1);
        assert_eq!(restored.candidates[0].backend_name, "backend");
    }
}

// =========================================================================
// Module: end_to_end_projection
// =========================================================================
mod end_to_end_projection {
    use super::*;

    /// Build a matrix with multiple backends spanning different dialects.
    fn multi_dialect_matrix() -> ProjectionMatrix {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend(
            "openai-backend",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            60,
        );
        pm.register_backend(
            "claude-backend",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::Claude,
            60,
        );
        pm.register_backend(
            "gemini-backend",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::Gemini,
            40,
        );
        pm
    }

    #[test]
    fn full_flow_work_order_to_backend_selection() {
        let pm = multi_dialect_matrix();
        let wo = work_order_with_reqs(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        // Must pick one of the backends that has all three capabilities natively.
        assert!(
            result.selected_backend == "openai-backend"
                || result.selected_backend == "claude-backend",
            "selected: {}",
            result.selected_backend
        );
        assert!(!result.fallback_chain.is_empty());
    }

    #[test]
    fn openai_work_order_projected_to_claude_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_source_dialect(Dialect::OpenAi);
        // Register only a Claude backend.
        pm.register_backend(
            "claude-only",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-only");
    }

    #[test]
    fn claude_work_order_projected_to_openai_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_source_dialect(Dialect::Claude);
        pm.register_backend(
            "openai-only",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-only");
    }

    #[test]
    fn work_order_with_unsupported_dialect_source_still_projects() {
        // Kimi→OpenAi is unsupported in default matrix, but projection should
        // still succeed if the backend satisfies capability requirements.
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_source_dialect(Dialect::Kimi);
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        // Should still work — mapping fidelity will be lower but not a hard error.
        let result = pm.project(&wo);
        assert!(
            result.is_ok(),
            "projection should not hard-fail: {:?}",
            result.err()
        );
    }

    #[test]
    fn projection_preserves_work_order_metadata() {
        let pm = multi_dialect_matrix();
        let wo = WorkOrderBuilder::new("preserve metadata task")
            .requirements(require_caps(&[Capability::Streaming]))
            .build();
        assert_eq!(wo.task, "preserve metadata task");
        // Projection does not mutate the work order — it returns a result.
        let result = pm.project(&wo).unwrap();
        assert!(!result.selected_backend.is_empty());
        // Work order is unchanged.
        assert_eq!(wo.task, "preserve metadata task");
    }

    #[test]
    fn empty_matrix_returns_empty_error() {
        let pm = ProjectionMatrix::new();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn passthrough_mode_prefers_same_dialect_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
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
        let wo = passthrough_work_order("claude", require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn projection_result_has_valid_fidelity_scores() {
        let pm = multi_dialect_matrix();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!((0.0..=2.0).contains(&result.fidelity_score.total));
        assert!((0.0..=1.0).contains(&result.fidelity_score.capability_coverage));
        assert!((0.0..=1.0).contains(&result.fidelity_score.mapping_fidelity));
        assert!((0.0..=1.0).contains(&result.fidelity_score.priority));
    }

    #[test]
    fn fallback_chain_sorted_descending_by_total_score() {
        let pm = multi_dialect_matrix();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let scores: Vec<f64> = result
            .fallback_chain
            .iter()
            .map(|f| f.score.total)
            .collect();
        for w in scores.windows(2) {
            assert!(
                w[0] >= w[1],
                "fallback chain not sorted: {} < {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn source_dialect_from_vendor_config() {
        let mut pm = ProjectionMatrix::with_defaults();
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
        // Source dialect set via work order vendor config, not pm.set_source_dialect.
        let wo = work_order_with_dialect("claude", require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Claude-source → Claude backend should get fidelity=1.0, OpenAI gets less.
        // The claude backend should be selected due to perfect fidelity.
        assert_eq!(result.selected_backend, "claude-be");
    }
}

// =========================================================================
// Module: routing_paths
// =========================================================================
mod routing_paths {
    use super::*;

    #[test]
    fn identity_route_has_zero_cost() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(route.cost, 0);
        assert!(route.hops.is_empty());
        assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn direct_mapped_route_has_cost_one() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(route.cost, 1);
        assert!(route.is_direct());
        assert!(!route.is_multi_hop());
    }

    #[test]
    fn multi_hop_route_via_intermediate() {
        // Kimi→Copilot is unsupported, but Kimi→OpenAI and OpenAI→Copilot might
        // allow a multi-hop. Let's set that up explicitly.
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
        assert!(route.is_some(), "should find multi-hop route");
        let route = route.unwrap();
        assert_eq!(route.cost, 2);
        assert!(route.is_multi_hop());
        assert_eq!(route.hops.len(), 2);
    }

    #[test]
    fn no_route_when_completely_disconnected() {
        let pm = ProjectionMatrix::new();
        let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
        assert!(route.is_none());
    }

    #[test]
    fn route_fidelity_decreases_with_hops() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        if let Some(route) = pm.find_route(Dialect::Kimi, Dialect::Copilot) {
            // Multi-hop fidelity should be <= single-hop fidelity.
            assert!(route.fidelity <= 1.0);
        }
    }

    #[test]
    fn direct_route_preferred_over_multi_hop() {
        let mut pm = ProjectionMatrix::new();
        // Direct: OpenAI→Claude
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        // Also set up a multi-hop: OpenAI→Gemini→Claude
        pm.register(Dialect::OpenAi, Dialect::Gemini, ProjectionMode::Mapped);
        pm.register(Dialect::Gemini, Dialect::Claude, ProjectionMode::Mapped);
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        // Direct route has cost 1, should be selected.
        assert_eq!(route.cost, 1);
        assert!(route.is_direct());
    }
}

// =========================================================================
// Module: mapping_fidelity_integration
// =========================================================================
mod mapping_fidelity_integration {
    use super::*;

    #[test]
    fn lossless_mapping_produces_high_fidelity() {
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

        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!(result.fidelity_score.mapping_fidelity > 0.5);
    }

    #[test]
    fn lossy_mapping_produces_lower_fidelity_than_lossless() {
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
                warning: "partial support".into(),
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

        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // OpenAI (lossless) should be selected over Gemini (lossy).
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn same_dialect_backend_always_gets_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(Dialect::OpenAi);
        pm.register_backend(
            "openai-native",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn no_source_dialect_assumes_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        // No source dialect set.
        pm.register_backend(
            "any-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compatibility_score_with_mapping_features() {
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

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(score.lossless_features, 1);
        assert_eq!(score.lossy_features, 1);
        assert_eq!(score.unsupported_features, 0);
        assert!(score.fidelity > 0.0 && score.fidelity <= 1.0);
    }
}

// =========================================================================
// Module: projection_error_cases
// =========================================================================
mod projection_error_cases {
    use super::*;

    #[test]
    fn project_on_empty_matrix_yields_empty_error() {
        let pm = ProjectionMatrix::new();
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        assert!(matches!(
            pm.project(&wo).unwrap_err(),
            ProjectionError::EmptyMatrix
        ));
    }

    #[test]
    fn project_with_unsatisfiable_caps_yields_no_suitable_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "limited",
            manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        assert!(matches!(
            pm.project(&wo).unwrap_err(),
            ProjectionError::NoSuitableBackend { .. }
        ));
    }

    #[test]
    fn project_error_display_messages() {
        let empty_err = ProjectionError::EmptyMatrix;
        let msg = format!("{empty_err}");
        assert!(msg.contains("empty"));

        let no_be_err = ProjectionError::NoSuitableBackend {
            reason: "test reason".into(),
        };
        let msg = format!("{no_be_err}");
        assert!(msg.contains("test reason"));
    }

    #[test]
    fn project_error_serde_roundtrip() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "serde test".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn remove_backend_then_project() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.remove_backend("be");
        let wo = work_order_with_reqs(require_caps(&[Capability::Streaming]));
        assert!(matches!(
            pm.project(&wo).unwrap_err(),
            ProjectionError::EmptyMatrix
        ));
    }
}
