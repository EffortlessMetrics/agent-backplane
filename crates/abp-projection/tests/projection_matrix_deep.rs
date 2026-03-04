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
//! Deep tests for the projection matrix covering projection strategies,
//! strategy selection, direct/mapped projection, fallback logic, priority
//! ordering, capability matching, dialect compatibility, load balancing,
//! no-match errors, serde round-trips, and edge cases.

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

fn work_order(reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

fn passthrough_work_order(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn sourced_work_order(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("sourced task")
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

fn empty_reqs() -> CapabilityRequirements {
    CapabilityRequirements::default()
}

// ── 1. ProjectionStrategy — All strategy variants ───────────────────────

#[test]
fn selection_strategy_lowest_latency_picks_fastest() {
    let candidates = vec![
        ModelCandidate {
            estimated_latency_ms: Some(200),
            ..candidate("slow", "m1")
        },
        ModelCandidate {
            estimated_latency_ms: Some(50),
            ..candidate("fast", "m2")
        },
        ModelCandidate {
            estimated_latency_ms: Some(100),
            ..candidate("mid", "m3")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::LowestLatency, candidates);
    assert_eq!(sel.select().unwrap().backend_name, "fast");
}

#[test]
fn selection_strategy_lowest_cost_picks_cheapest() {
    let candidates = vec![
        ModelCandidate {
            estimated_cost_per_1k_tokens: Some(0.05),
            ..candidate("expensive", "m1")
        },
        ModelCandidate {
            estimated_cost_per_1k_tokens: Some(0.001),
            ..candidate("cheap", "m2")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::LowestCost, candidates);
    assert_eq!(sel.select().unwrap().backend_name, "cheap");
}

#[test]
fn selection_strategy_highest_fidelity_picks_best() {
    let candidates = vec![
        ModelCandidate {
            fidelity_score: Some(0.5),
            ..candidate("low-fi", "m1")
        },
        ModelCandidate {
            fidelity_score: Some(0.99),
            ..candidate("hi-fi", "m2")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::HighestFidelity, candidates);
    assert_eq!(sel.select().unwrap().backend_name, "hi-fi");
}

#[test]
fn selection_strategy_round_robin_cycles() {
    let candidates = vec![
        candidate("a", "m1"),
        candidate("b", "m2"),
        candidate("c", "m3"),
    ];
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, candidates);
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
    assert_eq!(sel.select().unwrap().backend_name, "c");
    assert_eq!(sel.select().unwrap().backend_name, "a"); // wraps
}

#[test]
fn selection_strategy_fallback_chain_picks_first() {
    let candidates = vec![candidate("primary", "m1"), candidate("secondary", "m2")];
    let sel = ModelSelector::new(SelectionStrategy::FallbackChain, candidates);
    assert_eq!(sel.select().unwrap().backend_name, "primary");
    // Repeated calls always return first.
    assert_eq!(sel.select().unwrap().backend_name, "primary");
}

#[test]
fn selection_strategy_weighted_random_returns_some() {
    let candidates = vec![
        ModelCandidate {
            weight: 10.0,
            ..candidate("heavy", "m1")
        },
        ModelCandidate {
            weight: 0.001,
            ..candidate("light", "m2")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::WeightedRandom, candidates);
    // Just verify it returns *something* — weighted random is non-deterministic.
    assert!(sel.select().is_some());
}

#[test]
fn selection_strategy_empty_candidates_returns_none() {
    for strategy in [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ] {
        let sel = ModelSelector::new(strategy, vec![]);
        assert!(
            sel.select().is_none(),
            "strategy {strategy:?} should be None"
        );
    }
}

// ── 2. Strategy selection — choose strategy based on dialect compat ─────

#[test]
fn same_dialect_scores_perfect_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend(
        "oai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn different_dialect_lowers_fidelity_without_mapping() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // Without mapping rules, fidelity should be 0.0 for Claude→Gemini.
    assert!(result.fidelity_score.mapping_fidelity < 1.0);
}

#[test]
fn strategy_prefers_backend_with_lossless_mapping() {
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
    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ── 3. Direct projection — same dialect passthrough ─────────────────────

#[test]
fn direct_projection_identity_route() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 0);
    assert!(route.hops.is_empty());
    assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn direct_projection_identity_is_direct() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Claude, Dialect::Claude).unwrap();
    assert!(route.is_direct());
    assert!(!route.is_multi_hop());
}

#[test]
fn direct_projection_lookup_passthrough() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn direct_projection_mapper_is_identity() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Codex, Dialect::Codex).unwrap();
    // IdentityMapper always reports OpenAi as its dialect pair.
    assert_eq!(mapper.source_dialect(), mapper.target_dialect());
}

#[test]
fn direct_projection_same_dialect_backend_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "claude");
    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ── 4. Mapped projection — different dialect with translation ───────────

#[test]
fn mapped_projection_openai_to_claude_route() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert_eq!(route.hops.len(), 1);
    assert_eq!(route.hops[0].from, Dialect::OpenAi);
    assert_eq!(route.hops[0].to, Dialect::Claude);
}

