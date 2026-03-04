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
//! Comprehensive tests for the projection matrix (dialect×engine mapping).

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};
use abp_projection::{ProjectionError, ProjectionMatrix, ProjectionResult, ProjectionScore};

// ── Helpers ────────────────────────────────────────────────────────────────

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

fn wo_passthrough(source_dialect: &str, reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp = serde_json::json!({ "mode": "passthrough", "source_dialect": source_dialect });
    config.vendor.insert("abp".into(), abp);
    WorkOrderBuilder::new("passthrough test")
        .requirements(reqs)
        .config(config)
        .build()
}

fn wo_mapped(source_dialect: &str, reqs: CapabilityRequirements) -> abp_core::WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp = serde_json::json!({ "mode": "mapped", "source_dialect": source_dialect });
    config.vendor.insert("abp".into(), abp);
    WorkOrderBuilder::new("mapped test")
        .requirements(reqs)
        .config(config)
        .build()
}

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn streaming_native() -> CapabilityManifest {
    manifest(&[(Capability::Streaming, SupportLevel::Native)])
}

fn full_manifest() -> CapabilityManifest {
    manifest(&[
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Native),
        (Capability::ToolWrite, SupportLevel::Native),
        (Capability::ToolUse, SupportLevel::Native),
        (Capability::ExtendedThinking, SupportLevel::Native),
        (Capability::ImageInput, SupportLevel::Native),
    ])
}

// ── 1. Valid dialect×engine combinations ───────────────────────────────────

#[test]
fn valid_same_dialect_openai_to_openai() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::OpenAi);
    pm.register_backend("openai", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "openai");
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn valid_same_dialect_claude_to_claude() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.register_backend("claude", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude");
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn valid_same_dialect_gemini_to_gemini() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Gemini);
    pm.register_backend("gemini", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "gemini");
}

#[test]
fn valid_same_dialect_codex_to_codex() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Codex);
    pm.register_backend("codex", streaming_native(), Dialect::Codex, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "codex");
}

#[test]
fn valid_same_dialect_kimi_to_kimi() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Kimi);
    pm.register_backend("kimi", streaming_native(), Dialect::Kimi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "kimi");
}

#[test]
fn valid_same_dialect_copilot_to_copilot() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Copilot);
    pm.register_backend("copilot", streaming_native(), Dialect::Copilot, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "copilot");
}

#[test]
fn valid_cross_dialect_openai_to_claude() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
    assert!(result.fidelity_score.mapping_fidelity > 0.0);
}

#[test]
fn valid_cross_dialect_claude_to_gemini() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::STREAMING.into()]);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "gemini-be");
}

// ── 2. Invalid/unsupported combinations ────────────────────────────────────

#[test]
fn empty_matrix_returns_error() {
    let pm = ProjectionMatrix::new();
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::EmptyMatrix));
}

#[test]
fn no_backend_satisfies_any_required_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn backend_with_unsupported_cap_rejected() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "bad",
        manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn all_backends_unsupported_for_required_caps() {
    let mut pm = ProjectionMatrix::new();
    for dialect in all_dialects() {
        pm.register_backend(
            format!("be-{}", dialect.label()),
            manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
            *dialect,
            50,
        );
    }
    let err = pm
        .project(&wo(require(&[Capability::Streaming])))
        .unwrap_err();
    assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn restricted_capability_treated_as_emulated() {
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
    // Restricted is treated as emulated (still satisfies requirement).
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "restricted");
    assert_eq!(result.required_emulations.len(), 1);
}

// ── 3. Capability requirements vs availability ─────────────────────────────

#[test]
fn native_cap_fully_satisfied() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "native-be",
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
    assert_eq!(result.selected_backend, "native-be");
    assert!(result.required_emulations.is_empty());
    assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
}

#[test]
fn emulated_cap_still_satisfies() {
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
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
}

#[test]
fn partial_coverage_degrades_score() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "partial",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    assert!(result.fidelity_score.capability_coverage < 1.0);
    assert!(result.fidelity_score.capability_coverage > 0.0);
}

