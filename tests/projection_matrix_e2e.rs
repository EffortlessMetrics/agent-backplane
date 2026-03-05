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
//! End-to-end tests for the projection matrix routing work orders to backends.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule, features, known_rules};
use abp_projection::{
    FallbackEntry, ProjectionError, ProjectionMatrix, ProjectionResult, ProjectionScore,
    RequiredEmulation,
};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn manifest(caps: &[(Capability, SupportLevel)]) -> CapabilityManifest {
    caps.iter().cloned().collect()
}

fn require(caps: &[Capability]) -> CapabilityRequirements {
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

#[allow(dead_code)]
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
    WorkOrderBuilder::new("test task")
        .requirements(reqs)
        .build()
}

fn passthrough_wo(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
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

fn mapped_wo(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "mode": "mapped", "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("mapped task")
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
            warning: "lossy".into(),
        },
    }
}

fn unsupported_rule(src: Dialect, tgt: Dialect, feature: &str) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feature.into(),
        fidelity: Fidelity::Unsupported {
            reason: "not available".into(),
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Matrix matches work order dialect to backend
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_match_openai_work_order_selects_openai_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("openai-be", rich_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", rich_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn dialect_match_claude_source_prefers_claude_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Same-dialect gets 1.0 fidelity vs 0.0 for cross-dialect without mappings
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn dialect_match_gemini_source_routes_to_gemini() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Gemini);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn dialect_match_codex_source_routes_to_codex() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Codex);
    pm.register_backend("codex-be", streaming_manifest(), Dialect::Codex, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "codex-be");
}

#[test]
fn dialect_match_kimi_source_routes_to_kimi() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Kimi);
    pm.register_backend("kimi-be", streaming_manifest(), Dialect::Kimi, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "kimi-be");
}

#[test]
fn dialect_match_copilot_source_routes_to_copilot() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Copilot);
    pm.register_backend("copilot-be", streaming_manifest(), Dialect::Copilot, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "copilot-be");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Matrix returns best backend for capability set
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn best_backend_full_native_coverage_wins() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("full", rich_manifest(), Dialect::OpenAi, 50);
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "full");
}

#[test]
fn best_backend_emulated_coverage_beats_no_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emulated",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "missing",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "emulated");
}

#[test]
fn best_backend_five_cap_requirement_picks_richest() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("rich", rich_manifest(), Dialect::OpenAi, 50);
    pm.register_backend(
        "medium",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::Claude,
        50,
    );
    pm.register_backend("minimal", streaming_manifest(), Dialect::Gemini, 50);

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "rich");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. No match returns error
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_match_empty_matrix_returns_empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn no_match_empty_matrix_error_display() {
    let err = ProjectionError::EmptyMatrix;
    assert!(err.to_string().contains("empty"));
}

#[test]
fn no_match_no_backend_satisfies_required_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "limited",
        manifest(&[(Capability::Logprobs, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let err = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn no_match_all_backends_have_zero_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty-a", CapabilityManifest::new(), Dialect::OpenAi, 50);
    pm.register_backend("empty-b", CapabilityManifest::new(), Dialect::Claude, 50);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn no_match_error_has_reason() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("x", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    if let ProjectionError::NoSuitableBackend { reason } = err {
        assert!(!reason.is_empty());
    } else {
        panic!("expected NoSuitableBackend");
    }
}

#[test]
fn no_match_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Multiple candidates ranked by fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_ranking_lossless_beats_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "streaming"));
    reg.insert(lossy_rule(Dialect::Claude, Dialect::Gemini, "tool_use"));
    reg.insert(unsupported_rule(
        Dialect::Claude,
        Dialect::Gemini,
        "streaming",
    ));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn fidelity_ranking_all_lossless_sorted_by_priority() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));
    reg.insert(lossless_rule(Dialect::Claude, Dialect::Gemini, "tool_use"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);

    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 80);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 40);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn fidelity_ranking_same_dialect_perfect_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn fidelity_ranking_fallback_ordered_by_score() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));
    reg.insert(lossy_rule(Dialect::Claude, Dialect::Gemini, "tool_use"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);

    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude-be has perfect fidelity (same dialect)
    assert_eq!(result.selected_backend, "claude-be");
    // Fallback should be descending by score
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|e| e.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1]);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Exact dialect match preferred over mapped
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn exact_dialect_preferred_over_mapped_with_equal_caps() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);

    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Same dialect always gets 1.0 fidelity, lossless mapping is < 1.0
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn exact_dialect_preferred_even_at_lower_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);

    pm.register_backend("openai-low", streaming_manifest(), Dialect::OpenAi, 30);
    pm.register_backend("claude-high", streaming_manifest(), Dialect::Claude, 80);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // OpenAI gets fidelity 1.0, Claude gets 0.0 (no mappings). Fidelity weight 0.3
    // outweighs priority difference.
    assert_eq!(result.selected_backend, "openai-low");
}