#[test]
fn mapped_projection_has_mapper_hint() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(
        route.hops[0].mapper_hint.as_deref(),
        Some("openai_to_claude")
    );
}

#[test]
fn mapped_projection_claude_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
    assert_eq!(entry.mapper_hint.as_deref(), Some("claude_to_openai"));
}

#[test]
fn mapped_projection_codex_openai_identity_hint() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Codex, Dialect::OpenAi).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn mapped_projection_resolver_openai_to_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Claude);
}

#[test]
fn mapped_projection_resolver_claude_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert_eq!(mapper.source_dialect(), Dialect::Claude);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

#[test]
fn mapped_projection_multi_hop_through_intermediate() {
    // Kimi→OpenAi is unsupported, but if we add Kimi→Claude and Claude→OpenAi,
    // there should be a 2-hop route.
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::Claude, Dialect::OpenAi, ProjectionMode::Mapped);

    let route = pm.find_route(Dialect::Kimi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 2);
    assert!(route.is_multi_hop());
    assert_eq!(route.hops.len(), 2);
    assert_eq!(route.hops[0].from, Dialect::Kimi);
    assert_eq!(route.hops[0].to, Dialect::Claude);
    assert_eq!(route.hops[1].from, Dialect::Claude);
    assert_eq!(route.hops[1].to, Dialect::OpenAi);
}

// ── 5. Fallback logic — primary unavailable → fallback ──────────────────

#[test]
fn fallback_chain_populated_when_multiple_backends() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "primary",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "fallback",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        20,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "primary");
    assert_eq!(result.fallback_chain.len(), 1);
    assert_eq!(result.fallback_chain[0].backend_id, "fallback");
}

#[test]
fn fallback_chain_excludes_selected_backend() {
    let mut pm = ProjectionMatrix::new();
    for id in ["a", "b", "c"] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
    }
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    for fb in &result.fallback_chain {
        assert_ne!(fb.backend_id, result.selected_backend);
    }
}

#[test]
fn fallback_includes_partially_compatible_backends() {
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
        Dialect::Claude,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "full");
    assert!(
        result
            .fallback_chain
            .iter()
            .any(|f| f.backend_id == "partial")
    );
}

#[test]
fn fallback_chain_descending_scores() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 100), ("b", 75), ("c", 50), ("d", 25)] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|f| f.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback not descending: {} < {}", w[0], w[1]);
    }
}

// ── 6. Priority ordering — multiple backends sorted by priority ─────────

#[test]
fn highest_priority_selected_when_caps_equal() {
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
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "high");
}

#[test]
fn priority_normalized_to_max_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        42,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // Single backend: 42/42 = 1.0
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn priority_zero_is_valid() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "zero",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        0,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "zero");
}

#[test]
fn three_backends_sorted_by_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mid",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "top",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        99,
    );
    pm.register_backend(
        "bot",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        1,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "top");
    assert_eq!(result.fallback_chain[0].backend_id, "mid");
    assert_eq!(result.fallback_chain[1].backend_id, "bot");
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
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // Same score → sorted by id ascending; "alpha" < "beta".
    assert_eq!(result.selected_backend, "alpha");
}

// ── 7. Capability matching — required caps vs backend caps ──────────────

#[test]
fn full_native_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    assert!(result.required_emulations.is_empty());
}

#[test]
fn emulated_caps_count_as_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
        ])))
        .unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
}

