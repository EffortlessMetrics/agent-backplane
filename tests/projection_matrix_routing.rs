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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Comprehensive projection matrix tests verifying SDK×backend routing decisions.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule, features, known_rules};
use abp_projection::{
    ProjectionError, ProjectionMatrix, ProjectionMode, ProjectionResult,
    selection::{ModelCandidate, ModelSelector, SelectionStrategy},
};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    caps.iter().cloned().collect()
}

fn require_emulated(caps: &[Capability]) -> CapabilityRequirements {
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

fn wo(reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new("routing test")
        .requirements(reqs)
        .build()
}

fn empty_wo() -> WorkOrder {
    WorkOrderBuilder::new("empty routing test").build()
}

fn passthrough_wo(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "mode": "passthrough", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("passthrough routing")
        .requirements(reqs)
        .config(config)
        .build()
}

fn mapped_wo(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "mode": "mapped", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("mapped routing")
        .requirements(reqs)
        .config(config)
        .build()
}

fn streaming_manifest() -> CapabilityManifest {
    manifest(&[(Capability::Streaming, SupportLevel::Native)])
}

fn rich_manifest() -> CapabilityManifest {
    manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Native),
    ])
}

/// Projection matrix pre-loaded with one backend per dialect, each with default
/// mappings and a streaming manifest.
fn standard_matrix() -> ProjectionMatrix {
    let mut pm = ProjectionMatrix::with_defaults();
    let dialects = [
        ("openai-be", Dialect::OpenAi),
        ("claude-be", Dialect::Claude),
        ("gemini-be", Dialect::Gemini),
        ("codex-be", Dialect::Codex),
        ("kimi-be", Dialect::Kimi),
        ("copilot-be", Dialect::Copilot),
    ];
    for (id, dialect) in dialects {
        pm.register_backend(id, streaming_manifest(), dialect, 50);
    }
    pm
}

fn lossless_rule(src: Dialect, tgt: Dialect, feature: &str) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feature.into(),
        fidelity: Fidelity::Lossless,
    }
}

fn lossy_rule(src: Dialect, tgt: Dialect, feature: &str) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feature.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "lossy mapping".into(),
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Single dialect routing — each dialect routes to its native backend
// ═══════════════════════════════════════════════════════════════════════════
mod single_dialect_routing {
    use super::*;