#[test]
fn exact_dialect_same_priority_deterministic() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);

    pm.register_backend("claude-a", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("claude-b", streaming_manifest(), Dialect::Claude, 50);

    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(r1.selected_backend, r2.selected_backend);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Passthrough mode requires exact match
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_selects_same_dialect_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "claude"))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn passthrough_bonus_overrides_priority_gap() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-low", streaming_manifest(), Dialect::Claude, 30);
    pm.register_backend("openai-high", streaming_manifest(), Dialect::OpenAi, 80);

    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "claude"))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-low");
}

#[test]
fn passthrough_with_openai_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "openai"))
        .unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn passthrough_still_requires_capability_satisfaction() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", CapabilityManifest::new(), Dialect::Claude, 50);

    let err = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "claude"))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn passthrough_no_bonus_for_different_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("openai-high", streaming_manifest(), Dialect::OpenAi, 90);
    pm.register_backend("claude-low", streaming_manifest(), Dialect::Claude, 10);

    // Passthrough for gemini — neither backend matches, so no bonus
    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "gemini"))
        .unwrap();
    // OpenAI wins by priority since neither gets a bonus
    assert_eq!(result.selected_backend, "openai-high");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Mapped mode allows cross-dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapped_mode_allows_cross_dialect_selection() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);

    pm.register_backend("openai-be", rich_manifest(), Dialect::OpenAi, 80);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&mapped_wo(
            require(&[
                Capability::Streaming,
                Capability::ToolRead,
                Capability::ToolWrite,
            ]),
            "claude",
        ))
        .unwrap();
    // OpenAI has better caps despite cross-dialect
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn mapped_mode_no_passthrough_bonus() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 30);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 80);

    // mapped_wo sets source_dialect=claude, giving claude-be perfect fidelity.
    // In mapped mode there is no passthrough bonus, but same-dialect fidelity
    // still applies. Verify fidelity (not passthrough bonus) is what decides.
    let result = pm
        .project(&mapped_wo(require(&[Capability::Streaming]), "claude"))
        .unwrap();
    // Claude-be wins via fidelity (same dialect = 1.0) despite lower priority
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn mapped_mode_with_known_rules_selects_best_fidelity() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude-to-Claude is same dialect = 1.0 fidelity
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn mapped_mode_cross_dialect_with_lossy_mapping_still_works() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossy_rule(Dialect::Claude, Dialect::Codex, "tool_use"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);

    pm.register_backend("codex-be", streaming_manifest(), Dialect::Codex, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "codex-be");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Matrix serialization/deserialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn projection_score_serde_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.85,
        mapping_fidelity: 0.70,
        priority: 0.50,
        total: 0.73,
    };
    let json = serde_json::to_string(&score).unwrap();
    let back: ProjectionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(back, score);
}

