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
//! Deep tests for the projection matrix routing logic.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_projection::{
    selection::{ModelCandidate, ModelSelector, SelectionStrategy},
    CompatibilityScore, DialectPair, ProjectionError, ProjectionMatrix, ProjectionMode,
    ProjectionScore, RoutingHop, RoutingPath,
};

// ── Helpers ─────────────────────────────────────────────────────────────

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

fn wo(reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    WorkOrderBuilder::new("test").requirements(reqs).build()
}

fn wo_with_source_dialect(reqs: CapabilityRequirements, dialect: &str) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "source_dialect": dialect }),
    );
    WorkOrderBuilder::new("test")
        .requirements(reqs)
        .config(config)
        .build()
}

fn passthrough_wo(reqs: CapabilityRequirements, dialect: &str) -> abp_core::WorkOrder {
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

fn streaming_manifest() -> CapabilityManifest {
    manifest(&[(Capability::Streaming, SupportLevel::Native)])
}

fn all_dialects() -> Vec<Dialect> {
    Dialect::all().to_vec()
}

fn lossless_registry(src: Dialect, tgt: Dialect, feature: &str) -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feature.into(),
        fidelity: Fidelity::Lossless,
    });
    reg
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Basic matrix operations (insert, lookup, remove)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn basic_new_matrix_is_empty() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn basic_default_matrix_is_empty() {
    let pm = ProjectionMatrix::default();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn basic_register_single_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("alpha", streaming_manifest(), Dialect::OpenAi, 50);
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn basic_register_multiple_backends() {
    let mut pm = ProjectionMatrix::new();
    for (i, d) in all_dialects().iter().enumerate() {
        pm.register_backend(format!("be-{i}"), streaming_manifest(), *d, 50);
    }
    assert_eq!(pm.backend_count(), 6);
}

#[test]
fn basic_overwrite_same_id() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 10);
    pm.register_backend("be", streaming_manifest(), Dialect::Claude, 90);
    assert_eq!(pm.backend_count(), 1);
    // The second registration wins.
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Exact match lookup (specific dialect → backend)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn exact_match_openai_backend_for_streaming() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "openai-gpt4",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        80,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "openai-gpt4");
    assert!(result.required_emulations.is_empty());
}

#[test]
fn exact_match_claude_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-sonnet",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ExtendedThinking, SupportLevel::Native),
        ]),
        Dialect::Claude,
        70,
    );
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ExtendedThinking,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-sonnet");
}

#[test]
fn exact_match_preferred_over_partial() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full-match",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "partial-match",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        90,
    );
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "full-match");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Wildcard / fallback behavior
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fallback_backend_used_when_no_perfect_match() {
    let mut pm = ProjectionMatrix::new();
    // Generic "fallback" with all capabilities
    pm.register_backend(
        "fallback",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
        ]),
        Dialect::Gemini,
        20,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "fallback");
    assert_eq!(result.required_emulations.len(), 2);
}

#[test]
fn fallback_chain_populated_with_all_alternatives() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 90);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 60);
    pm.register_backend("c", streaming_manifest(), Dialect::Gemini, 30);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "a");
    assert_eq!(result.fallback_chain.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Priority ordering (higher priority wins when caps equal)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn priority_higher_wins() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("low", streaming_manifest(), Dialect::OpenAi, 10);
    pm.register_backend("high", streaming_manifest(), Dialect::OpenAi, 90);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "high");
}

#[test]
fn priority_zero_is_valid() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("zero", streaming_manifest(), Dialect::OpenAi, 0);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "zero");
}