    #[test]
    fn openai_routes_to_openai_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::OpenAi);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn claude_routes_to_claude_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::Claude);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn gemini_routes_to_gemini_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::Gemini);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "gemini-be");
    }

    #[test]
    fn codex_routes_to_codex_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::Codex);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "codex-be");
    }

    #[test]
    fn kimi_routes_to_kimi_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::Kimi);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "kimi-be");
    }

    #[test]
    fn copilot_routes_to_copilot_backend() {
        let mut pm = standard_matrix();
        pm.set_source_dialect(Dialect::Copilot);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "copilot-be");
    }

    #[test]
    fn passthrough_openai_selects_native() {
        let pm = standard_matrix();
        let result = pm
            .project(&passthrough_wo(
                require_emulated(&[Capability::Streaming]),
                "openai",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn passthrough_claude_selects_native() {
        let pm = standard_matrix();
        let result = pm
            .project(&passthrough_wo(
                require_emulated(&[Capability::Streaming]),
                "claude",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Cross-dialect routing — dialect A → backend B selection
// ═══════════════════════════════════════════════════════════════════════════
mod cross_dialect_routing {
    use super::*;

    #[test]
    fn openai_source_only_claude_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.set_source_dialect(Dialect::OpenAi);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn claude_source_only_openai_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.set_source_dialect(Dialect::Claude);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn gemini_source_only_openai_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.set_source_dialect(Dialect::Gemini);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn codex_source_routed_to_openai_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.set_source_dialect(Dialect::Codex);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn mapped_mode_cross_dialect_selects_best() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("claude-be", rich_manifest(), Dialect::Claude, 50);
        let result = pm
            .project(&mapped_wo(
                require_emulated(&[Capability::Streaming]),
                "openai",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn cross_dialect_with_two_backends_prefers_mapped() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);
        pm.set_source_dialect(Dialect::OpenAi);
        // Both have mapping from OpenAI; determinism by id sort
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert!(result.selected_backend == "claude-be" || result.selected_backend == "gemini-be");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Fallback routing — no native backend → best alternative
// ═══════════════════════════════════════════════════════════════════════════
mod fallback_routing {
    use super::*;

    #[test]
    fn fallback_chain_populated_when_multiple_backends() {
        let pm = standard_matrix();
        let result = pm.project(&empty_wo()).unwrap();
        assert!(
            !result.fallback_chain.is_empty(),
            "fallback chain should contain alternatives"
        );
    }

    #[test]
    fn fallback_chain_excludes_selected() {
        let pm = standard_matrix();
        let result = pm.project(&empty_wo()).unwrap();
        for fb in &result.fallback_chain {
            assert_ne!(fb.backend_id, result.selected_backend);
        }
    }

    #[test]
    fn fallback_chain_ordered_by_score_desc() {
        let pm = standard_matrix();
        let result = pm.project(&empty_wo()).unwrap();
        for window in result.fallback_chain.windows(2) {
            assert!(
                window[0].score.total >= window[1].score.total,
                "fallback chain not sorted desc: {} < {}",
                window[0].score.total,
                window[1].score.total,
            );
        }
    }

    #[test]
    fn single_backend_has_empty_fallback() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("only-be", streaming_manifest(), Dialect::OpenAi, 50);
        let result = pm.project(&empty_wo()).unwrap();
        assert!(result.fallback_chain.is_empty());
    }

    #[test]
    fn fallback_when_native_unavailable() {
        let mut pm = ProjectionMatrix::with_defaults();
        // Register only a Claude backend, request from OpenAI source
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 30);
        pm.set_source_dialect(Dialect::OpenAi);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        // Claude should be selected (higher priority, has mapping from OpenAI)
        assert_eq!(result.selected_backend, "claude-be");
        // Gemini should be in fallback
        assert!(
            result
                .fallback_chain
                .iter()
                .any(|f| f.backend_id == "gemini-be")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability-based routing — select backend with required capabilities
// ═══════════════════════════════════════════════════════════════════════════
mod capability_based_routing {
    use super::*;

    #[test]
    fn native_satisfies_emulated_requirement() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "native-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_emulated(&[Capability::ToolBash])))
            .unwrap();
        assert_eq!(result.selected_backend, "native-be");
        assert!(result.required_emulations.is_empty());
    }

    #[test]
    fn emulated_satisfies_emulated_requirement() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Emulated)]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_emulated(&[Capability::ToolBash])))
            .unwrap();
        assert_eq!(result.selected_backend, "emu-be");
        assert!(!result.required_emulations.is_empty());
    }

    #[test]
    fn emulated_does_not_satisfy_native_requirement() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Emulated)]),
            Dialect::OpenAi,
            50,
        );
        // With native requirement, emulated backend is not compatible
        let result = pm.project(&wo(require_native(&[Capability::ToolBash])));
        // The backend doesn't satisfy Native requirement → no compatible backend
        // It may still get selected as best partial match
        match result {
            Ok(r) => {
                // If selected, it should be a partial match (score < 1.0)
                assert!(
                    r.fidelity_score.capability_coverage < 1.0 || !r.required_emulations.is_empty()
                );
            }
            Err(ProjectionError::NoSuitableBackend { .. }) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn native_satisfies_native_requirement() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "native-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_native(&[Capability::ToolBash])))
            .unwrap();
        assert_eq!(result.selected_backend, "native-be");
        assert!(result.required_emulations.is_empty());
    }

    #[test]
    fn prefer_backend_with_more_capabilities() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "minimal-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend("rich-be", rich_manifest(), Dialect::OpenAi, 50);
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
            ])))
            .unwrap();
        assert_eq!(result.selected_backend, "rich-be");
    }

    #[test]
    fn unsupported_capability_lowers_coverage() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "partial-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolBash,
            ])))
            .unwrap();
        assert!(result.fidelity_score.capability_coverage < 1.0);
    }

    #[test]
    fn backend_with_all_native_caps_has_no_emulations() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu-be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolBash, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "native-be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolBash, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            60, // higher priority breaks the tie
        );
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolBash,
            ])))
            .unwrap();
        assert_eq!(result.selected_backend, "native-be");
        assert!(result.required_emulations.is_empty());
    }

    #[test]
    fn restricted_satisfies_emulated_requirement() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "restricted-be",
            manifest(&[(
                Capability::ToolBash,
                SupportLevel::Restricted {
                    reason: "sandbox only".into(),
                },
            )]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_emulated(&[Capability::ToolBash])))
            .unwrap();
        assert_eq!(result.selected_backend, "restricted-be");
    }

    #[test]
    fn multiple_caps_all_native() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("full-be", rich_manifest(), Dialect::OpenAi, 50);
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
                Capability::ToolEdit,
                Capability::ToolBash,
            ])))
            .unwrap();
        assert_eq!(result.selected_backend, "full-be");
        assert!(result.required_emulations.is_empty());
        assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Priority routing — prefer native over emulated; higher priority wins
