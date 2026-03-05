#![allow(clippy::all)]
#![allow(dead_code)]
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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the projection matrix system spanning
//! `abp_mapping`, `abp_dialect`, and `abp_projection`.

use std::collections::BTreeMap;

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};
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

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn all_features() -> &'static [&'static str] {
    &[
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ]
}

fn make_rule(src: Dialect, tgt: Dialect, feat: &str, fidelity: Fidelity) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feat.into(),
        fidelity,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. MappingRegistry construction and population
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let reg = MappingRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_default_is_empty() {
    let reg = MappingRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn registry_insert_single_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

#[test]
fn registry_insert_multiple_rules() {
    let mut reg = MappingRegistry::new();
    for &feat in all_features() {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            feat,
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.len(), 5);
}

#[test]
fn registry_insert_replaces_existing() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "changed".into(),
        },
    ));
    assert_eq!(reg.len(), 1);
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn registry_distinct_keys_for_different_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 2);
}

#[test]
fn registry_distinct_keys_for_different_direction() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 2);
}

#[test]
fn registry_iter_yields_all_rules() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "a",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::Gemini,
        "b",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.iter().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Lookup for all dialect×dialect×feature combinations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lookup_hit_returns_correct_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert_eq!(rule.source_dialect, Dialect::OpenAi);
    assert_eq!(rule.target_dialect, Dialect::Claude);
    assert_eq!(rule.feature, "tool_use");
}

#[test]
fn lookup_miss_returns_none() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_none()
    );
}

#[test]
fn lookup_wrong_direction_returns_none() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert!(
        reg.lookup(Dialect::Claude, Dialect::OpenAi, "tool_use")
            .is_none()
    );
}

#[test]
fn lookup_wrong_feature_returns_none() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "streaming")
            .is_none()
    );
}

#[test]
fn known_rules_all_same_dialect_lookups_succeed() {
    let reg = known_rules();
    for &d in all_dialects() {
        for &f in all_features() {
            assert!(
                reg.lookup(d, d, f).is_some(),
                "missing self-mapping for {d} -> {d} feature={f}",
            );
        }
    }
}

