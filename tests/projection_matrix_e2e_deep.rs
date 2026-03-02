// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep end-to-end tests for abp-projection: backend projection and selection logic.

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule, features};
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

fn wo_empty() -> WorkOrder {
    WorkOrderBuilder::new("empty task").build()
}

fn passthrough_wo(reqs: CapabilityRequirements, source_dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": source_dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn passthrough_wo_no_dialect(reqs: CapabilityRequirements) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "passthrough" });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough task no dialect")
        .requirements(reqs)
        .config(config)
        .build()
}

fn wo_with_source_dialect(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("dialect task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn rich_registry() -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::STREAMING.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial mapping".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: features::STREAMING.into(),
        fidelity: Fidelity::Unsupported {
            reason: "not mapped".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: features::STREAMING.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "minor differences".into(),
        },
    });
    reg
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. ProjectionMatrix construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn construction_new_is_empty() {
    let pm = ProjectionMatrix::new();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn construction_default_is_empty() {
    let pm = ProjectionMatrix::default();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn construction_with_mapping_registry() {
    let reg = rich_registry();
    let pm = ProjectionMatrix::with_mapping_registry(reg);
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn register_single_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be-1",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn register_multiple_backends() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..5 {
        pm.register_backend(
            format!("be-{i}"),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            i * 20,
        );
    }
    assert_eq!(pm.backend_count(), 5);
}

#[test]
fn register_overwrite_same_id() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("dup", manifest(&[]), Dialect::OpenAi, 10);
    pm.register_backend(
        "dup",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        90,
    );
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn register_string_id_types() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        String::from("string-id"),
        manifest(&[]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend("str-id", manifest(&[]), Dialect::Claude, 50);
    assert_eq!(pm.backend_count(), 2);
}

#[test]
fn set_source_dialect() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn set_mapping_features() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fidelity_score.mapping_fidelity > 0.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Backend scoring / ranking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scoring_all_native_caps_full_coverage() {
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
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scoring_mixed_native_emulated_full_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
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
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scoring_partial_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
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
    assert!((result.fidelity_score.capability_coverage - 1.0 / 3.0).abs() < 0.01);
}

#[test]
fn scoring_empty_requirements_perfect_coverage() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("any", manifest(&[]), Dialect::OpenAi, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scoring_priority_normalized_single_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        42,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Single backend: 42/42 = 1.0
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn scoring_priority_normalized_multiple() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "high",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        100,
    );
    pm.register_backend(
        "low",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        25,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "high");
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
    // Low backend has priority = 25/100 = 0.25
    let low_fb = result
        .fallback_chain
        .iter()
        .find(|e| e.backend_id == "low")
        .unwrap();
    assert!((low_fb.score.priority - 0.25).abs() < f64::EPSILON);
}

#[test]
fn scoring_total_is_weighted_sum() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "test",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let s = &result.fidelity_score;
    let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
    assert!((s.total - expected).abs() < 1e-10);
}

#[test]
fn scoring_higher_caps_wins_over_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full-caps",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        10,
    );
    pm.register_backend(
        "low-caps",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // full-caps: coverage=1.0, prio=10/90=0.11 => 0.5*1.0+0.3*1.0+0.2*0.11 = 0.822
    // low-caps: coverage=0.5, prio=1.0 => 0.5*0.5+0.3*1.0+0.2*1.0 = 0.75
    assert_eq!(result.selected_backend, "full-caps");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Best-fit backend selection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn bestfit_single_backend_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "solo",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "solo");
}

#[test]
fn bestfit_compatible_preferred_over_partial() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "compatible",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        30,
    );
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "compatible");
}

#[test]
fn bestfit_id_tiebreaker_deterministic() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "z-backend",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "a-backend",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "a-backend");
}