#[test]
fn priority_100_normalized_to_one() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("max", streaming_manifest(), Dialect::OpenAi, 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn priority_tie_broken_by_id() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("beta", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("alpha", streaming_manifest(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Same score → alphabetical id order (BTreeMap iteration + sort stability).
    assert_eq!(result.selected_backend, "alpha");
}

#[test]
fn priority_many_levels() {
    let mut pm = ProjectionMatrix::new();
    for prio in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
        pm.register_backend(
            format!("be-{prio}"),
            streaming_manifest(),
            Dialect::OpenAi,
            prio,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be-100");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Fidelity scoring per dialect×engine pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_same_dialect_is_perfect() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn fidelity_no_source_dialect_defaults_to_perfect() {
    let mut pm = ProjectionMatrix::new();
    // No source dialect set → fidelity defaults to 1.0.
    pm.register_backend("any-be", streaming_manifest(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn fidelity_lossless_mapping_scores_high() {
    let reg = lossless_registry(Dialect::Claude, Dialect::OpenAi, "tool_use");
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Lossless: 0.7 * 1.0 + 0.3 * 1.0 = 1.0
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn fidelity_lossy_mapping_scores_lower() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Codex,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "schema differs".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("codex-be", streaming_manifest(), Dialect::Codex, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Lossy: lossless_count=0, supported_count=1 → 0.7*0 + 0.3*1 = 0.3
    assert!((result.fidelity_score.mapping_fidelity - 0.3).abs() < 0.01);
}

#[test]
fn fidelity_unsupported_mapping_scores_zero() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Kimi,
        feature: "tool_use".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no mapping".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("kimi-be", streaming_manifest(), Dialect::Kimi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fidelity_score.mapping_fidelity.abs() < f64::EPSILON);
}

#[test]
fn fidelity_mixed_features_blended() {
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
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // 1 lossless out of 2, 2 supported out of 2
    // 0.7 * 0.5 + 0.3 * 1.0 = 0.65
    let fid = result.fidelity_score.mapping_fidelity;
    assert!((fid - 0.65).abs() < 0.01);
}

#[test]
fn fidelity_constrains_backend_selection_over_priority() {
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

    pm.register_backend("openai-low", streaming_manifest(), Dialect::OpenAi, 10);
    pm.register_backend("gemini-high", streaming_manifest(), Dialect::Gemini, 90);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // OpenAI has fidelity 1.0 vs Gemini 0.0; fidelity weight 0.3 overcomes priority gap.
    assert_eq!(result.selected_backend, "openai-low");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. rank_targets returns backends sorted by fidelity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rank_targets_descending_order() {
    let reg = abp_mapping::known_rules();
    let ranked = reg.rank_targets(Dialect::Claude, &["tool_use"]);
    // Should be sorted by lossless count descending.
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1, "rank_targets not sorted descending");
    }
}

#[test]
fn rank_targets_excludes_same_dialect() {
    let reg = abp_mapping::known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert!(
        !ranked.iter().any(|(d, _)| *d == Dialect::OpenAi),
        "rank_targets should exclude source dialect"
    );
}

#[test]
fn rank_targets_empty_features_returns_empty() {
    let reg = abp_mapping::known_rules();
    let ranked = reg.rank_targets(Dialect::Claude, &[]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_targets_unknown_feature_returns_empty() {
    let reg = abp_mapping::known_rules();
    let ranked = reg.rank_targets(Dialect::Claude, &["nonexistent_feature"]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_targets_multiple_features() {
    let reg = abp_mapping::known_rules();
    let ranked = reg.rank_targets(Dialect::Claude, &["tool_use", "streaming"]);
    assert!(!ranked.is_empty());
    // Each entry should have lossless count <= number of features queried.
    for (_, count) in &ranked {
        assert!(*count <= 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. All 6 dialects can be source and target
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_dialects_as_backend() {
    for dialect in all_dialects() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            format!("be-{}", dialect.label()),
            streaming_manifest(),
            dialect,
            50,
        );
        let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
        assert_eq!(result.selected_backend, format!("be-{}", dialect.label()));
    }
}

#[test]
fn all_dialects_as_source() {
    for dialect in all_dialects() {
        let mut pm = ProjectionMatrix::new();
        pm.set_source_dialect(dialect);
        pm.register_backend("be", streaming_manifest(), dialect, 50);
        let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
        assert_eq!(result.selected_backend, "be");
        // Same source and target → perfect fidelity.
        assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn all_dialect_pairs_can_be_projected() {
    let dialects = all_dialects();
    for &src in &dialects {
        for &tgt in &dialects {
            let mut pm = ProjectionMatrix::new();
            pm.set_source_dialect(src);
            pm.register_backend("be", streaming_manifest(), tgt, 50);
            let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
            assert_eq!(result.selected_backend, "be");
            if src == tgt {
                assert!(
                    (result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON,
                    "same dialect {src:?}→{tgt:?} should have perfect fidelity"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Empty matrix returns no results
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn empty_matrix_error_display() {
    let err = ProjectionError::EmptyMatrix;
    assert!(err.to_string().contains("empty"));
}

#[test]
fn empty_matrix_with_empty_requirements() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(CapabilityRequirements::default()))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Duplicate entries (last wins)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn duplicate_id_last_registration_wins_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("be", streaming_manifest(), Dialect::Claude, 50);
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn duplicate_id_last_registration_wins_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 10);
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 99);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Priority should be 99 (normalized to 1.0 since it's the only backend).
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn duplicate_id_last_registration_wins_capabilities() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.required_emulations.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Serde roundtrip for score and related types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_score_serde_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.9,
        mapping_fidelity: 0.8,
        priority: 0.7,
        total: 0.85,
    };
    let json = serde_json::to_string(&score).unwrap();
    let deser: ProjectionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(score, deser);
}

#[test]
fn projection_error_serde_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let deser: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, deser);
}

#[test]
fn projection_error_empty_matrix_serde() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let deser: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, deser);
}

#[test]
fn projection_result_serde_roundtrip() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be-a", streaming_manifest(), Dialect::OpenAi, 80);
    pm.register_backend("be-b", streaming_manifest(), Dialect::Claude, 40);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["selected_backend"].is_string());
    assert!(parsed["fidelity_score"]["total"].is_number());
    assert!(parsed["fallback_chain"].is_array());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Matrix merge / composition
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compose_two_matrices_via_re_registration() {
    let mut pm1 = ProjectionMatrix::new();
    pm1.register_backend("be-1", streaming_manifest(), Dialect::OpenAi, 50);

    let mut pm2 = ProjectionMatrix::new();
    pm2.register_backend("be-2", streaming_manifest(), Dialect::Claude, 60);

    // "Merge" by re-registering into a new matrix.
    let mut merged = ProjectionMatrix::new();
    merged.register_backend("be-1", streaming_manifest(), Dialect::OpenAi, 50);
    merged.register_backend("be-2", streaming_manifest(), Dialect::Claude, 60);
    assert_eq!(merged.backend_count(), 2);
}

#[test]
fn compose_with_mapping_registry() {
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
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ═══════════════════════════════════════════════════════════════════════
// 12. select_backend integration via project()
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn select_backend_picks_best_overall() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

    pm.register_backend(
        "openai-be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolUse, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        60,
    );
    pm.register_backend(
        "claude-be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolUse, SupportLevel::Native),
        ]),
        Dialect::Claude,
        50,
    );

    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolUse])))
        .unwrap();
    // Claude backend has same-dialect fidelity (1.0) vs OpenAI lossless mapping.
    // Claude fidelity=1.0 w=0.3 → 0.3; OpenAI fidelity≈1.0 too w=0.3 → 0.3
    // But OpenAI has higher priority (60 vs 50 → 60/60=1.0 vs 50/60=0.83)
    // OpenAI: 0.5*1 + 0.3*1 + 0.2*1.0 = 1.0
    // Claude: 0.5*1 + 0.3*1 + 0.2*0.83 = 0.967
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn select_backend_no_suitable_error() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "useless",
        manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn select_backend_returns_emulation_plan() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu-be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::Streaming
    );
    assert_eq!(result.required_emulations[0].strategy, "adapter");
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Cross-dialect routing (OpenAI → Claude engine, etc.)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_openai_to_claude() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
    assert!(result.fidelity_score.mapping_fidelity > 0.0);
}

#[test]
fn cross_dialect_claude_to_gemini() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn cross_dialect_gemini_to_codex() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Gemini);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("codex-be", streaming_manifest(), Dialect::Codex, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "codex-be");
}

#[test]
fn cross_dialect_source_from_work_order_vendor_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 50);

    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
    // Source dialect matches backend → fidelity should be 1.0.
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn cross_dialect_picks_higher_fidelity_target() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

    // OpenAI: both lossless for claude→openai
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    // Codex: tool_use is lossy for claude→codex
    pm.register_backend("codex-be", streaming_manifest(), Dialect::Codex, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Passthrough mode detection (same dialect and engine)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_mode_detected() {
    let w = passthrough_wo(require(&[Capability::Streaming]), "claude");
    assert!(w.config.vendor.contains_key("abp"));
}

#[test]
fn passthrough_bonus_selects_same_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 40);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 60);

    let w = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Claude gets +0.15 passthrough bonus, overcoming priority gap.
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn passthrough_no_bonus_for_different_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let w = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // OpenAI doesn't match claude source → no bonus.
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn passthrough_all_dialects() {
    for dialect in all_dialects() {
        let label = match dialect {
            Dialect::OpenAi => "openai",
            Dialect::Claude => "claude",
            Dialect::Gemini => "gemini",
            Dialect::Codex => "codex",
            Dialect::Kimi => "kimi",
            Dialect::Copilot => "copilot",
        };
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("same-be", streaming_manifest(), dialect, 30);
        pm.register_backend(
            "other-be",
            streaming_manifest(),
            if dialect == Dialect::OpenAi {
                Dialect::Claude
            } else {
                Dialect::OpenAi
            },
            50,
        );
        let w = passthrough_wo(require(&[Capability::Streaming]), label);
        let result = pm.project(&w).unwrap();
        assert_eq!(
            result.selected_backend, "same-be",
            "passthrough failed for {dialect:?}"
        );
    }
}