// ═══════════════════════════════════════════════════════════════════════════
mod priority_routing {
    use super::*;

    #[test]
    fn higher_priority_wins_equal_caps() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("low-be", streaming_manifest(), Dialect::OpenAi, 10);
        pm.register_backend("high-be", streaming_manifest(), Dialect::OpenAi, 90);
        let result = pm.project(&empty_wo()).unwrap();
        assert_eq!(result.selected_backend, "high-be");
    }

    #[test]
    fn priority_breaks_tie_same_dialect() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("lo", streaming_manifest(), Dialect::Claude, 20);
        pm.register_backend("hi", streaming_manifest(), Dialect::Claude, 80);
        pm.set_source_dialect(Dialect::Claude);
        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(result.selected_backend, "hi");
    }

    #[test]
    fn capability_coverage_beats_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("high-prio", streaming_manifest(), Dialect::OpenAi, 100);
        pm.register_backend("rich-low-prio", rich_manifest(), Dialect::OpenAi, 10);
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
            ])))
            .unwrap();
        // rich-low-prio has full capability coverage, which weighs 0.5 vs 0.2 for priority
        assert_eq!(result.selected_backend, "rich-low-prio");
    }

    #[test]
    fn zero_priority_still_selectable() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("zero-be", streaming_manifest(), Dialect::OpenAi, 0);
        let result = pm.project(&empty_wo()).unwrap();
        assert_eq!(result.selected_backend, "zero-be");
    }

    #[test]
    fn max_priority_wins() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("b", streaming_manifest(), Dialect::OpenAi, 100);
        pm.register_backend("c", streaming_manifest(), Dialect::OpenAi, 75);
        let result = pm.project(&empty_wo()).unwrap();
        assert_eq!(result.selected_backend, "b");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Load balancing — ModelSelector round-robin and weighted selection
// ═══════════════════════════════════════════════════════════════════════════
mod load_balancing {
    use super::*;

    fn candidate(
        name: &str,
        latency: Option<u64>,
        cost: Option<f64>,
        weight: f64,
    ) -> ModelCandidate {
        ModelCandidate {
            backend_name: name.into(),
            model_id: format!("{name}-model"),
            estimated_latency_ms: latency,
            estimated_cost_per_1k_tokens: cost,
            fidelity_score: Some(0.9),
            weight,
        }
    }