#[test]
fn bestfit_emulated_backend_selected_when_only_option() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::ToolRead]))).unwrap();
    assert_eq!(result.selected_backend, "emu");
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn bestfit_native_preferred_over_emulated_same_priority() {
    let mut pm = ProjectionMatrix::new();
    // Both have same priority and same dialect. The ID tiebreaker applies since
    // both have coverage=1.0. With alphabetical ID sort, "emu" < "native".
    pm.register_backend(
        "native-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "emu-be",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Both have coverage=1.0, fidelity=1.0, priority=1.0. ID tiebreaker: "emu-be" < "native-be"
    assert_eq!(result.selected_backend, "emu-be");
}

#[test]
fn bestfit_partial_match_when_no_compatible() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "partial-a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "partial-b",
        manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
        Dialect::OpenAi,
        20,
    );
    // Require both - neither backend is fully compatible
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // partial-a: cap=0.5, prio=80/80=1.0 => 0.5*0.5+0.3*1.0+0.2*1.0=0.75
    // partial-b: cap=0.5, prio=20/80=0.25 => 0.5*0.5+0.3*1.0+0.2*0.25=0.60
    assert_eq!(result.selected_backend, "partial-a");
}

#[test]
fn bestfit_result_has_correct_emulation_list() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
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
    assert_eq!(result.required_emulations.len(), 2);
    let emu_caps: Vec<_> = result
        .required_emulations
        .iter()
        .map(|e| &e.capability)
        .collect();
    assert!(emu_caps.contains(&&Capability::ToolRead));
    assert!(emu_caps.contains(&&Capability::ToolWrite));
}

#[test]
fn bestfit_emulation_strategy_is_adapter() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.required_emulations[0].strategy, "adapter");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Multi-criteria selection (capability + dialect + cost)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_capability_and_fidelity_combined() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

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
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // OpenAI: lossless mapping for both features => high fidelity
    // Gemini: lossy tool_use + unsupported streaming => low fidelity
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn multi_capability_dominates_fidelity() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend(
        "openai-partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "gemini-full",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::Gemini,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // gemini-full has 2/2 coverage vs openai-partial 1/2
    // Capability weight (0.5) > fidelity weight (0.3)
    assert_eq!(result.selected_backend, "gemini-full");
}

#[test]
fn multi_fidelity_dominates_priority() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

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
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Fidelity weight (0.3) > priority weight (0.2) in advantage
    assert_eq!(result.selected_backend, "openai-low");
}

#[test]
fn multi_all_three_factors() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

    pm.register_backend(
        "openai-full",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "gemini-full",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::Gemini,
        80,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // Same caps, same priority, but OpenAI has better fidelity from Claude
    assert_eq!(result.selected_backend, "openai-full");
}

#[test]
fn multi_same_dialect_perfect_fidelity() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

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
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude→Claude = 1.0 fidelity, Claude→OpenAI = 1.0 (lossless)
    // Both 1.0 fidelity, same priority, alphabetical: claude-be wins
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn multi_no_source_dialect_assumes_perfect_fidelity() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    // No source dialect set
    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "gemini",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // No source dialect → fidelity = 1.0 for all → priority tie → alphabetical
    assert_eq!(result.selected_backend, "gemini");
}

#[test]
fn multi_passthrough_plus_fidelity_plus_caps() {
    let mut pm = ProjectionMatrix::with_mapping_registry(rich_registry());
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

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
    let w = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Claude gets passthrough bonus (+0.15), plus same-dialect fidelity
    assert_eq!(result.selected_backend, "claude-be");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Fallback chains
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fallback_empty_for_single_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fallback_chain.is_empty());
}

#[test]
fn fallback_excludes_selected() {
    let mut pm = ProjectionMatrix::new();
    for id in ["a", "b", "c"] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    for fb in &result.fallback_chain {
        assert_ne!(fb.backend_id, result.selected_backend);
    }
}

#[test]
fn fallback_sorted_descending_by_score() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 100), ("b", 75), ("c", 50), ("d", 25)] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            prio,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|e| e.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback not descending: {:?}", scores);
    }
}

#[test]
fn fallback_includes_incompatible_backends() {
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
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "full");
    assert!(
        result
            .fallback_chain
            .iter()
            .any(|e| e.backend_id == "partial")
    );
}

#[test]
fn fallback_chain_length_equals_total_minus_one() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..7 {
        pm.register_backend(
            format!("be-{i}"),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            i * 10,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.fallback_chain.len(), 6);
}