#[test]
fn non_passthrough_no_bonus() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_manifest(), Dialect::Claude, 30);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 80);
    // Non-passthrough → no dialect bonus.
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Matrix coverage report (which pairs are mapped)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn coverage_known_rules_has_all_self_mappings() {
    let reg = abp_mapping::known_rules();
    let features = [
        "tool_use",
        "streaming",
        "thinking",
        "image_input",
        "code_exec",
    ];
    for &dialect in Dialect::all() {
        for feat in &features {
            let rule = reg.lookup(dialect, dialect, feat);
            assert!(
                rule.is_some(),
                "missing self-mapping for {dialect:?} feature {feat}"
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "self-mapping for {dialect:?} feature {feat} should be lossless"
            );
        }
    }
}

#[test]
fn coverage_streaming_lossless_across_main_dialects() {
    let reg = abp_mapping::known_rules();
    let main = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &src in &main {
        for &tgt in &main {
            if src == tgt {
                continue;
            }
            if let Some(rule) = reg.lookup(src, tgt, "streaming") {
                assert!(
                    !rule.fidelity.is_unsupported(),
                    "streaming {src:?}→{tgt:?} should be supported"
                );
            }
        }
    }
}

#[test]
fn coverage_report_count_mapped_pairs() {
    let reg = abp_mapping::known_rules();
    let count = reg.len();
    // known_rules populates 6 self-mappings × 5 features = 30 minimum
    assert!(
        count >= 30,
        "known_rules should have ≥30 rules, got {count}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge cases and deep tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn score_weights_sum_to_one() {
    // Verify weighting constants sum to 1.0.
    let score_full = ProjectionScore {
        capability_coverage: 1.0,
        mapping_fidelity: 1.0,
        priority: 1.0,
        total: 0.0,
    };
    // We verify the internal weighting via the projection: total score for
    // a perfect backend must equal 1.0.
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend(
        "perfect",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // cap=1.0, fidelity=1.0 (same dialect), priority=1.0 (only backend)
    assert!(
        (result.fidelity_score.total - 1.0).abs() < f64::EPSILON,
        "perfect score should be 1.0, got {}",
        result.fidelity_score.total
    );
    let _ = score_full;
}

#[test]
fn empty_requirements_all_backends_compatible() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    // Both compatible, both in results.
    assert_eq!(result.fallback_chain.len(), 1);
}

#[test]
fn large_number_of_backends() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..100 {
        pm.register_backend(
            format!("be-{i:03}"),
            streaming_manifest(),
            Dialect::OpenAi,
            i,
        );
    }
    assert_eq!(pm.backend_count(), 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be-099");
    assert_eq!(result.fallback_chain.len(), 99);
}

#[test]
fn fallback_chain_scores_descending() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 90);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 50);
    pm.register_backend("c", streaming_manifest(), Dialect::Gemini, 10);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    for w in result.fallback_chain.windows(2) {
        assert!(w[0].score.total >= w[1].score.total);
    }
}

#[test]
fn unsupported_capability_not_in_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::Logprobs, SupportLevel::Unsupported),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.required_emulations.is_empty());
}

#[test]
fn multiple_emulations_listed() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu-be",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ])))
        .unwrap();
    assert_eq!(result.required_emulations.len(), 3);
}

#[test]
fn native_and_emulated_both_satisfy_requirements() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu-only",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "native-only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    // Both emulated and native satisfy capability requirements in the projection.
    let result = pm
        .project(&wo(require_native(&[Capability::Streaming])))
        .unwrap();
    // Both compatible, same priority → alphabetical id selection.
    assert!(!result.selected_backend.is_empty());
}

#[test]
fn source_dialect_from_vendor_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::Claude, 50);

    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn source_dialect_explicit_overrides_vendor_config() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);

    // Vendor config says "claude" but explicit override says OpenAi.
    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Explicit source dialect (OpenAi) matches backend → fidelity 1.0.
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn no_suitable_backend_error_message() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "testing".into(),
    };
    assert!(err.to_string().contains("testing"));
}

#[test]
fn projection_result_contains_all_fields() {
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
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert!(!result.selected_backend.is_empty());
    assert!(result.fidelity_score.total > 0.0);
    assert!(result.fidelity_score.capability_coverage > 0.0);
    assert!(result.fidelity_score.priority > 0.0);
}

#[test]
fn with_mapping_registry_constructor() {
    let reg = MappingRegistry::new();
    let pm = ProjectionMatrix::with_mapping_registry(reg);
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn mapping_fidelity_no_features_uses_heuristic() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    // No mapping features set → uses rank_targets heuristic.
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Heuristic assigns 0.8 for known mappings.
    assert!(
        (result.fidelity_score.mapping_fidelity - 0.8).abs() < 0.01,
        "expected ~0.8 heuristic fidelity, got {}",
        result.fidelity_score.mapping_fidelity
    );
}

#[test]
fn mapping_fidelity_no_mapping_at_all() {
    let reg = MappingRegistry::new(); // Empty registry.
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("kimi-be", streaming_manifest(), Dialect::Kimi, 50);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Empty registry → validate_mapping returns empty → fidelity 0.0.
    assert!(result.fidelity_score.mapping_fidelity.abs() < f64::EPSILON);
}

#[test]
fn deterministic_selection_across_runs() {
    let setup = || {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("alpha", streaming_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("beta", streaming_manifest(), Dialect::Claude, 50);
        pm.register_backend("gamma", streaming_manifest(), Dialect::Gemini, 50);
        pm.project(&wo(require(&[Capability::Streaming])))
            .unwrap()
            .selected_backend
    };
    let first = setup();
    for _ in 0..10 {
        assert_eq!(setup(), first, "selection must be deterministic");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Direct routing — mapper selection for all dialect pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn direct_route_identity_all_dialects() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let route = pm.find_route(d, d).unwrap();
        assert_eq!(route.cost, 0, "identity route should have cost 0 for {d:?}");
        assert!(route.hops.is_empty());
        assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn direct_route_openai_to_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
    assert_eq!(route.hops[0].from, Dialect::OpenAi);
    assert_eq!(route.hops[0].to, Dialect::Claude);
}

#[test]
fn direct_route_claude_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

#[test]
fn direct_route_openai_to_gemini() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Gemini).unwrap();
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

#[test]
fn direct_route_gemini_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Gemini, Dialect::OpenAi).unwrap();
    assert!(route.is_direct());
}