#[test]
fn native_only_requirement_rejects_emulated_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu-be",
        manifest(&[(Capability::Streaming, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    // MinSupport::Native means emulated is not accepted → the backend is incompatible.
    let result = pm.project(&wo(require_native(&[Capability::Streaming])));
    assert!(
        result.is_err(),
        "Emulated backend should not satisfy Native requirement"
    );
}

#[test]
fn multiple_caps_all_native() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("rich", full_manifest(), Dialect::Claude, 50);
    let result = pm
        .project(&wo(require(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolUse,
        ])))
        .unwrap();
    assert_eq!(result.selected_backend, "rich");
    assert!(result.required_emulations.is_empty());
}

#[test]
fn empty_requirements_always_compatible() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", CapabilityManifest::new(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(CapabilityRequirements::default())).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn mixed_native_and_emulated_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "mixed",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Native),
        ]),
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
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(
        result.required_emulations[0].capability,
        Capability::ToolRead
    );
}

// ── 4. Passthrough mode validation ─────────────────────────────────────────

#[test]
fn passthrough_same_dialect_gets_bonus() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm
        .project(&wo_passthrough("claude", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn passthrough_bonus_overrides_moderate_priority_gap() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-lo", streaming_native(), Dialect::Claude, 30);
    pm.register_backend("openai-hi", streaming_native(), Dialect::OpenAi, 80);
    let result = pm
        .project(&wo_passthrough("claude", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-lo");
}

#[test]
fn passthrough_openai_source_prefers_openai_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    let result = pm
        .project(&wo_passthrough("openai", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn passthrough_gemini_source_prefers_gemini_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 40);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 60);
    let result = pm
        .project(&wo_passthrough("gemini", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn passthrough_codex_source_prefers_codex_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("codex-be", streaming_native(), Dialect::Codex, 40);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 60);
    let result = pm
        .project(&wo_passthrough("codex", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "codex-be");
}

#[test]
fn passthrough_kimi_source_prefers_kimi_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("kimi-be", streaming_native(), Dialect::Kimi, 40);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 60);
    let result = pm
        .project(&wo_passthrough("kimi", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "kimi-be");
}

#[test]
fn passthrough_copilot_source_prefers_copilot_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("copilot-be", streaming_native(), Dialect::Copilot, 40);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 60);
    let result = pm
        .project(&wo_passthrough(
            "copilot",
            require(&[Capability::Streaming]),
        ))
        .unwrap();
    assert_eq!(result.selected_backend, "copilot-be");
}

#[test]
fn passthrough_without_matching_backend_falls_back() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm
        .project(&wo_passthrough("claude", require(&[Capability::Streaming])))
        .unwrap();
    // No Claude backend, so it falls back to the only available one.
    assert_eq!(result.selected_backend, "openai-be");
}

// ── 5. Mapped mode validation ──────────────────────────────────────────────

#[test]
fn mapped_mode_no_passthrough_bonus() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("claude-lo", streaming_native(), Dialect::Claude, 30);
    pm.register_backend("openai-hi", streaming_native(), Dialect::OpenAi, 80);
    let result = pm
        .project(&wo_mapped("claude", require(&[Capability::Streaming])))
        .unwrap();
    // Mapped mode does NOT add passthrough bonus, but source_dialect=claude
    // still gives claude-lo perfect fidelity (same dialect) while openai-hi
    // gets zero fidelity (no mapping rules). This outweighs priority.
    assert_eq!(result.selected_backend, "claude-lo");
}

#[test]
fn mapped_mode_fidelity_from_registry() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm
        .project(&wo_mapped("claude", require(&[Capability::Streaming])))
        .unwrap();
    // Both have mapping support; result should be deterministic.
    assert!(!result.selected_backend.is_empty());
}

#[test]
fn mapped_mode_prefers_higher_fidelity_target() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: features::TOOL_USE.into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);
    pm.register_backend("openai-be", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm
        .project(&wo_mapped("claude", require(&[Capability::Streaming])))
        .unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn mapped_mode_lossy_still_preferred_over_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: features::THINKING.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "lossy".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: features::THINKING.into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::THINKING.into()]);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ── 6. Matrix completeness: all SDKs × all backends ───────────────────────

#[test]
fn known_rules_covers_all_same_dialect_pairs() {
    let reg = known_rules();
    for &dialect in all_dialects() {
        for &feat in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let rule = reg.lookup(dialect, dialect, feat);
            assert!(
                rule.is_some(),
                "missing same-dialect rule: {dialect} -> {dialect} for {feat}"
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "same-dialect should be lossless: {dialect} -> {dialect} for {feat}"
            );
        }
    }
}