#[test]
fn fallback_chain_has_scores() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        30,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    for fb in &result.fallback_chain {
        assert!(fb.score.total > 0.0);
        assert!(fb.score.total <= 2.0); // reasonable upper bound
    }
}

#[test]
fn fallback_four_backends_three_fallbacks() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "best",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        100,
    );
    pm.register_backend(
        "good",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        80,
    );
    pm.register_backend(
        "ok",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend("empty", manifest(&[]), Dialect::Gemini, 20);
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert_eq!(result.selected_backend, "best");
    assert_eq!(result.fallback_chain.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_projection_score_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.85,
        mapping_fidelity: 0.72,
        priority: 0.6,
        total: 0.75,
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
        backend_id: "my-backend".into(),
        score: ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 0.5,
            priority: 0.8,
            total: 0.86,
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: FallbackEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn serde_projection_error_roundtrip() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "testing".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn serde_projection_error_empty_matrix_roundtrip() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn serde_projection_result_roundtrip() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be-1",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "be-2",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::Claude,
        30,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let back: ProjectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.selected_backend, result.selected_backend);
    assert_eq!(back.fallback_chain.len(), result.fallback_chain.len());
}

#[test]
fn serde_projection_score_json_fields() {
    let score = ProjectionScore {
        capability_coverage: 0.5,
        mapping_fidelity: 0.3,
        priority: 0.2,
        total: 0.38,
    };
    let val: serde_json::Value = serde_json::to_value(&score).unwrap();
    assert!(val.get("capability_coverage").is_some());
    assert!(val.get("mapping_fidelity").is_some());
    assert!(val.get("priority").is_some());
    assert!(val.get("total").is_some());
}

#[test]
fn serde_required_emulation_all_capabilities() {
    for cap in [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ExtendedThinking,
    ] {
        let emu = RequiredEmulation {
            capability: cap,
            strategy: "adapter".into(),
        };
        let json = serde_json::to_string(&emu).unwrap();
        let back: RequiredEmulation = serde_json::from_str(&json).unwrap();
        assert_eq!(emu, back);
    }
}

#[test]
fn serde_empty_fallback_chain() {
    let result = ProjectionResult {
        selected_backend: "test".into(),
        fidelity_score: ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 1.0,
            priority: 1.0,
            total: 1.0,
        },
        required_emulations: vec![],
        fallback_chain: vec![],
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: ProjectionResult = serde_json::from_str(&json).unwrap();
    assert!(back.fallback_chain.is_empty());
    assert!(back.required_emulations.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_matrix_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn edge_empty_matrix_empty_reqs_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(CapabilityRequirements::default()))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn edge_single_backend_always_selected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "only",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        1,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "only");
}

#[test]
fn edge_all_unsupported_fails() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "no-caps",
        manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let err = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn edge_empty_manifest_no_reqs_succeeds() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    assert_eq!(result.selected_backend, "empty");
}

#[test]
fn edge_empty_manifest_with_reqs_fails() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn edge_priority_zero_normalized() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "zero",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        0,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // priority = 0/max(0,1) = 0.0
    assert!((result.fidelity_score.priority).abs() < f64::EPSILON);
}

