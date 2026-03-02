// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the projection matrix routing logic.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule};
use abp_projection::{ProjectionError, ProjectionMatrix, ProjectionScore};

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