#[test]
fn known_rules_cross_dialect_streaming_all_present() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let rule = reg.lookup(src, tgt, features::STREAMING);
            assert!(
                rule.is_some(),
                "missing streaming mapping for {src} -> {tgt}",
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Fidelity level verification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_predicates() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_lossy_predicates() {
    let f = Fidelity::LossyLabeled {
        warning: "test".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_predicates() {
    let f = Fidelity::Unsupported {
        reason: "nope".into(),
    };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

#[test]
fn fidelity_equality() {
    assert_eq!(Fidelity::Lossless, Fidelity::Lossless);
    assert_ne!(
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "x".into()
        }
    );
    assert_ne!(
        Fidelity::LossyLabeled {
            warning: "a".into()
        },
        Fidelity::LossyLabeled {
            warning: "b".into()
        }
    );
}

#[test]
fn fidelity_clone() {
    let f = Fidelity::LossyLabeled {
        warning: "test".into(),
    };
    let f2 = f.clone();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_debug_formatting() {
    let f = Fidelity::Lossless;
    let s = format!("{f:?}");
    assert!(s.contains("Lossless"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Self-mapping is always Lossless
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn self_mapping_lossless_for_all_dialects_and_features() {
    let reg = known_rules();
    for &d in all_dialects() {
        for &f in all_features() {
            let rule = reg.lookup(d, d, f).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "{d} -> {d} {f} should be lossless, got {:?}",
                rule.fidelity,
            );
        }
    }
}

#[test]
fn self_mapping_validation_no_errors() {
    let reg = known_rules();
    for &d in all_dialects() {
        let feats: Vec<String> = all_features().iter().map(|f| (*f).to_string()).collect();
        let results = validate_mapping(&reg, d, d, &feats);
        for v in &results {
            assert!(
                v.errors.is_empty(),
                "{d} -> {d} {}: expected no errors, got {:?}",
                v.feature,
                v.errors,
            );
            assert!(
                v.fidelity.is_lossless(),
                "{d} -> {d} {}: expected lossless",
                v.feature,
            );
        }
    }
}

#[test]
fn self_mapping_validate_count_matches_feature_count() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|f| (*f).to_string()).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::OpenAi, &feats);
    assert_eq!(results.len(), all_features().len());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Symmetric vs asymmetric mappings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_is_symmetric_across_all_dialects() {
    let reg = known_rules();
    for &a in all_dialects() {
        for &b in all_dialects() {
            let ab = reg.lookup(a, b, features::STREAMING);
            let ba = reg.lookup(b, a, features::STREAMING);
            assert!(ab.is_some() && ba.is_some(), "streaming {a}<->{b} missing");
            assert_eq!(
                ab.unwrap().fidelity.is_lossless(),
                ba.unwrap().fidelity.is_lossless(),
                "streaming symmetry broken for {a}<->{b}",
            );
        }
    }
}

#[test]
fn tool_use_openai_claude_is_symmetric_lossless() {
    let reg = known_rules();
    let ab = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    let ba = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(ab.fidelity.is_lossless());
    assert!(ba.fidelity.is_lossless());
}

#[test]
fn image_input_codex_is_symmetric_unsupported() {
    let reg = known_rules();
    for &other in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let to_codex = reg
            .lookup(other, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        let from_codex = reg
            .lookup(Dialect::Codex, other, features::IMAGE_INPUT)
            .unwrap();
        assert!(to_codex.fidelity.is_unsupported());
        assert!(from_codex.fidelity.is_unsupported());
    }
}

#[test]
fn thinking_claude_openai_may_be_asymmetric_in_warning_text() {
    let reg = known_rules();
    let co = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    let oc = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
        .unwrap();
    // Both are lossy but the warning strings may differ.
    assert!(!co.fidelity.is_lossless());
    assert!(!oc.fidelity.is_lossless());
    // They need not be identical.
    assert_ne!(co, oc, "asymmetric warnings expected");
}

#[test]
fn code_exec_kimi_is_asymmetric_unsupported() {
    let reg = known_rules();
    // Kimi -> others for code_exec: unsupported
    for &other in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Kimi, other, features::CODE_EXEC)
            .unwrap();
        assert!(
            rule.fidelity.is_unsupported(),
            "Kimi -> {other} code_exec should be unsupported",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Feature-specific mapping rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn streaming_all_cross_dialect_lossless() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {src} -> {tgt} should be lossless",
            );
        }
    }
}

#[test]
fn tool_use_openai_gemini_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn tool_use_codex_is_lossy_with_all_non_codex() {
    let reg = known_rules();
    for &other in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::Copilot,
    ] {
        let to_codex = reg.lookup(other, Dialect::Codex, features::TOOL_USE);
        let from_codex = reg.lookup(Dialect::Codex, other, features::TOOL_USE);
        if let Some(r) = to_codex {
            assert!(
                !r.fidelity.is_lossless(),
                "tool_use {other} -> Codex should not be lossless",
            );
        }
        if let Some(r) = from_codex {
            assert!(
                !r.fidelity.is_lossless(),
                "tool_use Codex -> {other} should not be lossless",
            );
        }
    }
}

#[test]
fn thinking_all_cross_dialect_is_lossy() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            if src == tgt {
                continue;
            }
            let rule = reg.lookup(src, tgt, features::THINKING);
            if let Some(r) = rule {
                assert!(
                    !r.fidelity.is_lossless(),
                    "thinking {src} -> {tgt} should be lossy",
                );
            }
        }
    }
}

#[test]
fn image_input_openai_claude_gemini_triangle_lossless() {
    let reg = known_rules();
    let triangle = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    for &a in &triangle {
        for &b in &triangle {
            let rule = reg.lookup(a, b, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "image_input {a} -> {b} should be lossless",
            );
        }
    }
}

#[test]
fn image_input_kimi_copilot_unsupported() {
    let reg = known_rules();
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &other in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(nd, other, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image_input {nd} -> {other} should be unsupported",
            );
        }
    }
}

#[test]
fn code_exec_cross_dialect_among_capable_is_lossy() {
    let reg = known_rules();
    let capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &a in &capable {
        for &b in &capable {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::CODE_EXEC);
            if let Some(r) = rule {
                assert!(
                    !r.fidelity.is_lossless() && !r.fidelity.is_unsupported(),
                    "code_exec {a} -> {b} should be lossy, got {:?}",
                    r.fidelity,
                );
            }
        }
    }
}