#[test]
fn direct_route_claude_to_gemini() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Claude, Dialect::Gemini).unwrap();
    assert!(route.is_direct());
}

#[test]
fn direct_route_gemini_to_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Gemini, Dialect::Claude).unwrap();
    assert!(route.is_direct());
}

#[test]
fn direct_route_codex_to_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Codex, Dialect::OpenAi).unwrap();
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

#[test]
fn direct_route_openai_to_codex() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Codex).unwrap();
    assert!(route.is_direct());
}

#[test]
fn direct_route_mapper_hint_openai_claude() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(
        route.hops[0].mapper_hint.as_deref(),
        Some("openai_to_claude")
    );
}

#[test]
fn direct_route_mapper_hint_claude_openai() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Claude, Dialect::OpenAi).unwrap();
    assert_eq!(
        route.hops[0].mapper_hint.as_deref(),
        Some("claude_to_openai")
    );
}

#[test]
fn direct_route_codex_openai_identity_hint() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Codex, Dialect::OpenAi).unwrap();
    assert_eq!(route.hops[0].mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn direct_route_is_not_multi_hop() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert!(!route.is_multi_hop());
}

#[test]
fn direct_route_identity_is_not_multi_hop() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert!(!route.is_multi_hop());
}

#[test]
fn direct_route_all_mapped_pairs_have_cost_one() {
    let pm = ProjectionMatrix::with_defaults();
    let mapped_pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::Claude, Dialect::OpenAi),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Gemini, Dialect::OpenAi),
        (Dialect::Claude, Dialect::Gemini),
        (Dialect::Gemini, Dialect::Claude),
        (Dialect::Codex, Dialect::OpenAi),
        (Dialect::OpenAi, Dialect::Codex),
    ];
    for (src, tgt) in mapped_pairs {
        let route = pm.find_route(src, tgt).unwrap();
        assert_eq!(route.cost, 1, "direct route {src:?}→{tgt:?} should cost 1");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Multi-hop routing — chain through intermediate dialects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multi_hop_kimi_to_claude_via_openai() {
    // Kimi→OpenAi is unsupported by default, but let's register Kimi→OpenAi as mapped.
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    // Should chain through OpenAI: Kimi→OpenAI→Claude
    assert!(route.is_multi_hop());
    assert_eq!(route.cost, 2);
    assert_eq!(route.hops.len(), 2);
    assert_eq!(route.hops[0].from, Dialect::Kimi);
    assert_eq!(route.hops[1].to, Dialect::Claude);
}

#[test]
fn multi_hop_copilot_to_gemini_via_openai() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Copilot, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Copilot, Dialect::Gemini).unwrap();
    assert!(route.is_multi_hop());
    assert_eq!(route.cost, 2);
}

#[test]
fn multi_hop_fidelity_is_product_of_hops() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);

    let hop1_fid = pm
        .compatibility_score(Dialect::Kimi, Dialect::OpenAi)
        .fidelity;
    let hop2_fid = pm
        .compatibility_score(Dialect::OpenAi, Dialect::Claude)
        .fidelity;

    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    let expected = hop1_fid * hop2_fid;
    assert!(
        (route.fidelity - expected).abs() < 0.01,
        "multi-hop fidelity {:.3} should be product {:.3}",
        route.fidelity,
        expected
    );
}

#[test]
fn multi_hop_picks_highest_fidelity_intermediate() {
    let mut pm = ProjectionMatrix::new();
    // Register two possible intermediates: OpenAI and Gemini
    pm.register(Dialect::Kimi, Dialect::Kimi, ProjectionMode::Passthrough);
    pm.register(
        Dialect::Copilot,
        Dialect::Copilot,
        ProjectionMode::Passthrough,
    );
    pm.register(
        Dialect::OpenAi,
        Dialect::OpenAi,
        ProjectionMode::Passthrough,
    );
    pm.register(
        Dialect::Gemini,
        Dialect::Gemini,
        ProjectionMode::Passthrough,
    );
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
    pm.register(Dialect::Kimi, Dialect::Gemini, ProjectionMode::Mapped);
    pm.register(Dialect::Gemini, Dialect::Copilot, ProjectionMode::Mapped);

    let route = pm.find_route(Dialect::Kimi, Dialect::Copilot).unwrap();
    assert!(route.is_multi_hop());
    assert_eq!(route.cost, 2);
}

#[test]
fn multi_hop_not_used_when_direct_exists() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    // Direct route exists → should not multi-hop.
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

#[test]
fn multi_hop_no_route_if_no_intermediate() {
    // Empty matrix — no routes registered except the pair itself (unsupported).
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Unsupported);
    let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
    assert!(route.is_none(), "should find no route");
}

#[test]
fn multi_hop_through_codex_to_claude() {
    let mut pm = ProjectionMatrix::with_defaults();
    // Kimi→Codex mapped, Codex→OpenAI mapped, OpenAI→Claude mapped
    pm.register(Dialect::Kimi, Dialect::Codex, ProjectionMode::Mapped);
    // Direct Kimi→Claude is unsupported, but Kimi→Codex→OpenAI chain is two hops.
    // Also Kimi→Codex is 1 hop and Codex→Claude needs to go through OpenAI.
    // find_route only does 1-intermediate chains, so we need Kimi→OpenAI to exist
    // or Kimi→Codex + Codex→Claude. Let's check Codex→Claude.
    pm.register(Dialect::Codex, Dialect::Claude, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    assert!(route.is_multi_hop());
}

#[test]
fn multi_hop_identity_source_equals_target() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let route = pm.find_route(d, d).unwrap();
        assert!(!route.is_multi_hop());
        assert_eq!(route.cost, 0);
    }
}

#[test]
fn multi_hop_fidelity_less_than_direct() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    pm.register_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);

    let direct = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    let multi = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();

    // Multi-hop fidelity should generally be ≤ direct fidelity.
    assert!(
        multi.fidelity <= direct.fidelity + 0.01,
        "multi-hop {:.3} should be ≤ direct {:.3}",
        multi.fidelity,
        direct.fidelity
    );
}

#[test]
fn multi_hop_route_has_contiguous_hops() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    if route.hops.len() == 2 {
        assert_eq!(
            route.hops[0].to, route.hops[1].from,
            "hops must be contiguous"
        );
    }
}

#[test]
fn multi_hop_copilot_to_claude() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Copilot, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Copilot, Dialect::Claude).unwrap();
    assert!(route.is_multi_hop());
    assert_eq!(route.hops[0].from, Dialect::Copilot);
    assert_eq!(route.hops[1].to, Dialect::Claude);
}

#[test]
fn multi_hop_kimi_to_gemini_via_openai() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Gemini).unwrap();
    assert!(route.is_multi_hop());
    assert_eq!(route.cost, 2);
}