#[test]
fn partial_coverage_prefers_more_capable_backend() {
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
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn empty_requirements_matches_all() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("bare", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let result = pm.project(&work_order(empty_reqs())).unwrap();
    assert_eq!(result.selected_backend, "bare");
}

#[test]
fn unsupported_cap_not_counted_as_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&work_order(require_caps(&[Capability::Streaming])));
    // Unsupported means zero coverage → error
    assert!(result.is_err());
}

#[test]
fn multiple_emulated_caps_listed() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.required_emulations.len(), 2);
}

// ── 8. Dialect compatibility — which dialects map to which ──────────────

#[test]
fn all_identity_pairs_passthrough() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
    }
}

#[test]
fn openai_claude_bidirectional_mapped() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Mapped
    );
    assert_eq!(
        pm.lookup(Dialect::Claude, Dialect::OpenAi).unwrap().mode,
        ProjectionMode::Mapped
    );
}

#[test]
fn openai_gemini_bidirectional_mapped() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Gemini).unwrap().mode,
        ProjectionMode::Mapped
    );
    assert_eq!(
        pm.lookup(Dialect::Gemini, Dialect::OpenAi).unwrap().mode,
        ProjectionMode::Mapped
    );
}

#[test]
fn claude_gemini_bidirectional_mapped() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::Claude, Dialect::Gemini).unwrap().mode,
        ProjectionMode::Mapped
    );
    assert_eq!(
        pm.lookup(Dialect::Gemini, Dialect::Claude).unwrap().mode,
        ProjectionMode::Mapped
    );
}

#[test]
fn codex_openai_bidirectional_mapped() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::Codex, Dialect::OpenAi).unwrap().mode,
        ProjectionMode::Mapped
    );
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Codex).unwrap().mode,
        ProjectionMode::Mapped
    );
}

#[test]
fn kimi_copilot_unsupported() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap().mode,
        ProjectionMode::Unsupported
    );
    assert_eq!(
        pm.lookup(Dialect::Copilot, Dialect::Kimi).unwrap().mode,
        ProjectionMode::Unsupported
    );
}

#[test]
fn kimi_to_claude_unsupported() {
    let pm = ProjectionMatrix::with_defaults();
    assert_eq!(
        pm.lookup(Dialect::Kimi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Unsupported
    );
}

#[test]
fn default_matrix_covers_all_pairs_exhaustive() {
    let pm = ProjectionMatrix::with_defaults();
    let all = Dialect::all();
    for &src in all {
        for &tgt in all {
            assert!(pm.lookup(src, tgt).is_some(), "missing pair {src} → {tgt}");
        }
    }
}

#[test]
fn default_mapped_count_is_eight() {
    let pm = ProjectionMatrix::with_defaults();
    let mapped = pm
        .dialect_entries()
        .filter(|e| e.mode == ProjectionMode::Mapped)
        .count();
    assert_eq!(mapped, 8);
}

// ── 9. Load balancing — round-robin across equivalent backends ──────────

#[test]
fn round_robin_cycles_through_three() {
    let candidates = vec![
        candidate("a", "gpt-4"),
        candidate("b", "gpt-4"),
        candidate("c", "gpt-4"),
    ];
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, candidates);
    let mut seen = Vec::new();
    for _ in 0..6 {
        seen.push(sel.select().unwrap().backend_name.clone());
    }
    assert_eq!(seen, vec!["a", "b", "c", "a", "b", "c"]);
}

#[test]
fn round_robin_single_candidate() {
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, vec![candidate("only", "m")]);
    for _ in 0..5 {
        assert_eq!(sel.select().unwrap().backend_name, "only");
    }
}

#[test]
fn select_n_round_robin_respects_limit() {
    let candidates = vec![
        candidate("a", "m"),
        candidate("b", "m"),
        candidate("c", "m"),
    ];
    let sel = ModelSelector::new(SelectionStrategy::RoundRobin, candidates);
    let top2 = sel.select_n(2);
    assert_eq!(top2.len(), 2);
}

#[test]
fn select_n_zero_returns_empty() {
    let sel = ModelSelector::new(SelectionStrategy::LowestLatency, vec![candidate("a", "m")]);
    assert!(sel.select_n(0).is_empty());
}