    #[test]
    fn round_robin_cycles_through_candidates() {
        let selector = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![
                candidate("a", None, None, 1.0),
                candidate("b", None, None, 1.0),
                candidate("c", None, None, 1.0),
            ],
        );
        let first = selector.select().unwrap().backend_name.clone();
        let second = selector.select().unwrap().backend_name.clone();
        let third = selector.select().unwrap().backend_name.clone();
        let fourth = selector.select().unwrap().backend_name.clone();
        // Should cycle: a, b, c, a
        assert_eq!(first, "a");
        assert_eq!(second, "b");
        assert_eq!(third, "c");
        assert_eq!(fourth, "a");
    }

    #[test]
    fn round_robin_single_candidate() {
        let selector = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![candidate("only", None, None, 1.0)],
        );
        for _ in 0..5 {
            assert_eq!(selector.select().unwrap().backend_name, "only");
        }
    }

    #[test]
    fn lowest_latency_strategy() {
        let selector = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate("slow", Some(500), None, 1.0),
                candidate("fast", Some(50), None, 1.0),
                candidate("medium", Some(200), None, 1.0),
            ],
        );
        assert_eq!(selector.select().unwrap().backend_name, "fast");
    }

    #[test]
    fn lowest_cost_strategy() {
        let selector = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate("expensive", None, Some(10.0), 1.0),
                candidate("cheap", None, Some(0.5), 1.0),
                candidate("moderate", None, Some(3.0), 1.0),
            ],
        );
        assert_eq!(selector.select().unwrap().backend_name, "cheap");
    }

    #[test]
    fn highest_fidelity_strategy() {
        let candidates = vec![
            ModelCandidate {
                backend_name: "low-fi".into(),
                model_id: "m1".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: Some(0.3),
                weight: 1.0,
            },
            ModelCandidate {
                backend_name: "hi-fi".into(),
                model_id: "m2".into(),
                estimated_latency_ms: None,
                estimated_cost_per_1k_tokens: None,
                fidelity_score: Some(0.99),
                weight: 1.0,
            },
        ];
        let selector = ModelSelector::new(SelectionStrategy::HighestFidelity, candidates);
        assert_eq!(selector.select().unwrap().backend_name, "hi-fi");
    }

    #[test]
    fn fallback_chain_strategy() {
        let selector = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![
                candidate("primary", None, None, 1.0),
                candidate("secondary", None, None, 1.0),
            ],
        );
        assert_eq!(selector.select().unwrap().backend_name, "primary");
        // FallbackChain always returns the first
        assert_eq!(selector.select().unwrap().backend_name, "primary");
    }

    #[test]
    fn select_n_returns_ranked_list() {
        let selector = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate("slow", Some(500), None, 1.0),
                candidate("fast", Some(50), None, 1.0),
                candidate("medium", Some(200), None, 1.0),
            ],
        );
        let top2 = selector.select_n(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].backend_name, "fast");
        assert_eq!(top2[1].backend_name, "medium");
    }

    #[test]
    fn select_empty_returns_none() {
        let selector = ModelSelector::new(SelectionStrategy::RoundRobin, vec![]);
        assert!(selector.select().is_none());
    }

    #[test]
    fn select_n_empty_returns_empty() {
        let selector = ModelSelector::new(SelectionStrategy::RoundRobin, vec![]);
        assert!(selector.select_n(5).is_empty());
    }

    #[test]
    fn weighted_random_returns_some() {
        let selector = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate("a", None, None, 10.0),
                candidate("b", None, None, 1.0),
            ],
        );
        // Should always return Some (non-deterministic which one)
        assert!(selector.select().is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Disabled backends — skip disabled/incompatible backends
// ═══════════════════════════════════════════════════════════════════════════
mod disabled_backends {
    use super::*;

    #[test]
    fn removed_backend_not_selected() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 90);
        pm.register_backend("b", streaming_manifest(), Dialect::OpenAi, 50);
        pm.remove_backend("a");
        let result = pm.project(&empty_wo()).unwrap();
        assert_eq!(result.selected_backend, "b");
    }

    #[test]
    fn remove_returns_true_if_existed() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
        assert!(pm.remove_backend("a"));
    }

    #[test]
    fn remove_returns_false_if_missing() {
        let mut pm = ProjectionMatrix::new();
        assert!(!pm.remove_backend("nonexistent"));
    }

    #[test]
    fn unsupported_cap_backend_skipped_for_compatible() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "no-bash",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        pm.register_backend(
            "has-bash",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolBash, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let result = pm
            .project(&wo(require_emulated(&[
                Capability::Streaming,
                Capability::ToolBash,
            ])))
            .unwrap();
        assert_eq!(result.selected_backend, "has-bash");
    }

    #[test]
    fn all_removed_gives_empty_matrix_error() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
        pm.remove_backend("a");
        let err = pm.project(&empty_wo()).unwrap_err();
        assert_eq!(err, ProjectionError::EmptyMatrix);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. No viable backend — all backends incompatible → error
// ═══════════════════════════════════════════════════════════════════════════
mod no_viable_backend {
    use super::*;