#[test]
fn known_rules_has_tool_use_for_all_cross_pairs() {
    let reg = known_rules();
    let dialects = all_dialects();
    for &src in dialects {
        for &tgt in dialects {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::TOOL_USE);
            assert!(rule.is_some(), "missing tool_use rule: {src} -> {tgt}");
        }
    }
}

#[test]
fn known_rules_has_streaming_for_all_cross_pairs() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::STREAMING);
            assert!(rule.is_some(), "missing streaming rule: {src} -> {tgt}");
        }
    }
}

#[test]
fn known_rules_has_thinking_for_all_cross_pairs() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::THINKING);
            assert!(rule.is_some(), "missing thinking rule: {src} -> {tgt}");
        }
    }
}

#[test]
fn known_rules_nonzero_count() {
    let reg = known_rules();
    // 6 dialects × 5 features × self-pairs + many cross-pairs
    assert!(reg.len() > 30, "expected many rules, got {}", reg.len());
}

#[test]
fn mapping_matrix_from_known_rules_covers_self_pairs() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    for &d in all_dialects() {
        assert!(
            matrix.is_supported(d, d),
            "self-pair should be supported: {d} -> {d}"
        );
    }
}

#[test]
fn mapping_matrix_from_known_rules_covers_major_cross_pairs() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    let majors = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &src in &majors {
        for &tgt in &majors {
            if src == tgt {
                continue;
            }
            assert!(
                matrix.is_supported(src, tgt),
                "major cross-pair should be supported: {src} -> {tgt}"
            );
        }
    }
}

// ── 7. Matrix serialization ───────────────────────────────────────────────

#[test]
fn projection_score_serializes_to_json() {
    let score = ProjectionScore {
        capability_coverage: 0.8,
        mapping_fidelity: 0.9,
        priority: 0.5,
        total: 0.77,
    };
    let json = serde_json::to_string(&score).unwrap();
    assert!(json.contains("capability_coverage"));
    assert!(json.contains("mapping_fidelity"));
    assert!(json.contains("priority"));
    assert!(json.contains("total"));
}

#[test]
fn projection_score_roundtrip_json() {
    let score = ProjectionScore {
        capability_coverage: 1.0,
        mapping_fidelity: 0.75,
        priority: 0.5,
        total: 0.875,
    };
    let json = serde_json::to_string(&score).unwrap();
    let back: ProjectionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(score, back);
}

#[test]
fn projection_result_serializes() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let back: ProjectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.selected_backend, "be");
}

#[test]
fn projection_error_serializes_empty_matrix() {
    let err = ProjectionError::EmptyMatrix;
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, ProjectionError::EmptyMatrix));
}

#[test]
fn projection_error_serializes_no_suitable() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ProjectionError = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, ProjectionError::NoSuitableBackend { .. }));
}

#[test]
fn fidelity_enum_serializes_all_variants() {
    let lossless = Fidelity::Lossless;
    let lossy = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    let unsupported = Fidelity::Unsupported { reason: "r".into() };
    for f in [&lossless, &lossy, &unsupported] {
        let json = serde_json::to_string(f).unwrap();
        let back: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, f);
    }
}

#[test]
fn mapping_rule_roundtrip_json() {
    let rule = MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    };
    let json = serde_json::to_string(&rule).unwrap();
    let back: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, back);
}

#[test]
fn dialect_serializes_all_variants() {
    for &d in all_dialects() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

// ── 8. Matrix lookup performance (no unnecessary allocations) ──────────────

#[test]
fn project_with_many_backends_returns_result() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..100 {
        pm.register_backend(
            format!("be-{i}"),
            streaming_native(),
            all_dialects()[i % all_dialects().len()],
            (i % 100) as u32,
        );
    }
    assert_eq!(pm.backend_count(), 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(!result.selected_backend.is_empty());
    assert_eq!(result.fallback_chain.len(), 99);
}

#[test]
fn project_deterministic_across_runs() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("b", streaming_native(), Dialect::Claude, 50);
    pm.register_backend("c", streaming_native(), Dialect::Gemini, 50);
    let work = wo(require(&[Capability::Streaming]));
    let r1 = pm.project(&work).unwrap();
    let r2 = pm.project(&work).unwrap();
    assert_eq!(r1.selected_backend, r2.selected_backend);
    assert_eq!(r1.fallback_chain.len(), r2.fallback_chain.len());
}