#[test]
fn projection_result_serde_roundtrip() {
    let result = ProjectionResult {
        selected_backend: "test-be".into(),
        fidelity_score: ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 1.0,
            priority: 1.0,
            total: 1.0,
        },
        required_emulations: vec![RequiredEmulation {
            capability: Capability::ToolRead,
            strategy: "adapter".into(),
        }],
        fallback_chain: vec![FallbackEntry {
            backend_id: "fallback-be".into(),
            score: ProjectionScore {
                capability_coverage: 0.5,
                mapping_fidelity: 0.5,
                priority: 0.5,
                total: 0.5,
            },
        }],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: ProjectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.selected_backend, "test-be");
    assert_eq!(back.required_emulations.len(), 1);
    assert_eq!(back.fallback_chain.len(), 1);
}

#[test]
fn projection_error_empty_matrix_serde_roundtrip() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn projection_error_no_suitable_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "caps missing".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn required_emulation_serde_roundtrip() {
    let emu = RequiredEmulation {
        capability: Capability::ToolBash,
        strategy: "sandbox adapter".into(),
    };
    let json = serde_json::to_string(&emu).unwrap();
    let back: RequiredEmulation = serde_json::from_str(&json).unwrap();
    assert_eq!(back, emu);
}

#[test]
fn fallback_entry_serde_roundtrip() {
    let entry = FallbackEntry {
        backend_id: "fb".into(),
        score: ProjectionScore {
            capability_coverage: 0.9,
            mapping_fidelity: 0.8,
            priority: 0.7,
            total: 0.84,
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: FallbackEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend_id, "fb");
}

#[test]
fn projection_score_json_has_all_fields() {
    let score = ProjectionScore {
        capability_coverage: 0.5,
        mapping_fidelity: 0.6,
        priority: 0.7,
        total: 0.8,
    };
    let val: serde_json::Value = serde_json::to_value(&score).unwrap();
    assert!(val.get("capability_coverage").is_some());
    assert!(val.get("mapping_fidelity").is_some());
    assert!(val.get("priority").is_some());
    assert!(val.get("total").is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Matrix construction from registry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn construction_with_mapping_registry() {
    let reg = known_rules();
    let pm = ProjectionMatrix::with_mapping_registry(reg);
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn construction_with_known_rules_then_backends() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    for (i, &d) in Dialect::all().iter().enumerate() {
        pm.register_backend(format!("be-{i}"), streaming_manifest(), d, 50);
    }
    assert_eq!(pm.backend_count(), Dialect::all().len());

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(!result.selected_backend.is_empty());
}

#[test]
fn construction_from_custom_registry() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "streaming"));
    reg.insert(lossy_rule(Dialect::Claude, Dialect::Gemini, "streaming"));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["streaming".into()]);

    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Lossless OpenAI mapping should win
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn construction_set_source_dialect_after_backends() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    // Set source dialect after registration
    pm.set_source_dialect(Dialect::Claude);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Empty matrix behavior
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_matrix_new() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn empty_matrix_default() {
    let pm = ProjectionMatrix::default();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn empty_matrix_project_returns_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn empty_matrix_project_with_empty_requirements_returns_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(CapabilityRequirements::default()))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn empty_matrix_with_mapping_registry_still_empty() {
    let pm = ProjectionMatrix::with_mapping_registry(known_rules());
    assert_eq!(pm.backend_count(), 0);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Matrix with single entry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn single_entry_exact_match() {
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
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "only");
    assert!(result.fallback_chain.is_empty());
}

#[test]
fn single_entry_partial_match_with_emulation() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "emu");
    assert_eq!(result.required_emulations.len(), 2);
}