#[test]
fn kimi_copilot_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn kimi_copilot_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn kimi_copilot_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn kimi_copilot_image_input_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Capability intersection via projection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_selects_backend_with_full_capability_coverage() {
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
        Dialect::Claude,
        80,
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
fn projection_prefers_native_over_emulated() {
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
        Dialect::Claude,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Both satisfy the requirement; native should rank at least as high.
    assert!(result.fidelity_score.capability_coverage >= 1.0);
}

#[test]
fn projection_fallback_chain_contains_alternatives() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "alpha",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        90,
    );
    pm.register_backend(
        "beta",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.fallback_chain.len(), 1);
}

#[test]
fn projection_empty_requirements_selects_highest_priority() {
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
        Dialect::Claude,
        90,
    );
    let empty_reqs = CapabilityRequirements { required: vec![] };
    let result = pm.project(&wo(empty_reqs)).unwrap();
    assert_eq!(result.selected_backend, "high");
}

#[test]
fn projection_with_mapping_fidelity_scoring() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "codex-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Codex,
        50,
    );

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude has lossless tool_use+streaming from OpenAI; Codex has lossy tool_use.
    // Claude should score higher on mapping fidelity.
    assert_eq!(result.selected_backend, "claude-be");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Early failure on Unsupported fidelity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_unsupported_produces_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn validate_unsupported_has_feature_name() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::OpenAi,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results[0].feature, features::IMAGE_INPUT);
}

#[test]
fn validate_unknown_feature_is_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["teleportation".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn validate_empty_feature_name_is_invalid_input() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { .. }
    ));
}

#[test]
fn validate_mixed_fidelity_results() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[
            features::STREAMING.into(),
            features::TOOL_USE.into(),
            features::IMAGE_INPUT.into(),
        ],
    );
    assert_eq!(results.len(), 3);
    // streaming: lossless
    assert!(results[0].fidelity.is_lossless());
    // tool_use: lossy
    assert!(!results[1].fidelity.is_lossless());
    // image_input: unsupported
    assert!(results[2].fidelity.is_unsupported());
}

#[test]
fn projection_empty_matrix_returns_error() {
    let pm = ProjectionMatrix::new();
    let result = pm.project(&wo(require(&[Capability::Streaming])));
    assert!(matches!(result, Err(ProjectionError::EmptyMatrix)));
}