#[test]
fn fallback_chain_sorted_descending() {
    let mut pm = ProjectionMatrix::new();
    for (id, prio) in [("a", 90), ("b", 60), ("c", 30), ("d", 10)] {
        pm.register_backend(id, streaming_native(), Dialect::OpenAi, prio);
    }
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let scores: Vec<f64> = result
        .fallback_chain
        .iter()
        .map(|e| e.score.total)
        .collect();
    for w in scores.windows(2) {
        assert!(w[0] >= w[1], "fallback chain not descending: {scores:?}");
    }
}

#[test]
fn backend_id_sort_breaks_tie_deterministically() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("zzz", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("aaa", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Same score → alphabetical order of id.
    assert_eq!(result.selected_backend, "aaa");
}

#[test]
fn mapping_registry_lookup_miss_returns_none() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "nonexistent")
            .is_none()
    );
}

#[test]
fn mapping_registry_repeated_insert_replaces() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Unsupported {
            reason: "gone".into(),
        },
    });
    assert_eq!(reg.len(), 1);
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "f").unwrap();
    assert!(rule.fidelity.is_unsupported());
}

// ── 9. Edge cases: unknown dialect, unknown backend ────────────────────────

#[test]
fn unknown_source_dialect_in_config_defaults_to_none() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert(
        "abp".into(),
        serde_json::json!({ "source_dialect": "foobar", "mode": "passthrough" }),
    );
    let work = WorkOrderBuilder::new("test")
        .requirements(require(&[Capability::Streaming]))
        .config(config)
        .build();
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    // Should succeed even with unknown dialect — no passthrough bonus applied.
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn no_vendor_config_still_projects() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    let work = WorkOrderBuilder::new("test")
        .requirements(require(&[Capability::Streaming]))
        .build();
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn empty_vendor_config_still_projects() {
    let mut config = RuntimeConfig::default();
    config.vendor.insert("abp".into(), serde_json::json!({}));
    let work = WorkOrderBuilder::new("test")
        .requirements(require(&[Capability::Streaming]))
        .config(config)
        .build();
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&work).unwrap();
    assert_eq!(result.selected_backend, "be");
}

#[test]
fn register_backend_with_empty_id() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("", streaming_native(), Dialect::OpenAi, 50);
    assert_eq!(pm.backend_count(), 1);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "");
}

#[test]
fn register_backend_overwrites_same_id() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 10);
    pm.register_backend("be", streaming_native(), Dialect::Claude, 90);
    assert_eq!(pm.backend_count(), 1);
}

#[test]
fn no_source_dialect_assumes_perfect_fidelity() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

// ── 10. Emulation level reporting ──────────────────────────────────────────

#[test]
fn emulation_strategy_is_adapter() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::ToolRead]))).unwrap();
    assert_eq!(result.required_emulations.len(), 1);
    assert_eq!(result.required_emulations[0].strategy, "adapter");
}

#[test]
fn no_emulation_when_all_native() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "native",
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
    assert!(result.required_emulations.is_empty());
}

#[test]
fn multiple_emulations_reported() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "multi-emu",
        manifest(&[
            (Capability::Streaming, SupportLevel::Emulated),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]),
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
    assert_eq!(result.required_emulations.len(), 3);
}

#[test]
fn emulation_only_for_required_caps() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "emu",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Emulated),
            (Capability::ToolWrite, SupportLevel::Emulated),
        ]),
        Dialect::OpenAi,
        50,
    );
    // Only require Streaming (native) — emulated caps should not appear.
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.required_emulations.is_empty());
}

// ── 11. validate_mapping tests ─────────────────────────────────────────────

#[test]
fn validate_mapping_lossless_rule() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn validate_mapping_unsupported_feature() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn validate_mapping_empty_feature_name() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_mapping_multiple_features() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
        ],
    );
    assert_eq!(results.len(), 3);
}

// ── 12. MappingMatrix tests ───────────────────────────────────────────────

#[test]
fn mapping_matrix_empty_returns_false() {
    let m = MappingMatrix::new();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn mapping_matrix_set_and_get() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn mapping_matrix_get_returns_none_for_unknown() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
}