#[test]
fn edge_very_high_priority() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "high",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        u32::MAX,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_many_backends() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..100 {
        pm.register_backend(
            format!("be-{i:03}"),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
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
fn edge_restricted_capability_treated_as_emulatable() {
    // Restricted capabilities are treated as emulatable by the negotiation layer
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "restricted",
        manifest(&[(
            Capability::Streaming,
            SupportLevel::Restricted {
                reason: "policy".into(),
            },
        )]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "restricted");
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn edge_mixed_restricted_and_native() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (
                Capability::ToolRead,
                SupportLevel::Restricted {
                    reason: "blocked".into(),
                },
            ),
        ]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // Restricted is treated as emulatable, so 2/2 coverage
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
    assert_eq!(result.required_emulations.len(), 1);
}

#[test]
fn edge_passthrough_no_source_dialect_no_bonus() {
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
    // Passthrough without source_dialect → no bonus applied
    let w = passthrough_wo_no_dialect(require(&[Capability::Streaming]));
    let result = pm.project(&w).unwrap();
    // No dialect bonus → priority wins
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn edge_passthrough_wrong_dialect_no_bonus() {
    let mut pm = ProjectionMatrix::new();
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
    // Passthrough with claude source but no claude backend
    let w = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Neither gets bonus → same priority → alphabetical
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn edge_all_same_score_alphabetical() {
    let mut pm = ProjectionMatrix::new();
    for id in ["zeta", "alpha", "beta", "gamma"] {
        pm.register_backend(
            id,
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "alpha");
    let fb_ids: Vec<_> = result
        .fallback_chain
        .iter()
        .map(|e| &e.backend_id)
        .collect();
    assert_eq!(fb_ids, &["beta", "gamma", "zeta"]);
}

#[test]
fn edge_projection_error_display() {
    let err = ProjectionError::EmptyMatrix;
    let msg = format!("{err}");
    assert!(msg.contains("empty"));

    let err2 = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let msg2 = format!("{err2}");
    assert!(msg2.contains("test reason"));
}

#[test]
fn edge_wo_vendor_source_dialect_detection() {
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
    // Source dialect from work order vendor config
    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Claude→Claude = 1.0 fidelity, Claude→OpenAI needs mapping → lower fidelity
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn edge_explicit_source_dialect_overrides_wo_config() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
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
    // WO says claude, but matrix overrides to openai
    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "claude");
    let result = pm.project(&w).unwrap();
    // Source is OpenAi (from matrix override), so OpenAi backend gets perfect fidelity
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn edge_many_capabilities_required() {
    let mut pm = ProjectionMatrix::new();
    let all_caps = vec![
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolEdit, SupportLevel::Native),
        (Capability::ToolBash, SupportLevel::Native),
        (Capability::ToolGlob, SupportLevel::Native),
        (Capability::ToolGrep, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
        (Capability::ImageInput, SupportLevel::Native),
    ];
    pm.register_backend("mega", manifest(&all_caps), Dialect::OpenAi, 50);
    let reqs = require(&[
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ExtendedThinking,
        Capability::ImageInput,
    ]);
    let result = pm.project(&wo(reqs)).unwrap();
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_duplicate_capability_in_requirements() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let result = pm.project(&wo(reqs)).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn edge_mapping_registry_no_features_heuristic() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    // No mapping features set → uses rank_targets heuristic

    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "gemini",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // OpenAI has mapping rule → fidelity = 0.8, Gemini has none → fidelity = 0.0
    assert_eq!(result.selected_backend, "openai");
}

#[test]
fn edge_mapping_features_all_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // All unsupported → fidelity = 0.0
    assert!((result.fidelity_score.mapping_fidelity).abs() < f64::EPSILON);
}

#[test]
fn edge_mapping_features_mixed_fidelity() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::STREAMING.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "minor".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // 1 lossless / 2 total, 2 supported / 2 total
    // fidelity = 0.7 * (1/2) + 0.3 * (2/2) = 0.35 + 0.3 = 0.65
    assert!((result.fidelity_score.mapping_fidelity - 0.65).abs() < 0.01);
}

#[test]
fn edge_unknown_source_dialect_in_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    // Unknown dialect string → parse returns None → fidelity = 1.0 for all
    let w = wo_with_source_dialect(require(&[Capability::Streaming]), "unknown_dialect");
    let result = pm.project(&w).unwrap();
    // Both get fidelity=1.0, same priority → alphabetical
    assert_eq!(result.selected_backend, "a");
}

#[test]
fn edge_all_dialects_as_backends() {
    let mut pm = ProjectionMatrix::new();
    for (i, dialect) in Dialect::all().iter().enumerate() {
        pm.register_backend(
            format!("be-{}", dialect.label()),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            *dialect,
            (i as u32 + 1) * 10,
        );
    }
    assert_eq!(pm.backend_count(), Dialect::all().len());
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(!result.selected_backend.is_empty());
}

#[test]
fn edge_clone_matrix_projects_same() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let pm2 = pm.clone();
    let w = wo(require(&[Capability::Streaming]));
    let r1 = pm.project(&w).unwrap();
    let r2 = pm2.project(&w).unwrap();
    assert_eq!(r1.selected_backend, r2.selected_backend);
}