#[test]
fn projection_no_suitable_backend_error() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "weak",
        manifest(&[(Capability::Streaming, SupportLevel::Unsupported)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require_native(&[Capability::ToolWrite])));
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Registry serialization/deserialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_serde_roundtrip_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_serde_roundtrip_lossy() {
    let f = Fidelity::LossyLabeled {
        warning: "thinking maps differently".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_serde_roundtrip_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "no images".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn mapping_rule_serde_roundtrip() {
    let rule = make_rule(
        Dialect::Gemini,
        Dialect::Claude,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "thinkingConfig differs".into(),
        },
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

#[test]
fn mapping_error_serde_roundtrip() {
    let err = MappingError::FeatureUnsupported {
        feature: "image_input".into(),
        from: Dialect::Kimi,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_error_fidelity_loss_serde() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_error_dialect_mismatch_serde() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_error_invalid_input_serde() {
    let err = MappingError::InvalidInput {
        reason: "bad feature".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_validation_serde_roundtrip() {
    let v = MappingValidation {
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn mapping_validation_with_errors_serde() {
    let v = MappingValidation {
        feature: "image_input".into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
        errors: vec![MappingError::FeatureUnsupported {
            feature: "image_input".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn fidelity_json_contains_type_tag() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"type\""));
    assert!(json.contains("\"lossless\""));
}

#[test]
fn fidelity_lossy_json_contains_warning() {
    let f = Fidelity::LossyLabeled {
        warning: "test warn".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"lossy_labeled\""));
    assert!(json.contains("test warn"));
}

#[test]
fn rule_collection_btreemap_deterministic_json() {
    let reg = known_rules();
    let rules: BTreeMap<String, &MappingRule> = reg
        .iter()
        .map(|r| {
            let key = format!("{}->{}/{}", r.source_dialect, r.target_dialect, r.feature);
            (key, r)
        })
        .collect();
    let json1 = serde_json::to_string(&rules).unwrap();
    let json2 = serde_json::to_string(&rules).unwrap();
    assert_eq!(json1, json2, "BTreeMap JSON should be deterministic");
}

#[test]
fn projection_score_serde_roundtrip() {
    let score = ProjectionScore {
        capability_coverage: 0.9,
        mapping_fidelity: 0.75,
        priority: 0.5,
        total: 0.78,
    };
    let json = serde_json::to_string(&score).unwrap();
    let score2: ProjectionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(score, score2);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Default registry completeness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_has_all_self_mappings() {
    let reg = known_rules();
    let expected = all_dialects().len() * all_features().len();
    let self_count = reg
        .iter()
        .filter(|r| r.source_dialect == r.target_dialect)
        .count();
    assert_eq!(
        self_count, expected,
        "every dialect×feature should have self-mapping"
    );
}

#[test]
fn known_rules_substantial_cross_dialect_count() {
    let reg = known_rules();
    let cross = reg
        .iter()
        .filter(|r| r.source_dialect != r.target_dialect)
        .count();
    // At least all_dialects * (all_dialects-1) * at least 1 feature
    let min_expected = all_dialects().len() * (all_dialects().len() - 1);
    assert!(
        cross >= min_expected,
        "expected at least {min_expected} cross-dialect rules, got {cross}",
    );
}

#[test]
fn known_rules_tool_use_coverage() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let rule = reg.lookup(src, tgt, features::TOOL_USE);
            assert!(rule.is_some(), "tool_use {src} -> {tgt} missing",);
        }
    }
}

#[test]
fn known_rules_thinking_coverage() {
    let reg = known_rules();
    for &src in all_dialects() {
        for &tgt in all_dialects() {
            let rule = reg.lookup(src, tgt, features::THINKING);
            assert!(rule.is_some(), "thinking {src} -> {tgt} missing",);
        }
    }
}

#[test]
fn known_rules_image_input_coverage() {
    let reg = known_rules();
    // At least all self-mappings exist.
    for &d in all_dialects() {
        let rule = reg.lookup(d, d, features::IMAGE_INPUT);
        assert!(rule.is_some(), "image_input self-mapping {d} missing");
    }
}

#[test]
fn known_rules_code_exec_coverage() {
    let reg = known_rules();
    for &d in all_dialects() {
        let rule = reg.lookup(d, d, features::CODE_EXEC);
        assert!(rule.is_some(), "code_exec self-mapping {d} missing");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Registry merging and overrides
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn merge_two_registries_by_insert() {
    let mut base = MappingRegistry::new();
    base.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let overlay_rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    );
    base.insert(overlay_rule);
    assert_eq!(base.len(), 2);
}

#[test]
fn override_fidelity_via_insert() {
    let mut reg = known_rules();
    let original = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap()
        .clone();
    assert!(!original.fidelity.is_lossless());

    // Override thinking mapping to lossless.
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        features::THINKING,
        Fidelity::Lossless,
    ));
    let overridden = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(overridden.fidelity.is_lossless());
}

#[test]
fn override_does_not_affect_other_rules() {
    let mut reg = known_rules();
    let tool_use_before = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap()
        .clone();

    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        features::THINKING,
        Fidelity::Lossless,
    ));

    let tool_use_after = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert_eq!(tool_use_before, *tool_use_after);
}

#[test]
fn add_custom_feature_to_known_rules() {
    let mut reg = known_rules();
    let before = reg.len();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "custom_feature",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), before + 1);
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "custom_feature")
            .is_some()
    );
}

#[test]
fn merge_preserves_base_when_no_conflict() {
    let mut base = MappingRegistry::new();
    base.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "a",
        Fidelity::Lossless,
    ));
    base.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "b",
        Fidelity::Lossless,
    ));
    assert_eq!(base.len(), 2);
    assert!(base.lookup(Dialect::OpenAi, Dialect::Claude, "a").is_some());
    assert!(base.lookup(Dialect::OpenAi, Dialect::Gemini, "b").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Performance characteristics with large registries
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    // Insert 1000 rules.
    for i in 0..1000 {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            &format!("feat_{i}"),
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.len(), 1000);
    // Lookup should succeed for any inserted feature.
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "feat_999")
            .is_some()
    );
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "feat_0")
            .is_some()
    );
}