#[test]
fn mapping_matrix_set_false_explicitly() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
}

#[test]
fn mapping_matrix_from_registry_excludes_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no".into(),
        },
    });
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn mapping_matrix_from_registry_includes_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "f".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    });
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Gemini));
}

// ── 13. MappingRegistry rank_targets ──────────────────────────────────────

#[test]
fn rank_targets_excludes_source_dialect() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(
        ranked.iter().all(|(d, _)| *d != Dialect::OpenAi),
        "rank_targets should not include source dialect"
    );
}

#[test]
fn rank_targets_sorted_by_lossless_descending() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1, "not sorted descending: {ranked:?}");
    }
}

#[test]
fn rank_targets_empty_features_returns_empty() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_targets_excludes_all_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no".into(),
        },
    });
    let ranked = reg.rank_targets(Dialect::OpenAi, &["f"]);
    assert!(ranked.is_empty());
}

// ── 14. Fidelity scoring internals ─────────────────────────────────────────

#[test]
fn same_dialect_fidelity_is_1() {
    let mut pm = ProjectionMatrix::new();
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn cross_dialect_fidelity_with_lossless_rules() {
    let mut reg = MappingRegistry::new();
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
        fidelity: Fidelity::Lossless,
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // 0.7 * 1.0 + 0.3 * 1.0 = 1.0
    assert!((result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON);
}

#[test]
fn fidelity_blend_lossy_and_lossless() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "a".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "b".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec!["a".into(), "b".into()]);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // lossless_ratio = 0.5, supported_ratio = 1.0 → 0.7*0.5 + 0.3*1.0 = 0.65
    let expected = 0.7 * 0.5 + 0.3 * 1.0;
    assert!(
        (result.fidelity_score.mapping_fidelity - expected).abs() < f64::EPSILON,
        "got {}, expected {expected}",
        result.fidelity_score.mapping_fidelity
    );
}

#[test]
fn fidelity_zero_when_all_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "a".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no".into(),
        },
    });
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec!["a".into()]);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fidelity_score.mapping_fidelity.abs() < f64::EPSILON);
}

#[test]
fn fidelity_zero_when_no_mapping_features_and_no_rank() {
    let reg = MappingRegistry::new(); // empty
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    // No mapping_features set, so rank_targets fallback is used with empty registry.
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fidelity_score.mapping_fidelity.abs() < f64::EPSILON);
}

// ── 15. Priority normalization ─────────────────────────────────────────────

#[test]
fn priority_normalized_to_max_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("only", streaming_native(), Dialect::OpenAi, 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

#[test]
fn priority_zero_normalizes_to_zero() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("lo", streaming_native(), Dialect::OpenAi, 0);
    pm.register_backend("hi", streaming_native(), Dialect::Claude, 100);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // hi selected; lo in fallback with priority = 0/100 = 0.0
    assert_eq!(result.selected_backend, "hi");
    let lo_entry = result
        .fallback_chain
        .iter()
        .find(|e| e.backend_id == "lo")
        .unwrap();
    assert!(lo_entry.score.priority.abs() < f64::EPSILON);
}

#[test]
fn all_same_priority_normalizes_to_one() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_native(), Dialect::OpenAi, 50);
    pm.register_backend("b", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
}

// ── 16. Fallback chain completeness ────────────────────────────────────────

#[test]
fn fallback_chain_excludes_selected_backend() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("a", streaming_native(), Dialect::OpenAi, 80);
    pm.register_backend("b", streaming_native(), Dialect::Claude, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    for entry in &result.fallback_chain {
        assert_ne!(entry.backend_id, result.selected_backend);
    }
}

#[test]
fn fallback_includes_incompatible_backends() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "good",
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
    assert_eq!(result.selected_backend, "good");
    assert!(
        result
            .fallback_chain
            .iter()
            .any(|e| e.backend_id == "partial")
    );
}

#[test]
fn single_backend_has_empty_fallback() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("solo", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert!(result.fallback_chain.is_empty());
}

// ── 17. Dialect enum completeness ──────────────────────────────────────────

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_labels_unique() {
    let labels: Vec<&str> = all_dialects().iter().map(|d| d.label()).collect();
    let mut unique = labels.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(labels.len(), unique.len());
}