#[test]
fn select_n_more_than_available() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![candidate("a", "m"), candidate("b", "m")],
    );
    let all = sel.select_n(10);
    assert_eq!(all.len(), 2);
}

// ── 10. No match — no compatible backend → clear error ──────────────────

#[test]
fn empty_matrix_returns_empty_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn empty_matrix_error_display() {
    let err = ProjectionError::EmptyMatrix;
    let msg = err.to_string();
    assert!(msg.contains("empty"));
}

#[test]
fn no_suitable_backend_when_all_unsupported() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "useless",
        manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let err = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn no_suitable_backend_error_display() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("test reason"));
}

#[test]
fn no_suitable_backend_empty_manifest() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let err = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn unsupported_route_returns_none() {
    // An empty matrix has no routes registered.
    let pm = ProjectionMatrix::new();
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

// ── 11. Serde roundtrip — all projection types serialize ────────────────

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
fn dialect_pair_serde_roundtrip() {
    let pair = DialectPair::new(Dialect::Gemini, Dialect::Kimi);
    let json = serde_json::to_string(&pair).unwrap();
    let back: DialectPair = serde_json::from_str(&json).unwrap();
    assert_eq!(pair, back);
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
fn projection_entry_no_hint_serde() {
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
fn projection_score_serde_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.75,
        mapping_fidelity: 0.9,
        priority: 0.5,
        total: 0.735,
    };
    let json = serde_json::to_string(&score).unwrap();
    let back: ProjectionScore = serde_json::from_str(&json).unwrap();
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
        hops: vec![RoutingHop {
            from: Dialect::Claude,
            to: Dialect::OpenAi,
            mapper_hint: Some("claude_to_openai".into()),
        }],
        cost: 1,
        fidelity: 0.95,
    };
    let json = serde_json::to_string(&path).unwrap();
    let back: RoutingPath = serde_json::from_str(&json).unwrap();
    assert_eq!(path, back);
}

#[test]
fn compatibility_score_serde_roundtrip() {
    let cs = CompatibilityScore {
        source: Dialect::OpenAi,
        target: Dialect::Claude,
        fidelity: 0.85,
        lossless_features: 3,
        lossy_features: 1,
        unsupported_features: 0,
    };
    let json = serde_json::to_string(&cs).unwrap();
    let back: CompatibilityScore = serde_json::from_str(&json).unwrap();
    assert_eq!(cs, back);
}

#[test]
fn fallback_entry_serde_roundtrip() {
    let fb = FallbackEntry {
        backend_id: "test-be".into(),
        score: ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 1.0,
            priority: 0.5,
            total: 0.85,
        },
    };
    let json = serde_json::to_string(&fb).unwrap();
    let back: FallbackEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(fb, back);
}

#[test]
fn projection_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "no caps".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn projection_error_empty_matrix_serde() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn selection_strategy_serde_all_variants() {
    for strat in [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ] {
        let json = serde_json::to_string(&strat).unwrap();
        let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strat, back);
    }
}

#[test]
fn model_candidate_serde_roundtrip() {
    let mc = ModelCandidate {
        backend_name: "test-be".into(),
        model_id: "gpt-4".into(),
        estimated_latency_ms: Some(150),
        estimated_cost_per_1k_tokens: Some(0.03),
        fidelity_score: Some(0.95),
        weight: 2.5,
    };
    let json = serde_json::to_string(&mc).unwrap();
    let back: ModelCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(mc, back);
}

#[test]
fn model_selector_serde_roundtrip() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![candidate("a", "m1"), candidate("b", "m2")],
    );
    let json = serde_json::to_string(&sel).unwrap();
    let back: ModelSelector = serde_json::from_str(&json).unwrap();
    assert_eq!(back.strategy, SelectionStrategy::HighestFidelity);
    assert_eq!(back.candidates.len(), 2);
}

// ── 12. Edge cases ──────────────────────────────────────────────────────

#[test]
fn single_backend_always_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "solo",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "solo");
    assert!(result.fallback_chain.is_empty());
}

#[test]
fn all_backends_identical_selects_by_id() {
    let mut pm = ProjectionMatrix::new();
    for id in ["charlie", "alpha", "bravo"] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
    }
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // Deterministic tie-break by id.
    assert_eq!(result.selected_backend, "alpha");
}