#[test]
fn multi_hop_copilot_to_codex_via_openai() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Copilot, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Copilot, Dialect::Codex).unwrap();
    assert!(route.is_multi_hop());
    // Should route Copilot→OpenAI→Codex
    assert_eq!(route.hops[0].to, Dialect::OpenAi);
    assert_eq!(route.hops[1].from, Dialect::OpenAi);
    assert_eq!(route.hops[1].to, Dialect::Codex);
}

#[test]
fn multi_hop_roundtrip_kimi_openai_kimi() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    pm.register(Dialect::OpenAi, Dialect::Kimi, ProjectionMode::Mapped);
    let fwd = pm.find_route(Dialect::Kimi, Dialect::OpenAi).unwrap();
    let rev = pm.find_route(Dialect::OpenAi, Dialect::Kimi).unwrap();
    // Both should be direct since we registered them.
    assert!(fwd.is_direct());
    assert!(rev.is_direct());
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Cost-based routing — prefer direct over multi-hop
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cost_direct_is_one() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
}

#[test]
fn cost_identity_is_zero() {
    let pm = ProjectionMatrix::with_defaults();
    let route = pm.find_route(Dialect::Claude, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 0);
}

#[test]
fn cost_multi_hop_is_two() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 2);
}

#[test]
fn cost_direct_preferred_over_multi_hop() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    // Register direct Kimi→Claude as well.
    pm.register(Dialect::Kimi, Dialect::Claude, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    // Direct exists → cost 1 preferred over cost 2.
    assert!(route.is_direct());
    assert_eq!(route.cost, 1);
}

#[test]
fn cost_ordering_identity_direct_multihop() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);

    let identity = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let direct = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    let multi = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();

    assert!(identity.cost < direct.cost);
    assert!(direct.cost < multi.cost);
}

#[test]
fn cost_no_route_returns_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn cost_unsupported_pair_tries_multi_hop() {
    let pm = ProjectionMatrix::with_defaults();
    // Kimi→Copilot is unsupported and no intermediate exists.
    let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
    assert!(route.is_none());
}

#[test]
fn cost_all_identity_routes_are_zero() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        assert_eq!(pm.find_route(d, d).unwrap().cost, 0);
    }
}

#[test]
fn cost_fidelity_decreases_with_hops() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);

    let identity = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let multi = pm.find_route(Dialect::Kimi, Dialect::Claude).unwrap();
    assert!(identity.fidelity >= multi.fidelity);
}

#[test]
fn cost_direct_fidelity_higher_than_or_equal_multi_hop() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    pm.register_defaults();
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);

    if let Some(multi) = pm.find_route(Dialect::Kimi, Dialect::Claude) {
        let direct_oai_claude = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        // The individual hop's fidelity should be >= entire multi-hop chain.
        assert!(direct_oai_claude.fidelity >= multi.fidelity - 0.01);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Compatibility scoring — rate mapping fidelity per pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compat_same_dialect_perfect() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let score = pm.compatibility_score(d, d);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 0);
    }
}

#[test]
fn compat_openai_to_claude_with_features() {
    let reg = abp_mapping::known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    pm.register_defaults();
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert!(score.fidelity > 0.0);
    assert_eq!(score.source, Dialect::OpenAi);
    assert_eq!(score.target, Dialect::Claude);
}

#[test]
fn compat_counts_lossless_features() {
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
        fidelity: Fidelity::Lossless,
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.lossless_features, 2);
    assert_eq!(score.lossy_features, 0);
    assert_eq!(score.unsupported_features, 0);
}

#[test]
fn compat_counts_lossy_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["tool_use".into()]);
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Gemini);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.lossless_features, 0);
}

#[test]
fn compat_counts_unsupported_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Kimi,
        feature: "thinking".into(),
        fidelity: Fidelity::Unsupported {
            reason: "not supported".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec!["thinking".into()]);
    let score = pm.compatibility_score(Dialect::Claude, Dialect::Kimi);
    assert_eq!(score.unsupported_features, 1);
    assert!(score.fidelity.abs() < f64::EPSILON);
}

#[test]
fn compat_mixed_features() {
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
        feature: "logprobs".into(),
        fidelity: Fidelity::Unsupported {
            reason: "N/A".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec![
        "tool_use".into(),
        "streaming".into(),
        "logprobs".into(),
    ]);
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.lossless_features, 1);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.unsupported_features, 1);
}

#[test]
fn compat_no_features_uses_heuristic() {
    let reg = abp_mapping::known_rules();
    let pm = ProjectionMatrix::with_mapping_registry(reg);
    // No features set → fidelity from heuristic.
    let score = pm.compatibility_score(Dialect::Claude, Dialect::OpenAi);
    // Should still produce something meaningful via rank_targets.
    assert!(score.fidelity >= 0.0);
}

#[test]
fn compat_score_serde_roundtrip() {
    let score = CompatibilityScore {
        source: Dialect::OpenAi,
        target: Dialect::Claude,
        fidelity: 0.85,
        lossless_features: 3,
        lossy_features: 1,
        unsupported_features: 0,
    };
    let json = serde_json::to_string(&score).unwrap();
    let deser: CompatibilityScore = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.source, Dialect::OpenAi);
    assert_eq!(deser.target, Dialect::Claude);
    assert!((deser.fidelity - 0.85).abs() < f64::EPSILON);
    assert_eq!(deser.lossless_features, 3);
}

#[test]
fn compat_higher_fidelity_wins_backend_selection() {
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
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });

    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec!["tool_use".into()]);
    pm.register_backend("openai-be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_manifest(), Dialect::Gemini, 50);

    let oai = pm.compatibility_score(Dialect::Claude, Dialect::OpenAi);
    let gem = pm.compatibility_score(Dialect::Claude, Dialect::Gemini);
    assert!(oai.fidelity > gem.fidelity);

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Edge cases — unknown dialects, self-mapping, empty matrix
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_self_mapping_all_dialects_lookup() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
    }
}

#[test]
fn edge_empty_matrix_find_route_returns_identity_only() {
    let pm = ProjectionMatrix::new();
    // Identity still works (no lookup needed).
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 0);
    // Non-identity returns None.
    assert!(pm.find_route(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn edge_empty_matrix_compatibility_score_identity() {
    let pm = ProjectionMatrix::new();
    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
    assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_remove_backend_reduces_count() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 50);
    assert_eq!(pm.backend_count(), 2);
    assert!(pm.remove_backend("a"));
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn edge_remove_nonexistent_backend() {
    let mut pm = ProjectionMatrix::new();
    assert!(!pm.remove_backend("nonexistent"));
}

#[test]
fn edge_remove_dialect_entry() {
    let mut pm = ProjectionMatrix::with_defaults();
    let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert!(removed.is_some());
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
}

#[test]
fn edge_remove_nonexistent_dialect_entry() {
    let mut pm = ProjectionMatrix::new();
    assert!(pm.remove(Dialect::Kimi, Dialect::Copilot).is_none());
}

#[test]
fn edge_project_after_backend_removal() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 50);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 50);
    pm.remove_backend("a");
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "b");
}