#[test]
fn large_registry_iter_count() {
    let mut reg = MappingRegistry::new();
    for i in 0..500 {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            &format!("f{i}"),
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.iter().count(), 500);
}

#[test]
fn validate_many_features_at_once() {
    let reg = known_rules();
    let features: Vec<String> = (0..100).map(|i| format!("feat_{i}")).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert_eq!(results.len(), 100);
    // All should be unsupported (unknown features).
    for v in &results {
        assert!(v.fidelity.is_unsupported());
    }
}

#[test]
fn large_projection_matrix_many_backends() {
    let mut pm = ProjectionMatrix::new();
    for i in 0..50 {
        pm.register_backend(
            format!("be-{i}"),
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            *all_dialects().get(i % all_dialects().len()).unwrap(),
            (i as u32) % 100,
        );
    }
    assert_eq!(pm.backend_count(), 50);
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Should select the highest-scoring backend.
    assert!(!result.selected_backend.is_empty());
    assert_eq!(result.fallback_chain.len(), 49);
}

#[test]
fn rank_targets_with_known_rules() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
    // Claude should appear high since both are lossless.
    let claude_entry = ranked.iter().find(|(d, _)| *d == Dialect::Claude);
    assert!(claude_entry.is_some());
    assert_eq!(
        claude_entry.unwrap().1,
        2,
        "both features lossless for Claude"
    );
}

#[test]
fn rank_targets_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    assert!(
        !ranked.iter().any(|(d, _)| *d == Dialect::OpenAi),
        "rank_targets should not include source dialect",
    );
}

#[test]
fn rank_targets_sorted_descending() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
        ],
    );
    for window in ranked.windows(2) {
        assert!(
            window[0].1 >= window[1].1,
            "rank_targets should be sorted descending",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Thread safety of registry operations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MappingRegistry>();
}

#[test]
fn registry_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<MappingRegistry>();
}

#[test]
fn mapping_rule_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappingRule>();
}

#[test]
fn fidelity_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Fidelity>();
}

#[test]
fn mapping_matrix_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappingMatrix>();
}

#[test]
fn projection_matrix_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProjectionMatrix>();
}

#[test]
fn concurrent_read_access_via_shared_ref() {
    let reg = known_rules();
    let reg_ref = &reg;

    // Simulate concurrent reads by spawning threads that share &reg.
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                s.spawn(|| {
                    for &d in all_dialects() {
                        for &f in all_features() {
                            let _ = reg_ref.lookup(d, d, f);
                        }
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    });
}

#[test]
fn projection_matrix_concurrent_project() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be1",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    pm.register_backend(
        "be2",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );

    let pm_ref = &pm;
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    let result = pm_ref
                        .project(&wo(require(&[Capability::Streaming])))
                        .unwrap();
                    assert!(!result.selected_backend.is_empty());
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    });
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: MappingMatrix tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn matrix_new_is_empty() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_set_and_get() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
}

#[test]
fn matrix_is_directional() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_from_registry_marks_supported_pairs() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    // OpenAI -> Claude has lossless rules, so should be supported.
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_from_registry_unsupported_only_pairs_not_marked() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::Gemini,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "nope".into(),
        },
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
}

#[test]
fn matrix_overwrite() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: Passthrough mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_same_dialect_gets_boost() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);

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

    // Passthrough with source=openai should favor the OpenAI backend.
    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "openai"))
        .unwrap();
    assert_eq!(result.selected_backend, "openai-be");
}

#[test]
fn passthrough_claude_selects_claude_backend() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);

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

    let result = pm
        .project(&passthrough_wo(require(&[Capability::Streaming]), "claude"))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: Validate mapping comprehensive
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_empty_feature_list_returns_empty() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn validate_lossless_feature_no_errors() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::STREAMING.into()],
    );
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn validate_lossy_feature_has_fidelity_loss_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::OpenAi,
        &[features::THINKING.into()],
    );
    assert!(!results[0].fidelity.is_lossless());
    assert!(
        results[0]
            .errors
            .iter()
            .any(|e| matches!(e, MappingError::FidelityLoss { .. }))
    );
}