#[test]
fn single_entry_no_match() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("x", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn single_entry_empty_requirements_matches() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn single_entry_priority_normalized_to_one() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("only", streaming_manifest(), Dialect::OpenAi, 42);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Single backend: normalized priority is priority/max(priority) = 1.0
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Large matrix with many backends
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn large_matrix_twenty_backends() {
    let mut pm = ProjectionMatrix::new();
    let dialects = Dialect::all();
    for i in 0..20 {
        let dialect = dialects[i % dialects.len()];
        pm.register_backend(
            format!("be-{i:02}"),
            streaming_manifest(),
            dialect,
            (i as u32 + 1) * 5,
        );
    }
    assert_eq!(pm.backend_count(), 20);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(!result.selected_backend.is_empty());
    assert_eq!(result.fallback_chain.len(), 19);
}

#[test]
fn large_matrix_highest_priority_selected() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..10 {
        pm.register_backend(
            format!("be-{i}"),
            streaming_manifest(),
            Dialect::OpenAi,
            (i + 1) * 10,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be-9"); // priority 100
}

#[test]
fn large_matrix_fallback_chain_sorted() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..15 {
        pm.register_backend(
            format!("be-{i:02}"),
            streaming_manifest(),
            Dialect::OpenAi,
            (i as u32 + 1) * 5,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|e| e.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback not sorted: {} < {}", w[0], w[1]);
    }
}

#[test]
fn large_matrix_only_best_caps_win() {
    let mut pm = ProjectionMatrix::new();
    // One backend with rich caps, many with just streaming
    pm.register_backend("rich", rich_manifest(), Dialect::OpenAi, 10);
    for i in 0..10 {
        pm.register_backend(
            format!("basic-{i}"),
            streaming_manifest(),
            Dialect::OpenAi,
            90,
        );
    }
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "rich");
}

#[test]
fn large_matrix_mixed_dialects() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);

    pm.register_backend("claude-main", rich_manifest(), Dialect::Claude, 50);
    for i in 0..8 {
        let dialect = if i % 2 == 0 {
            Dialect::OpenAi
        } else {
            Dialect::Gemini
        };
        pm.register_backend(format!("other-{i}"), rich_manifest(), dialect, 50);
    }

    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // Claude backend has perfect fidelity
    assert_eq!(result.selected_backend, "claude-main");
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Matrix update/rebuild
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn update_register_replaces_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::Claude,
        90,
    );
    assert_eq!(pm.backend_count(), 1);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Emulated version should be in use now
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn update_add_backend_changes_selection() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("basic", streaming_manifest(), Dialect::OpenAi, 50);

    let r1 = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(r1.selected_backend, "basic"); // partial match

    // Add a better backend
    pm.register_backend("rich", rich_manifest(), Dialect::OpenAi, 50);
    let r2 = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(r2.selected_backend, "rich");
}

#[test]
fn update_source_dialect_changes_routing() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    pm.set_source_dialect(Dialect::Claude);
    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(r1.selected_backend, "claude-be");

    pm.set_source_dialect(Dialect::OpenAi);
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(r2.selected_backend, "openai-be");
}

#[test]
fn update_mapping_features_changes_fidelity() {
    let mut reg = MappingRegistry::new();
    reg.insert(lossless_rule(Dialect::Claude, Dialect::OpenAi, "tool_use"));
    reg.insert(unsupported_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "streaming",
    ));

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    // With only tool_use feature: OpenAI has lossless mapping
    pm.set_mapping_features(vec!["tool_use".into()]);
    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude still wins (same dialect = 1.0 fidelity), but OpenAI has decent fidelity
    assert_eq!(r1.selected_backend, "claude-be");

    // With streaming feature too: OpenAI loses fidelity
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(r2.selected_backend, "claude-be");
}

#[test]
fn update_rebuild_matrix_from_scratch() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("old-be", streaming_manifest(), Dialect::OpenAi, 50);

    // "Rebuild" by creating a new matrix and re-registering
    let mut pm2 = ProjectionMatrix::new();
    pm2.register_backend("new-be", rich_manifest(), Dialect::Claude, 80);

    let result = pm2.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "new-be");
    assert_eq!(pm2.backend_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Deterministic selection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_same_input_same_output() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be-a", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("be-b", streaming_manifest(), Dialect::Claude, 50);

    let wo1 = wo(require(&[Capability::Streaming]));
    let wo2 = wo(require(&[Capability::Streaming]));

    let r1 = pm.project(&wo1).unwrap();
    let r2 = pm.project(&wo2).unwrap();
    assert_eq!(r1.selected_backend, r2.selected_backend);
}