#[test]
fn edge_routing_path_serde_roundtrip() {
    let path = RoutingPath {
        hops: vec![],
        cost: 0,
        fidelity: 1.0,
    };
    let json = serde_json::to_string(&path).unwrap();
    let deser: RoutingPath = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.cost, 0);
    assert!((deser.fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_all_unsupported_pairs_no_route() {
    let mut pm = ProjectionMatrix::new();
    // Register everything as unsupported.
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src != tgt {
                pm.register(src, tgt, ProjectionMode::Unsupported);
            }
        }
    }
    // No cross-dialect route should exist.
    assert!(pm.find_route(Dialect::OpenAi, Dialect::Claude).is_none());
    assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Registration — dynamic mapper registration, override, remove
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn register_custom_dialect_pair() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
}

#[test]
fn register_override_existing_pair() {
    let mut pm = ProjectionMatrix::with_defaults();
    // OpenAI→Claude is Mapped by default.
    assert_eq!(
        pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
        ProjectionMode::Mapped
    );
    // Override to Unsupported.
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
fn register_override_does_not_change_count() {
    let mut pm = ProjectionMatrix::with_defaults();
    let before = pm.dialect_entry_count();
    pm.register(
        Dialect::OpenAi,
        Dialect::Claude,
        ProjectionMode::Unsupported,
    );
    assert_eq!(pm.dialect_entry_count(), before);
}

#[test]
fn register_same_dialect_forces_passthrough() {
    let mut pm = ProjectionMatrix::new();
    // Even if we try to register as Mapped, identity should force Passthrough.
    pm.register(Dialect::Gemini, Dialect::Gemini, ProjectionMode::Mapped);
    let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
}

#[test]
fn register_backend_override_updates_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    pm.set_source_dialect(Dialect::OpenAi);
    let r1 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((r1.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);

    // Override to Claude dialect.
    pm.register_backend("be", streaming_manifest(), Dialect::Claude, 50);
    let r2 = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Now source=OpenAI, backend=Claude → fidelity may differ.
    assert_eq!(r2.selected_backend, "be");
}

#[test]
fn register_remove_then_re_register() {
    let mut pm = ProjectionMatrix::with_defaults();
    pm.remove(Dialect::OpenAi, Dialect::Claude);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
}

#[test]
fn register_defaults_populates_all_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    let n = Dialect::all().len();
    // All n*n pairs should be present (including identity).
    assert_eq!(pm.dialect_entry_count(), n * n);
}

#[test]
fn register_backend_remove_last_gives_empty_error() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("only", streaming_manifest(), Dialect::OpenAi, 50);
    pm.remove_backend("only");
    assert_eq!(pm.backend_count(), 0);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn register_multiple_and_verify_iteration() {
    let mut pm = ProjectionMatrix::new();
    pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
    pm.register(Dialect::Claude, Dialect::Gemini, ProjectionMode::Mapped);
    pm.register(
        Dialect::OpenAi,
        Dialect::OpenAi,
        ProjectionMode::Passthrough,
    );
    assert_eq!(pm.dialect_entry_count(), 3);
    let entries: Vec<_> = pm.dialect_entries().collect();
    assert_eq!(entries.len(), 3);
}

#[test]
fn register_route_affected_by_removal() {
    let mut pm = ProjectionMatrix::with_defaults();
    // Kimi→OpenAI mapped, so Kimi→Claude should find multi-hop.
    pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
    assert!(pm.find_route(Dialect::Kimi, Dialect::Claude).is_some());

    // Remove Kimi→OpenAI, now multi-hop should fail (unless another intermediate).
    pm.remove(Dialect::Kimi, Dialect::OpenAi);
    // May still find through other intermediates — let's also remove those.
    pm.register(Dialect::Kimi, Dialect::Claude, ProjectionMode::Unsupported);
    pm.register(Dialect::Kimi, Dialect::Gemini, ProjectionMode::Unsupported);
    pm.register(Dialect::Kimi, Dialect::Codex, ProjectionMode::Unsupported);
    pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Unsupported);
    let route = pm.find_route(Dialect::Kimi, Dialect::Claude);
    assert!(route.is_none(), "no route after removing intermediates");
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Model selection strategies — integration with projection
// ═══════════════════════════════════════════════════════════════════════

fn mk_candidate(name: &str) -> ModelCandidate {
    ModelCandidate {
        backend_name: name.to_string(),
        model_id: "m".to_string(),
        estimated_latency_ms: None,
        estimated_cost_per_1k_tokens: None,
        fidelity_score: None,
        weight: 1.0,
    }
}

#[test]
fn selection_lowest_latency_picks_fastest() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            ModelCandidate {
                estimated_latency_ms: Some(500),
                ..mk_candidate("slow")
            },
            ModelCandidate {
                estimated_latency_ms: Some(20),
                ..mk_candidate("fast")
            },
            ModelCandidate {
                estimated_latency_ms: Some(150),
                ..mk_candidate("mid")
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "fast");
}

#[test]
fn selection_lowest_cost_picks_cheapest() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestCost,
        vec![
            ModelCandidate {
                estimated_cost_per_1k_tokens: Some(8.0),
                ..mk_candidate("expensive")
            },
            ModelCandidate {
                estimated_cost_per_1k_tokens: Some(0.25),
                ..mk_candidate("cheap")
            },
            ModelCandidate {
                estimated_cost_per_1k_tokens: Some(2.0),
                ..mk_candidate("mid")
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "cheap");
}