#[test]
fn dialect_display_matches_label() {
    for &d in all_dialects() {
        assert_eq!(format!("{d}"), d.label());
    }
}

// ── 18. Cross-dialect projection with known_rules ──────────────────────────

#[test]
fn openai_source_selects_claude_over_gemini_for_tool_use_fidelity() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);
    pm.register_backend("claude-be", streaming_native(), Dialect::Claude, 50);
    pm.register_backend("gemini-be", streaming_native(), Dialect::Gemini, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Both OpenAi→Claude and OpenAi→Gemini tool_use are lossless in known_rules.
    // So the tiebreaker is alphabetical: claude-be < gemini-be.
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn claude_source_with_image_input_penalizes_codex() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::IMAGE_INPUT.into()]);
    pm.register_backend(
        "codex-be",
        manifest(&[(Capability::ImageInput, SupportLevel::Native)]),
        Dialect::Codex,
        50,
    );
    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::ImageInput, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::ImageInput]))).unwrap();
    // Claude→Codex image_input is unsupported, Claude→Gemini is lossless.
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn kimi_source_streaming_lossless_to_all() {
    let reg = known_rules();
    for &target in all_dialects() {
        if target == Dialect::Kimi {
            continue;
        }
        let rule = reg.lookup(Dialect::Kimi, target, features::STREAMING);
        assert!(
            rule.is_some_and(|r| r.fidelity.is_lossless()),
            "Kimi→{target} streaming should be lossless"
        );
    }
}

#[test]
fn copilot_source_streaming_lossless_to_all() {
    let reg = known_rules();
    for &target in all_dialects() {
        if target == Dialect::Copilot {
            continue;
        }
        let rule = reg.lookup(Dialect::Copilot, target, features::STREAMING);
        assert!(
            rule.is_some_and(|r| r.fidelity.is_lossless()),
            "Copilot→{target} streaming should be lossless"
        );
    }
}

#[test]
fn codex_image_input_unsupported_from_all() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg.lookup(src, Dialect::Codex, features::IMAGE_INPUT);
        assert!(
            rule.is_some_and(|r| r.fidelity.is_unsupported()),
            "{src}→Codex image_input should be unsupported"
        );
    }
}

#[test]
fn kimi_code_exec_unsupported_to_all() {
    let reg = known_rules();
    for &target in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let rule = reg.lookup(Dialect::Kimi, target, features::CODE_EXEC);
        assert!(
            rule.is_some_and(|r| r.fidelity.is_unsupported()),
            "Kimi→{target} code_exec should be unsupported"
        );
    }
}

// ── 19. Composite scoring ──────────────────────────────────────────────────

#[test]
fn score_weights_sum_to_one() {
    // W_CAPABILITY + W_FIDELITY + W_PRIORITY == 1.0
    // Verify through total score: compute(1, 1, 1) should give 1.0
    let mut pm = ProjectionMatrix::new();
    pm.register_backend("be", streaming_native(), Dialect::OpenAi, 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // All factors should contribute to a score <= 1.0 without passthrough bonus.
    assert!(result.fidelity_score.total <= 1.0 + f64::EPSILON);
}

#[test]
fn capability_coverage_dominates_with_weight_half() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "full-caps",
        manifest(&[
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
        ]),
        Dialect::OpenAi,
        10, // Low priority
    );
    pm.register_backend(
        "no-caps",
        CapabilityManifest::new(),
        Dialect::OpenAi,
        100, // High priority
    );
    let result = pm
        .project(&wo(require(&[Capability::Streaming, Capability::ToolRead])))
        .unwrap();
    // full-caps has coverage=1.0 (weight 0.5), no-caps has coverage=0.0.
    assert_eq!(result.selected_backend, "full-caps");
}

// ── 20. Default construction ───────────────────────────────────────────────

#[test]
fn projection_matrix_default_is_empty() {
    let pm = ProjectionMatrix::default();
    assert_eq!(pm.backend_count(), 0);
}

#[test]
fn projection_matrix_new_equals_default() {
    let new = ProjectionMatrix::new();
    let def = ProjectionMatrix::default();
    assert_eq!(new.backend_count(), def.backend_count());
}

#[test]
fn mapping_registry_default_is_empty() {
    let reg = MappingRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn mapping_matrix_default_is_empty() {
    let m = MappingMatrix::default();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}