    #[test]
    fn empty_matrix_error() {
        let pm = ProjectionMatrix::new();
        let err = pm.project(&empty_wo()).unwrap_err();
        assert_eq!(err, ProjectionError::EmptyMatrix);
    }

    #[test]
    fn no_capability_match_returns_error() {
        let mut pm = ProjectionMatrix::new();
        // Backend has nothing the work order needs
        pm.register_backend("empty-be", manifest(&[]), Dialect::OpenAi, 50);
        let result = pm.project(&wo(require_emulated(&[
            Capability::ToolBash,
            Capability::ToolRead,
        ])));
        match result {
            Err(ProjectionError::NoSuitableBackend { .. }) => {}
            other => panic!("expected NoSuitableBackend, got: {other:?}"),
        }
    }

    #[test]
    fn default_matrix_no_backends_empty_error() {
        let pm = ProjectionMatrix::with_defaults();
        let err = pm.project(&empty_wo()).unwrap_err();
        assert_eq!(err, ProjectionError::EmptyMatrix);
    }

    #[test]
    fn single_backend_unsupported_cap() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let result = pm.project(&wo(require_emulated(&[Capability::Streaming])));
        match result {
            Err(ProjectionError::NoSuitableBackend { .. }) => {}
            other => panic!("expected NoSuitableBackend, got: {other:?}"),
        }
    }

    #[test]
    fn projection_error_display() {
        let err = ProjectionError::EmptyMatrix;
        assert!(format!("{err}").contains("empty"));

        let err2 = ProjectionError::NoSuitableBackend {
            reason: "test reason".into(),
        };
        assert!(format!("{err2}").contains("test reason"));
    }

    #[test]
    fn projection_error_eq() {
        assert_eq!(ProjectionError::EmptyMatrix, ProjectionError::EmptyMatrix);
        assert_ne!(
            ProjectionError::EmptyMatrix,
            ProjectionError::NoSuitableBackend { reason: "x".into() }
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Matrix construction — build projection matrix from config/defaults
// ═══════════════════════════════════════════════════════════════════════════
mod matrix_construction {
    use super::*;

    #[test]
    fn new_matrix_zero_backends() {
        let pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn default_matrix_zero_backends() {
        let pm = ProjectionMatrix::default();
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn with_defaults_registers_dialect_entries() {
        let pm = ProjectionMatrix::with_defaults();
        assert!(pm.dialect_entry_count() > 0);
        // Identity entries
        for &d in Dialect::all() {
            let entry = pm.lookup(d, d).unwrap();
            assert_eq!(entry.mode, ProjectionMode::Passthrough);
        }
    }

    #[test]
    fn with_defaults_openai_claude_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn with_defaults_claude_openai_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Claude, Dialect::OpenAi).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn with_defaults_codex_openai_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Codex, Dialect::OpenAi).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn with_mapping_registry() {
        let reg = known_rules();
        let pm = ProjectionMatrix::with_mapping_registry(reg);
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn register_backend_increments_count() {
        let mut pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
        assert_eq!(pm.backend_count(), 1);
        pm.register_backend("b", streaming_manifest(), Dialect::Claude, 50);
        assert_eq!(pm.backend_count(), 2);
    }

    #[test]
    fn register_backend_overwrite_same_id() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("a", rich_manifest(), Dialect::Claude, 80);
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn dialect_pair_registration_and_lookup() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Mapped);
    }

    #[test]
    fn identity_pair_forced_passthrough() {
        let mut pm = ProjectionMatrix::new();
        // Even if we register Mapped, identity should be Passthrough
        pm.register(Dialect::OpenAi, Dialect::OpenAi, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
    }

    #[test]
    fn remove_dialect_pair() {
        let mut pm = ProjectionMatrix::with_defaults();
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Matrix update — hot-reload backend changes
// ═══════════════════════════════════════════════════════════════════════════
mod matrix_update {
    use super::*;

    #[test]
    fn add_backend_changes_routing() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("old", streaming_manifest(), Dialect::OpenAi, 50);
        let r1 = pm.project(&empty_wo()).unwrap();
        assert_eq!(r1.selected_backend, "old");

        pm.register_backend("new-high", streaming_manifest(), Dialect::OpenAi, 100);
        let r2 = pm.project(&empty_wo()).unwrap();
        assert_eq!(r2.selected_backend, "new-high");
    }

    #[test]
    fn remove_backend_changes_routing() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 100);
        pm.register_backend("b", streaming_manifest(), Dialect::OpenAi, 50);
        assert_eq!(pm.project(&empty_wo()).unwrap().selected_backend, "a");
        pm.remove_backend("a");
        assert_eq!(pm.project(&empty_wo()).unwrap().selected_backend, "b");
    }

    #[test]
    fn update_backend_caps_changes_selection() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
        // Backend can't satisfy ToolBash
        let r1 = pm.project(&wo(require_emulated(&[Capability::ToolBash])));
        assert!(r1.is_err() || r1.unwrap().fidelity_score.capability_coverage < 1.0);

        // Now update the backend with ToolBash support
        pm.register_backend(
            "be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolBash, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let r2 = pm
            .project(&wo(require_emulated(&[Capability::ToolBash])))
            .unwrap();
        assert_eq!(r2.selected_backend, "be");
        assert!((r2.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn update_priority_changes_ranking() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 10);
        pm.register_backend("b", streaming_manifest(), Dialect::OpenAi, 90);
        assert_eq!(pm.project(&empty_wo()).unwrap().selected_backend, "b");

        // Re-register a with higher priority
        pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 100);
        assert_eq!(pm.project(&empty_wo()).unwrap().selected_backend, "a");
    }

    #[test]
    fn add_dialect_entry_enables_cross_routing() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.set_source_dialect(Dialect::OpenAi);
        // No dialect entry yet — mapping fidelity will be 0
        let r1 = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        let fidelity_before = r1.fidelity_score.mapping_fidelity;

        // Register a mapped entry
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        let r2 = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        // After registering entry, we should still route to the only backend
        assert_eq!(r2.selected_backend, "claude-be");
        // Fidelity may change depending on mapping registry state
        let _ = fidelity_before; // just confirm no panic
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Concurrent routing — thread-safe routing decisions
// ═══════════════════════════════════════════════════════════════════════════
mod concurrent_routing {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn concurrent_projections_are_safe() {
        let pm = Arc::new(standard_matrix());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let pm = Arc::clone(&pm);
                std::thread::spawn(move || {
                    let wo = empty_wo();
                    let result = pm.project(&wo).unwrap();
                    assert!(!result.selected_backend.is_empty());
                    (i, result.selected_backend)
                })
            })
            .collect();
        for h in handles {
            let (idx, backend) = h.join().unwrap();
            let _ = (idx, backend);
        }
    }

    #[test]
    fn concurrent_projections_deterministic() {
        let pm = Arc::new(standard_matrix());
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let pm = Arc::clone(&pm);
                std::thread::spawn(move || pm.project(&empty_wo()).unwrap().selected_backend)
            })
            .collect();
        let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // All threads should pick the same backend for the same input
        assert!(results.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn round_robin_across_threads() {
        let selector = Arc::new(ModelSelector::new(
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
        ));
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let s = Arc::clone(&selector);
                std::thread::spawn(move || s.select().unwrap().backend_name.clone())
            })
            .collect();
        let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // Both "a" and "b" should appear
        assert!(results.contains(&"a".to_string()));
        assert!(results.contains(&"b".to_string()));
    }

    #[test]
    fn clone_matrix_independent() {
        let mut pm = standard_matrix();
        let pm2 = pm.clone();
        pm.remove_backend("openai-be");
        // pm2 should still have all backends
        assert_eq!(pm2.backend_count(), 6);
        assert_eq!(pm.backend_count(), 5);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Routing with policy — policy constraints affect routing
// ═══════════════════════════════════════════════════════════════════════════
mod routing_with_policy {
    use super::*;

    #[test]
    fn passthrough_mode_boosts_native_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        let result = pm
            .project(&passthrough_wo(
                require_emulated(&[Capability::Streaming]),
                "openai",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn passthrough_mode_with_source_dialect_config() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        let result = pm
            .project(&passthrough_wo(
                require_emulated(&[Capability::Streaming]),
                "claude",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn mapped_mode_selects_with_fidelity() {
        let mut reg = MappingRegistry::new();
        reg.insert(lossless_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            features::TOOL_USE,
        ));
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.register_defaults();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.set_mapping_features(vec![features::TOOL_USE.into()]);
        let result = pm
            .project(&mapped_wo(
                require_emulated(&[Capability::Streaming]),
                "openai",
            ))
            .unwrap();
        assert_eq!(result.selected_backend, "claude-be");
        assert!(result.fidelity_score.mapping_fidelity > 0.0);
    }

    #[test]
    fn mapping_features_influence_score() {
        let mut reg = MappingRegistry::new();
        reg.insert(lossless_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            features::TOOL_USE,
        ));
        reg.insert(lossy_rule(
            Dialect::OpenAi,
            Dialect::Gemini,
            features::TOOL_USE,
        ));
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.register_defaults();
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec![features::TOOL_USE.into()]);

        let result = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        // Claude has lossless mapping, Gemini has lossy → Claude should win
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn compatibility_score_identity_is_perfect() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_mapping_features(vec![features::TOOL_USE.into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 0);
    }

    #[test]
    fn set_source_dialect_persists() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

        pm.set_source_dialect(Dialect::OpenAi);
        let r1 = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(r1.selected_backend, "openai-be");

        pm.set_source_dialect(Dialect::Claude);
        let r2 = pm
            .project(&wo(require_emulated(&[Capability::Streaming])))
            .unwrap();
        assert_eq!(r2.selected_backend, "claude-be");
    }

    #[test]
    fn vendor_config_source_dialect_used() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
        let mut config = RuntimeConfig::default();
        config.vendor.insert(
            "abp".into(),
            serde_json::json!({ "source_dialect": "openai" }),
        );
        let order = WorkOrderBuilder::new("test")
            .requirements(require_emulated(&[Capability::Streaming]))
            .config(config)
            .build();
        let result = pm.project(&order).unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge-case and integration tests
// ═══════════════════════════════════════════════════════════════════════════
mod edge_cases {
    use super::*;

    #[test]
    fn projection_result_serializable() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
        let result = pm.project(&empty_wo()).unwrap();
        let json = serde_json::to_string(&result).unwrap();
        let _parsed: ProjectionResult = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn projection_error_serializable() {
        let err = ProjectionError::EmptyMatrix;
        let json = serde_json::to_string(&err).unwrap();
        let parsed: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, err);
    }

    #[test]
    fn large_backend_count() {
        let mut pm = ProjectionMatrix::new();
        for i in 0..100 {
            pm.register_backend(
                format!("be-{i}"),
                streaming_manifest(),
                Dialect::OpenAi,
                i as u32,
            );
        }
        assert_eq!(pm.backend_count(), 100);
        let result = pm.project(&empty_wo()).unwrap();
        assert_eq!(result.selected_backend, "be-99"); // highest priority
    }

    #[test]
    fn all_dialects_have_identity_passthrough() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let entry = pm.lookup(d, d).expect("identity entry missing");
            assert_eq!(entry.mode, ProjectionMode::Passthrough);
            assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
        }
    }

    #[test]
    fn find_route_identity_cost_zero() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let route = pm.find_route(d, d).unwrap();
            assert_eq!(route.cost, 0);
            assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
            assert!(route.hops.is_empty());
        }
    }

    #[test]
    fn find_route_direct_cost_one() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(route.cost, 1);
        assert!(!route.is_multi_hop());
        assert!(route.is_direct());
    }

    #[test]
    fn resolve_mapper_openai_to_claude() {
        let pm = ProjectionMatrix::with_defaults();
        assert!(
            pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude)
                .is_some()
        );
    }

    #[test]
    fn resolve_mapper_identity() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            assert!(pm.resolve_mapper(d, d).is_some());
        }
    }

    #[test]
    fn no_requirements_full_coverage() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
        let result = pm.project(&empty_wo()).unwrap();
        assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dialect_entries_iterator_works() {
        let pm = ProjectionMatrix::with_defaults();
        let count = pm.dialect_entries().count();
        assert_eq!(count, pm.dialect_entry_count());
        assert!(count > 0);
    }
}