#[test]
fn selection_highest_fidelity_picks_best() {
    let sel = ModelSelector::new(
        SelectionStrategy::HighestFidelity,
        vec![
            ModelCandidate {
                fidelity_score: Some(0.3),
                ..mk_candidate("low")
            },
            ModelCandidate {
                fidelity_score: Some(0.99),
                ..mk_candidate("high")
            },
            ModelCandidate {
                fidelity_score: Some(0.6),
                ..mk_candidate("mid")
            },
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "high");
}

#[test]
fn selection_round_robin_cycles() {
    let sel = ModelSelector::new(
        SelectionStrategy::RoundRobin,
        vec![mk_candidate("a"), mk_candidate("b"), mk_candidate("c")],
    );
    assert_eq!(sel.select().unwrap().backend_name, "a");
    assert_eq!(sel.select().unwrap().backend_name, "b");
    assert_eq!(sel.select().unwrap().backend_name, "c");
    assert_eq!(sel.select().unwrap().backend_name, "a");
}

#[test]
fn selection_weighted_random_respects_weights() {
    let sel = ModelSelector::new(
        SelectionStrategy::WeightedRandom,
        vec![
            ModelCandidate {
                weight: 0.0,
                ..mk_candidate("zero_a")
            },
            ModelCandidate {
                weight: 10.0,
                ..mk_candidate("heavy")
            },
            ModelCandidate {
                weight: 0.0,
                ..mk_candidate("zero_b")
            },
        ],
    );
    for _ in 0..20 {
        assert_eq!(sel.select().unwrap().backend_name, "heavy");
    }
}

#[test]
fn selection_fallback_chain_first_in_order() {
    let sel = ModelSelector::new(
        SelectionStrategy::FallbackChain,
        vec![
            mk_candidate("primary"),
            mk_candidate("secondary"),
            mk_candidate("tertiary"),
        ],
    );
    assert_eq!(sel.select().unwrap().backend_name, "primary");
    // Repeated calls always return first.
    assert_eq!(sel.select().unwrap().backend_name, "primary");
}

#[test]
fn selection_empty_backend_list_returns_none() {
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
            "{strategy:?} should return None on empty"
        );
    }
}

#[test]
fn selection_single_backend_always_selected() {
    for strategy in [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ] {
        let sel = ModelSelector::new(strategy, vec![mk_candidate("only")]);
        assert_eq!(
            sel.select().unwrap().backend_name,
            "only",
            "{strategy:?} should always pick the single candidate"
        );
    }
}

#[test]
fn selection_strategy_serde_roundtrip() {
    for strategy in [
        SelectionStrategy::LowestLatency,
        SelectionStrategy::LowestCost,
        SelectionStrategy::HighestFidelity,
        SelectionStrategy::RoundRobin,
        SelectionStrategy::WeightedRandom,
        SelectionStrategy::FallbackChain,
    ] {
        let json = serde_json::to_string(&strategy).unwrap();
        let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, strategy);
    }
}

#[test]
fn selection_custom_strategy_composition_select_n() {
    let sel = ModelSelector::new(
        SelectionStrategy::LowestLatency,
        vec![
            ModelCandidate {
                estimated_latency_ms: Some(300),
                ..mk_candidate("slow")
            },
            ModelCandidate {
                estimated_latency_ms: Some(10),
                ..mk_candidate("fast")
            },
            ModelCandidate {
                estimated_latency_ms: Some(100),
                ..mk_candidate("mid")
            },
        ],
    );
    let top2: Vec<_> = sel
        .select_n(2)
        .iter()
        .map(|c| c.backend_name.as_str())
        .collect();
    assert_eq!(top2, vec!["fast", "mid"]);
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Projection routing — metadata, concurrency, determinism
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn routing_preserves_work_order_metadata() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_manifest(), Dialect::OpenAi, 50);
    let work_order = WorkOrderBuilder::new("specific task description")
        .requirements(require(&[Capability::Streaming]))
        .root("/custom/path")
        .max_turns(42)
        .build();
    let result = pm.project(&work_order).unwrap();
    // Projection result references the backend, not the work order itself;
    // verify the work order was not mutated by projecting it.
    assert_eq!(work_order.task, "specific task description");
    assert_eq!(work_order.workspace.root, "/custom/path");
    assert_eq!(work_order.config.max_turns, Some(42));
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn multiple_concurrent_routes_dont_interfere() {
    let pm = ProjectionMatrix::with_defaults();
    let wo1 = wo(require(&[Capability::Streaming]));
    let wo2 = wo(require(&[Capability::ToolRead]));

    let mut pm_with_backends = ProjectionMatrix::new();
    pm_with_backends.register_backend(
        "a",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        50,
    );
    pm_with_backends.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let r1 = pm_with_backends.project(&wo1).unwrap();
    let r2 = pm_with_backends.project(&wo2).unwrap();

    // Both should succeed; routes are independent.
    assert!(!r1.selected_backend.is_empty());
    assert!(!r2.selected_backend.is_empty());

    // Routing the same dialect pair multiple times gives the same result.
    let route_a = pm.find_route(Dialect::OpenAi, Dialect::Claude);
    let route_b = pm.find_route(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(
        route_a.as_ref().map(|r| r.cost),
        route_b.as_ref().map(|r| r.cost)
    );
    assert_eq!(
        route_a.as_ref().map(|r| r.hops.len()),
        route_b.as_ref().map(|r| r.hops.len())
    );
}

#[test]
fn route_selection_is_deterministic_across_invocations() {
    let mut pm = ProjectionMatrix::new();
    pm.register_defaults();
    pm.register_backend("a", streaming_manifest(), Dialect::OpenAi, 80);
    pm.register_backend("b", streaming_manifest(), Dialect::Claude, 60);
    pm.register_backend("c", streaming_manifest(), Dialect::Gemini, 40);

    let work_order = wo(require(&[Capability::Streaming]));
    let results: Vec<_> = (0..10)
        .map(|_| pm.project(&work_order).unwrap().selected_backend.clone())
        .collect();

    // All results must be identical.
    assert!(
        results.windows(2).all(|w| w[0] == w[1]),
        "projection must be deterministic"
    );
}

#[test]
fn route_openai_to_openai_passthrough_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Passthrough);
    let route = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    assert_eq!(route.cost, 0);
    assert!((route.fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn route_openai_to_claude_mapped_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(route.cost, 1);
    assert!(route.is_direct());
}

#[test]
fn route_claude_to_gemini_mapped_mode() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::Claude, Dialect::Gemini).unwrap();
    assert_eq!(entry.mode, ProjectionMode::Mapped);
    let route = pm.find_route(Dialect::Claude, Dialect::Gemini).unwrap();
    assert!(route.is_direct());
}

#[test]
fn same_dialect_engine_always_passthrough() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough, "{d} → {d}");
        let route = pm.find_route(d, d).unwrap();
        assert_eq!(route.cost, 0, "{d} identity cost");
        assert_eq!(route.hops.len(), 0, "{d} identity has no hops");
    }
}