#[test]
fn deterministic_tied_scores_use_id_sort() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("alpha", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("beta", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gamma", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Tied scores → alphabetical id sort
    assert_eq!(result.selected_backend, "alpha");
}

#[test]
fn deterministic_fallback_order_stable() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 90), ("b", 60), ("c", 30)] {
        pm.register_backend(id, streaming_manifest(), Dialect::OpenAi, prio);
    }

    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();

    let ids1: Vec<&str> = r1
        .fallback_chain
        .iter()
        .map(|e| e.backend_id.as_str())
        .collect();
    let ids2: Vec<&str> = r2
        .fallback_chain
        .iter()
        .map(|e| e.backend_id.as_str())
        .collect();
    assert_eq!(ids1, ids2);
}

#[test]
fn deterministic_hundred_runs_same_result() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("x", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("y", streaming_manifest(), Dialect::Claude, 50);

    let first = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    for _ in 0..100 {
        let r = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
        assert_eq!(r.selected_backend, first.selected_backend);
    }
}

#[test]
fn deterministic_with_all_dialects_tied() {
    let mut pm = ProjectionMatrix::new();
    for (i, &d) in Dialect::all().iter().enumerate() {
        pm.register_backend(format!("be-{i}"), streaming_manifest(), d, 50);
    }
    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(r1.selected_backend, r2.selected_backend);
    assert_eq!(r1.fallback_chain.len(), r2.fallback_chain.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Matrix integration with runtime (end-to-end scenarios)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_claude_to_openai_translation_scenario() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

    pm.register_backend("openai-be", rich_manifest(), Dialect::OpenAi, 50);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    // OpenAI has full cap coverage; Claude only has streaming
    assert_eq!(result.selected_backend, "openai-be");
    assert!(result.fidelity_score.capability_coverage > 0.9);
}

#[test]
fn e2e_passthrough_claude_scenario() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", rich_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", rich_manifest(), Dialect::OpenAi, 50);

    let result = pm
        .project(&passthrough_wo(
            require(&[Capability::Streaming, Capability::ToolRead]),
            "claude",
        ))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn e2e_multi_backend_capability_negotiation() {
    let mut pm = ProjectionMatrix::new();

    // Tier 1: Full capability
    pm.register_backend(
        "tier1",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ExtendedThinking, SupportLevel::Native),
        ]),
        Dialect::Claude,
        80,
    );

    // Tier 2: Partial with emulation
    pm.register_backend(
        "tier2",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        60,
    );

    // Tier 3: Minimal
    pm.register_backend("tier3", streaming_manifest(), Dialect::Gemini, 40);

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "tier1");
    assert!(result.required_emulations.is_empty());
    assert_eq!(result.fallback_chain.len(), 2);
}

#[test]
fn e2e_emulation_reported_correctly() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Emulated),
            (Capability::ToolBash, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolBash,
        ])))
        .unwrap();
    assert_eq!(result.required_emulations.len(), 2);
    let emu_caps: Vec<&Capability> = result
        .required_emulations
        .iter()
        .map(|e| &e.capability)
        .collect();
    assert!(emu_caps.contains(&&Capability::ToolRead));
    assert!(emu_caps.contains(&&Capability::ToolWrite));
}

#[test]
fn e2e_known_rules_all_dialects_routable() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    for &d in Dialect::all() {
        pm.register_backend(format!("{d:?}"), streaming_manifest(), d, 50);
    }

    for &d in Dialect::all() {
        pm.set_source_dialect(d);
        let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
        assert!(!result.selected_backend.is_empty());
    }
}

#[test]
fn e2e_score_weights_correct() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("only", streaming_manifest(), Dialect::OpenAi, 100);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let s = &result.fidelity_score;
    // cap=1.0, fidelity=1.0 (no source dialect), priority=1.0
    let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
    assert!((s.total - expected).abs() < 1e-10);
}