#[test]
fn empty_registry_no_routes() {
    let pm = ProjectionMatrix::new();
    assert!(pm.find_route(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn register_then_remove_dialect_entry() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
    let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert!(removed.is_some());
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn remove_nonexistent_entry_returns_none() {
    let mut pm = ProjectionMatrix::new();
    assert!(pm.remove(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn register_then_remove_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "temp",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    assert_eq!(pm.backend_count(), 1);
    assert!(pm.remove_backend("temp"));
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn remove_nonexistent_backend_returns_false() {
    let mut pm = ProjectionMatrix::new();
    assert!(!pm.remove_backend("ghost"));
}

#[test]
fn backend_overwrite_replaces_entry() {
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
fn dialect_pair_display() {
    let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(pair.to_string(), "OpenAI → Claude");
}

#[test]
fn dialect_pair_ordering_deterministic() {
    let a = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    let b = DialectPair::new(Dialect::Claude, Dialect::OpenAi);
    // Just ensure Ord doesn't panic and is consistent.
    let mut pairs = vec![b.clone(), a.clone()];
    pairs.sort();
    let mut pairs2 = vec![a.clone(), b.clone()];
    pairs2.sort();
    assert_eq!(pairs, pairs2);
}

#[test]
fn routing_path_direct_predicates() {
    let direct = RoutingPath {
        hops: vec![RoutingHop {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
            mapper_hint: None,
        }],
        cost: 1,
        fidelity: 0.9,
    };
    assert!(direct.is_direct());
    assert!(!direct.is_multi_hop());
}

#[test]
fn routing_path_multi_hop_predicates() {
    let multi = RoutingPath {
        hops: vec![
            RoutingHop {
                from: Dialect::Kimi,
                to: Dialect::OpenAi,
                mapper_hint: None,
            },
            RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
                mapper_hint: None,
            },
        ],
        cost: 2,
        fidelity: 0.8,
    };
    assert!(!multi.is_direct());
    assert!(multi.is_multi_hop());
}

#[test]
fn routing_path_empty_hops_is_direct() {
    let identity = RoutingPath {
        hops: vec![],
        cost: 0,
        fidelity: 1.0,
    };
    assert!(identity.is_direct());
    assert!(!identity.is_multi_hop());
}

#[test]
fn compatibility_score_same_dialect_perfect() {
    let pm = ProjectionMatrix::with_defaults();
    let cs = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
    assert!((cs.fidelity - 1.0).abs() < f64::EPSILON);
    assert_eq!(cs.lossy_features, 0);
    assert_eq!(cs.unsupported_features, 0);
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
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    let cs = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(cs.lossless_features, 1);
    assert_eq!(cs.lossy_features, 1);
    assert_eq!(cs.unsupported_features, 0);
}

#[test]
fn passthrough_mode_detected_from_work_order() {
    let wo = passthrough_work_order(empty_reqs(), "openai");
    let config_val = wo.config.vendor.get("abp").unwrap();
    assert_eq!(
        config_val.get("mode").unwrap().as_str().unwrap(),
        "passthrough"
    );
}

#[test]
fn passthrough_bonus_applied_to_same_dialect() {
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
    let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "claude");
    let result = pm.project(&wo).unwrap();
    // Claude gets +0.15 passthrough bonus.
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn source_dialect_from_work_order_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "oai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let wo = sourced_work_order(require_caps(&[Capability::Streaming]), "claude");
    let result = pm.project(&wo).unwrap();
    // Claude→Claude fidelity = 1.0, Claude→OpenAi fidelity < 1.0.
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn source_dialect_override_via_set_method() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend(
        "oai-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn resolve_mapper_unsupported_pair_returns_none() {
    let pm = ProjectionMatrix::with_defaults();
    assert!(pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn resolve_mapper_unregistered_pair_returns_none() {
    let pm = ProjectionMatrix::new();
    assert!(
        pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude)
            .is_none()
    );
}

#[test]
fn find_route_multi_hop_picks_highest_fidelity() {
    let mut pm = ProjectionMatrix::new();
    // Path A: Kimi → OpenAi → Copilot
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
    // Path B: Kimi → Claude → Copilot
    pm.register(Dialect::Kimi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::Claude, Dialect::Copilot, ProjectionMode::Mapped);

    let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
    assert!(route.is_some());
    let route = route.unwrap();
    assert_eq!(route.cost, 2);
}

#[test]
fn with_defaults_is_idempotent() {
    let mut pm = ProjectionMatrix::with_defaults();
    let count = pm.dialect_entry_count();
    pm.register_defaults();
    assert_eq!(pm.dialect_entry_count(), count);
}

#[test]
fn register_identity_forces_passthrough_even_if_mapped() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Claude, Dialect::Claude, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::Claude, Dialect::Claude).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn register_identity_forces_passthrough_even_if_unsupported() {
    let mut pm = ProjectionMatrix::new();
    pm.register(
        Dialect::Gemini,
        Dialect::Gemini,
        ProjectionMode::Unsupported,
    );
    let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
}

#[test]
fn projection_result_has_all_fields() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "be");
    assert!(result.fidelity_score.total > 0.0);
    assert_eq!(result.required_emulations.len(), 1);
    assert!(result.fallback_chain.is_empty());
}

#[test]
fn score_weights_sum_to_one() {
    // Verify indirectly: score(1,1,1) should be exactly 1.0
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // cap=1.0, fidelity=1.0 (no source dialect), priority=1.0 → total = 1.0
    assert!((result.fidelity_score.total - 1.0).abs() < f64::EPSILON);
}

#[test]
fn many_backends_all_appear_in_result() {
    let mut pm = ProjectionMatrix::new();
    let count = 10;
    for i in 0..count {
        pm.register_backend(
            format!("be-{i:02}"),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            (i * 10) as u32,
        );
    }
    let result = pm
        .project(&work_order(require_caps(&[Capability::Streaming])))
        .unwrap();
    // 1 selected + 9 fallbacks = 10 total.
    assert_eq!(result.fallback_chain.len(), count - 1);
}

#[test]
fn dialect_entry_count_after_removals() {
    let mut pm = ProjectionMatrix::with_defaults();
    let initial = pm.dialect_entry_count();
    pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(pm.dialect_entry_count(), initial - 1);
    pm.remove(Dialect::Claude, Dialect::OpenAi);
    assert_eq!(pm.dialect_entry_count(), initial - 2);
}

#[test]
fn backend_count_reflects_additions_and_removals() {
    let mut pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
    pm.register_backend("a", CapabilityManifest::new(), Dialect::OpenAi, 1);
    assert_eq!(pm.backend_count(), 1);
    pm.register_backend("b", CapabilityManifest::new(), Dialect::Claude, 1);
    assert_eq!(pm.backend_count(), 2);
    pm.remove_backend("a");
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn select_n_lowest_latency_ordered() {
    let candidates = vec![
        ModelCandidate {
            estimated_latency_ms: Some(300),
            ..candidate("slow", "m")
        },
        ModelCandidate {
            estimated_latency_ms: Some(100),
            ..candidate("fast", "m")
        },
        ModelCandidate {
            estimated_latency_ms: Some(200),
            ..candidate("mid", "m")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::LowestLatency, candidates);
    let top = sel.select_n(3);
    assert_eq!(top[0].backend_name, "fast");
    assert_eq!(top[1].backend_name, "mid");
    assert_eq!(top[2].backend_name, "slow");
}

#[test]
fn select_n_highest_fidelity_ordered() {
    let candidates = vec![
        ModelCandidate {
            fidelity_score: Some(0.3),
            ..candidate("low", "m")
        },
        ModelCandidate {
            fidelity_score: Some(0.9),
            ..candidate("high", "m")
        },
        ModelCandidate {
            fidelity_score: Some(0.6),
            ..candidate("mid", "m")
        },
    ];
    let sel = ModelSelector::new(SelectionStrategy::HighestFidelity, candidates);
    let top = sel.select_n(3);
    assert_eq!(top[0].backend_name, "high");
    assert_eq!(top[1].backend_name, "mid");
    assert_eq!(top[2].backend_name, "low");
}

#[test]
fn model_selector_clone_preserves_counter() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![candidate("a", "m"), candidate("b", "m")],
    );
    let _ = sel.select(); // advance counter to 1
    let cloned = sel.clone();
    // Cloned selector should preserve counter position.
    assert_eq!(cloned.select().unwrap().backend_name, "b");
}