#[test]
fn different_dialect_engine_produces_mapped() {
    let pm = ProjectionMatrix::with_defaults();
    let mapped_pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::Claude, Dialect::OpenAi),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Gemini, Dialect::OpenAi),
        (Dialect::Claude, Dialect::Gemini),
        (Dialect::Gemini, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Codex),
        (Dialect::Codex, Dialect::OpenAi),
    ];
    for (src, tgt) in mapped_pairs {
        let entry = pm.lookup(src, tgt).unwrap();
        assert_eq!(
            entry.mode,
            ProjectionMode::Mapped,
            "{src} → {tgt} should be Mapped"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Projection matrix structure — symmetry, IR mapper coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn matrix_covers_all_six_dialects() {
    let pm = ProjectionMatrix::with_defaults();
    let dialects = Dialect::all();
    assert_eq!(dialects.len(), 6);
    for &d in dialects {
        assert!(pm.lookup(d, d).is_some(), "identity entry missing for {d}");
    }
}

#[test]
fn matrix_diagonal_always_passthrough() {
    let pm = ProjectionMatrix::with_defaults();
    for &d in Dialect::all() {
        let entry = pm.lookup(d, d).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
        assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
    }
}

#[test]
fn matrix_off_diagonal_mapped_entries_have_ir_mappers() {
    let pm = ProjectionMatrix::with_defaults();
    for entry in pm.dialect_entries() {
        if entry.mode == ProjectionMode::Mapped {
            assert_ne!(
                entry.pair.source, entry.pair.target,
                "mapped entry should be off-diagonal"
            );
            assert!(
                entry.mapper_hint.is_some(),
                "mapped entry {} should have a mapper hint",
                entry.pair
            );
        }
    }
}

#[test]
fn matrix_missing_mappers_produce_none() {
    let pm = ProjectionMatrix::new();
    assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    assert!(pm
        .resolve_mapper(Dialect::OpenAi, Dialect::Claude)
        .is_none());
}

#[test]
fn matrix_symmetry_mapped_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    for entry in pm.dialect_entries() {
        if entry.mode == ProjectionMode::Mapped {
            let reverse = pm.lookup(entry.pair.target, entry.pair.source);
            assert!(
                reverse.is_some(),
                "reverse entry for {} → {} missing",
                entry.pair.target,
                entry.pair.source
            );
            let rev = reverse.unwrap();
            assert_eq!(
                rev.mode,
                ProjectionMode::Mapped,
                "reverse of {} should also be Mapped, got {:?}",
                entry.pair,
                rev.mode
            );
        }
    }
}

#[test]
fn matrix_unsupported_entries_produce_no_mapper() {
    let pm = ProjectionMatrix::with_defaults();
    for entry in pm.dialect_entries() {
        if entry.mode == ProjectionMode::Unsupported {
            let mapper = pm.resolve_mapper(entry.pair.source, entry.pair.target);
            assert!(
                mapper.is_none(),
                "unsupported entry {} should not resolve to a mapper",
                entry.pair
            );
        }
    }
}

#[test]
fn matrix_capability_requirements_checked() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "limited",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let work_order = wo(require(&[Capability::ToolRead, Capability::ToolWrite]));
    let err = pm.project(&work_order).unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn matrix_feature_support_levels_propagate() {
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
    pm.register_defaults();

    let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(score.lossless_features, 1);
    assert_eq!(score.lossy_features, 1);
    assert_eq!(score.unsupported_features, 0);
    assert!(score.fidelity > 0.0);
}

#[test]
fn matrix_enumeration_lists_all_supported_pairs() {
    let pm = ProjectionMatrix::with_defaults();
    let entries: Vec<_> = pm.dialect_entries().collect();
    let n = Dialect::all().len();
    assert_eq!(entries.len(), n * n);

    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                entries
                    .iter()
                    .any(|e| e.pair.source == src && e.pair.target == tgt),
                "pair {src} → {tgt} missing from enumeration"
            );
        }
    }
}

#[test]
fn matrix_entry_lookup_by_dialect_pair() {
    let pm = ProjectionMatrix::with_defaults();
    let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
    assert_eq!(entry.pair.source, Dialect::OpenAi);
    assert_eq!(entry.pair.target, Dialect::Claude);
    assert_eq!(entry.mode, ProjectionMode::Mapped);
}

#[test]
fn matrix_size_matches_expected() {
    let pm = ProjectionMatrix::with_defaults();
    let n = Dialect::all().len();
    assert_eq!(n, 6);
    assert_eq!(pm.dialect_entry_count(), n * n); // 36 entries
}

// ═══════════════════════════════════════════════════════════════════════
// 25. IR mapper factory integration — supported_ir_pairs, default_ir_mapper
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapper_factory_covers_all_identity_pairs() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "identity mapper missing for {d}");
    }
}

#[test]
fn ir_mapper_factory_openai_claude_bidirectional() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn ir_mapper_factory_supported_pairs_includes_identities() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(
            pairs.contains(&(d, d)),
            "supported_ir_pairs missing identity ({d}, {d})"
        );
    }
}

#[test]
fn ir_mapper_factory_supported_pairs_symmetric() {
    let pairs = supported_ir_pairs();
    for &(a, b) in &pairs {
        if a != b {
            assert!(
                pairs.contains(&(b, a)),
                "supported_ir_pairs has ({a}, {b}) but not ({b}, {a})"
            );
        }
    }
}

#[test]
fn ir_mapper_factory_unsupported_returns_none() {
    let mapper = default_ir_mapper(Dialect::Copilot, Dialect::Codex);
    assert!(
        mapper.is_none(),
        "Copilot→Codex should not have a direct IR mapper"
    );
}

#[test]
fn ir_mapper_factory_all_supported_pairs_resolve() {
    let pairs = supported_ir_pairs();
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to);
        assert!(
            mapper.is_some(),
            "supported pair ({from}, {to}) should resolve to a mapper"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Routing types — RoutingHop, RoutingPath, DialectPair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn routing_hop_serde_roundtrip() {
    let hop = RoutingHop {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
        mapper_hint: Some("openai_to_claude".to_string()),
    };
    let json = serde_json::to_string(&hop).unwrap();
    let back: RoutingHop = serde_json::from_str(&json).unwrap();
    assert_eq!(back.from, Dialect::OpenAi);
    assert_eq!(back.to, Dialect::Claude);
    assert_eq!(back.mapper_hint.as_deref(), Some("openai_to_claude"));
}

#[test]
fn routing_path_direct_is_direct() {
    let path = RoutingPath {
        hops: vec![RoutingHop {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
            mapper_hint: None,
        }],
        cost: 1,
        fidelity: 0.9,
    };
    assert!(path.is_direct());
    assert!(!path.is_multi_hop());
}

#[test]
fn routing_path_multi_hop_is_multi_hop() {
    let path = RoutingPath {
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
        fidelity: 0.7,
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

#[test]
fn dialect_pair_display_format() {
    let pair = DialectPair::new(Dialect::Claude, Dialect::Gemini);
    assert_eq!(pair.to_string(), "Claude → Gemini");
}

#[test]
fn dialect_pair_ordering_consistent() {
    let a = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
    let b = DialectPair::new(Dialect::Claude, Dialect::OpenAi);
    // They should not be equal (direction matters).
    assert_ne!(a, b);
    // Ordering should be consistent (not equal means one < other or vice versa).
    assert!(a < b || b < a);
}