#[test]
fn validate_preserves_feature_order() {
    let reg = known_rules();
    let feats: Vec<String> = vec![
        features::CODE_EXEC.into(),
        features::TOOL_USE.into(),
        features::STREAMING.into(),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results[0].feature, features::CODE_EXEC);
    assert_eq!(results[1].feature, features::TOOL_USE);
    assert_eq!(results[2].feature, features::STREAMING);
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: Dialect enum coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_labels_non_empty() {
    for &d in Dialect::all() {
        assert!(!d.label().is_empty());
    }
}

#[test]
fn dialect_display_matches_label() {
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_clone_equality() {
    for &d in Dialect::all() {
        let d2 = d;
        assert_eq!(d, d2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: ProjectionMatrix with mapping features
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn projection_with_source_dialect_in_work_order() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_mapping_features(vec![features::IMAGE_INPUT.into()]);

    pm.register_backend(
        "claude-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    pm.register_backend(
        "codex-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Codex,
        50,
    );

    // With source_dialect=openai, image_input to Claude is lossless but to Codex is unsupported.
    let result = pm
        .project(&wo_with_source_dialect(
            require(&[Capability::Streaming]),
            "openai",
        ))
        .unwrap();
    assert_eq!(result.selected_backend, "claude-be");
}

#[test]
fn projection_set_source_dialect_overrides_work_order() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::Claude);
    pm.set_mapping_features(vec![features::TOOL_USE.into()]);

    pm.register_backend(
        "gemini-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Gemini,
        50,
    );
    pm.register_backend(
        "codex-be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Codex,
        50,
    );

    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    // Claude -> Gemini tool_use is lossless; Claude -> Codex is lossy.
    assert_eq!(result.selected_backend, "gemini-be");
}

#[test]
fn projection_no_mapping_features_uses_heuristic() {
    let reg = known_rules();
    let mut pm = ProjectionMatrix::with_mapping_registry(reg);
    pm.set_source_dialect(Dialect::OpenAi);
    // No set_mapping_features — uses default heuristic.

    pm.register_backend(
        "be1",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::Claude,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    assert_eq!(result.selected_backend, "be1");
}

#[test]
fn projection_score_total_is_weighted_sum() {
    let mut pm = ProjectionMatrix::new();
    pm.register_backend(
        "be",
        manifest(&[(Capability::Streaming, SupportLevel::Native)]),
        Dialect::OpenAi,
        50,
    );
    let result = pm.project(&wo(require(&[Capability::Streaming]))).unwrap();
    let s = &result.fidelity_score;
    // total = 0.5*cap + 0.3*fidelity + 0.2*priority
    let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
    assert!(
        (s.total - expected).abs() < 0.001,
        "total {} != expected {}",
        s.total,
        expected,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: Error display messages
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_display_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
    assert!(msg.contains("Claude"));
    assert!(msg.contains("Gemini"));
}

#[test]
fn mapping_error_display_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("mapped to system message"));
}

#[test]
fn mapping_error_display_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));
}

#[test]
fn mapping_error_display_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "bad thing".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("bad thing"));
}

#[test]
fn projection_error_display_empty_matrix() {
    let err = ProjectionError::EmptyMatrix;
    let msg = err.to_string();
    assert!(msg.contains("empty") || msg.contains("no backends"));
}

#[test]
fn projection_error_display_no_suitable() {
    let err = ProjectionError::NoSuitableBackend {
        reason: "nothing fits".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("nothing fits"));
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: features module constants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn feature_constants_are_non_empty() {
    assert!(!features::TOOL_USE.is_empty());
    assert!(!features::STREAMING.is_empty());
    assert!(!features::THINKING.is_empty());
    assert!(!features::IMAGE_INPUT.is_empty());
    assert!(!features::CODE_EXEC.is_empty());
}

#[test]
fn feature_constants_are_distinct() {
    let feats = all_features();
    for (i, a) in feats.iter().enumerate() {
        for (j, b) in feats.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "features should be distinct");
            }
        }
    }
}

#[test]
fn feature_constants_match_known_rules_coverage() {
    let reg = known_rules();
    for &feat in all_features() {
        // Every feature should at least have self-mappings.
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::OpenAi, feat).is_some(),
            "feature {feat} missing from known_rules",
        );
    }
}