#[test]
fn e2e_work_order_with_model_hint() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("gpt4", rich_manifest(), Dialect::OpenAi, 80);
    pm.register_backend("claude", rich_manifest(), Dialect::Claude, 60);

    let wo = WorkOrderBuilder::new("code review")
        .model("gpt-4")
        .requirements(require(&[Capability::Streaming]))
        .build();

    // Model hint doesn't affect projection matrix routing (yet), but projection still works
    let result = pm.project(&wo).unwrap();
    assert!(!result.selected_backend.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional edge cases & coverage boosters
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_restricted_capability_treated_as_emulated() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "restricted",
        manifest(&[(
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "sandbox only".into(),
            },
        )]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::ToolBash]))).unwrap();
    assert_eq!(result.selected_backend, "restricted");
    // Restricted counts as emulated → should appear in emulations
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn edge_zero_priority_backend_selectable() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("zero-prio", streaming_manifest(), Dialect::OpenAi, 0);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "zero-prio");
}

#[test]
fn edge_max_priority_normalized() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("high", streaming_manifest(), Dialect::OpenAi, 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_no_source_dialect_assumes_perfect_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    // No source dialect set → fidelity defaults to 1.0
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_all_capability_variants_in_manifest() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "everything",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ToolGlob, SupportLevel::Native),
            (Capability::ToolGrep, SupportLevel::Native),
            (Capability::ToolUse, SupportLevel::Native),
            (Capability::ExtendedThinking, SupportLevel::Native),
            (Capability::ImageInput, SupportLevel::Native),
        ]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::ExtendedThinking,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "everything");
    assert!(result.required_emulations.is_empty());
}

#[test]
fn edge_source_dialect_from_vendor_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    // Source dialect encoded in vendor config
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "source_dialect": "claude" }),
    );
    let wo = WorkOrderBuilder::new("test")
        .requirements(require(&[Capability::Streaming]))
        .config(config)
        .build();

    let result = pm.project(&wo).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn edge_capability_coverage_full() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("full", rich_manifest(), Dialect::OpenAi, 50);
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_duplicate_backend_ids_overwrite() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("dup", streaming_manifest(), Dialect::OpenAi, 10);
    pm.register_backend("dup", rich_manifest(), Dialect::Claude, 90);
    pm.register_backend("dup", streaming_manifest(), Dialect::Gemini, 50);
    assert_eq!(pm.backend_count(), 1); // last write wins
}

#[test]
fn edge_passthrough_empty_requirements() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::Claude, 50);
    let result = pm
        .project(&passthrough_wo(CapabilityRequirements::default(), "claude"))
        .unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn edge_mixed_support_levels_in_same_manifest() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Unsupported),
            (
                Capability::ToolBash,
                SupportLevel::Restricted {
                    reason: "sandbox".into(),
                },
            ),
        ]),
        Dialect::OpenAi,
        50,
    );
    // Require only the supported ones
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolBash,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "mixed");
    // ToolRead and ToolBash (restricted → emulated) should be in emulations
    assert_eq!(result.required_emulations.len(), 2);
}

#[test]
fn edge_projection_result_clone() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let cloned = result.clone();
    assert_eq!(cloned.selected_backend, result.selected_backend);
}

#[test]
fn edge_projection_matrix_clone() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    let pm2 = pm.clone();
    assert_eq!(pm2.backend_count(), 1);
    let result = pm2.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn edge_many_capabilities_superset() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "superset",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ToolGlob, SupportLevel::Native),
            (Capability::ToolGrep, SupportLevel::Native),
            (Capability::ToolUse, SupportLevel::Native),
            (Capability::McpClient, SupportLevel::Native),
            (Capability::McpServer, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    // Require only a small subset
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "superset");
    assert_eq!(result.fidelity_score.capability_coverage, 1.0);
}