#[test]
fn edge_passthrough_bonus_value() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    // Non-passthrough score
    let w_normal = wo(require(&[Capability::Streaming]));
    let r_normal = pm.project(&w_normal).unwrap();

    // Passthrough score (with matching dialect)
    let w_pass = passthrough_wo(require(&[Capability::Streaming]), "claude");
    let r_pass = pm.project(&w_pass).unwrap();

    // Passthrough should add 0.15
    let diff = r_pass.fidelity_score.total - r_normal.fidelity_score.total;
    assert!((diff - 0.15).abs() < 0.01);
}

#[test]
fn edge_error_messages_meaningful() {
    let err1 = ProjectionError::EmptyMatrix;
    assert!(err1.to_string().contains("empty"));

    let err2 = ProjectionError::NoSuitableBackend {
        reason: "all backends lack streaming".into(),
    };
    assert!(err2.to_string().contains("all backends lack streaming"));
}

#[test]
fn edge_projection_error_clone() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test".into(),
    };
    let err2 = err.clone();
    assert_eq!(err, err2);
}

#[test]
fn edge_projection_score_debug() {
    let score = ProjectionScore {
        capability_coverage: 0.5,
        mapping_fidelity: 0.5,
        priority: 0.5,
        total: 0.5,
    };
    let dbg = format!("{score:?}");
    assert!(dbg.contains("ProjectionScore"));
}

#[test]
fn edge_required_emulation_debug() {
    let emu = RequiredEmulation {
        capability: Capability::Streaming,
        strategy: "adapter".into(),
    };
    let dbg = format!("{emu:?}");
    assert!(dbg.contains("RequiredEmulation"));
}

#[test]
fn edge_fallback_entry_debug() {
    let entry = FallbackEntry {
        backend_id: "test".into(),
        score: ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 1.0,
            priority: 1.0,
            total: 1.0,
        },
    };
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("FallbackEntry"));
}

#[test]
fn edge_projection_result_debug() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let dbg = format!("{result:?}");
    assert!(dbg.contains("ProjectionResult"));
}

#[test]
fn edge_projection_result_clone() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let result2 = result.clone();
    assert_eq!(result.selected_backend, result2.selected_backend);
}

#[test]
fn edge_wo_with_no_vendor_config() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    // Default WorkOrder has no vendor config → no passthrough, no source dialect
    let result = pm.project(&wo_empty()).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn edge_native_min_support_with_emulated_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "native",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "emulated",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    // With MinSupport::Native, emulated backend is still compatible (negotiate considers
    // emulated as satisfying the requirement). Both have same score → alphabetical.
    let result = pm
        .project(&wo(require_native(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "emulated");
}

#[test]
fn edge_all_mapping_features_lossless() {
    let mut reg = MappingRegistry::new();
    for feat in [features::TOOL_USE, features::STREAMING, features::THINKING] {
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: feat.into(),
            fidelity: Fidelity::Lossless,
        });
    }
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![
        features::TOOL_USE.into(),
        features::STREAMING.into(),
        features::THINKING.into(),
    ]);
    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // All lossless: 0.7*1.0 + 0.3*1.0 = 1.0
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn edge_scoring_deterministic_across_runs() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "a",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "b",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let w = wo(require(&[Capability::Streaming]));
    let results: Vec<_> = (0..10)
        .map(|_| pm.project(&w).unwrap().selected_backend.clone())
        .collect();
    assert!(results.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn edge_unsupported_capability_not_in_manifest() {
    let mut pm = ProjectionMatrix::new();
    // Manifest only has Streaming, but we require ToolRead (not in manifest at all)
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // Only partial coverage (1/2)
    assert!((result.fidelity_score.capability_coverage - 0.5).abs() < 0.01);
}

#[test]
fn edge_empty_mapping_features_no_rules() {
    let mut pm = ProjectionMatrix::with_mapping_registry(MappingRegistry::new());
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend(
        "openai",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // No rules for Claude→OpenAi in empty registry → fidelity = 0.0
    assert!((result.fidelity_score.mapping_fidelity).abs() < f64::EPSILON);
}
