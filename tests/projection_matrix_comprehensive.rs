// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the projection matrix (dialect × engine routing).

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    MinSupport, RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{known_rules, features, Fidelity, MappingRegistry, MappingRule};
use abp_projection::{ProjectionError, ProjectionMatrix};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

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

fn empty_work_order() -> WorkOrder {
    WorkOrderBuilder::new("empty task").build()
}

fn passthrough_work_order(reqs: CapabilityRequirements, dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config =
        serde_json::json!({ "mode": "passthrough", "source_dialect": dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("passthrough task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn mapped_work_order(reqs: CapabilityRequirements, source_dialect: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    let abp_config = serde_json::json!({ "mode": "mapped", "source_dialect": source_dialect });
    config.vendor.insert("abp".into(), abp_config);
    WorkOrderBuilder::new("mapped task")
        .requirements(reqs)
        .config(config)
        .build()
}

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. ProjectionMatrix construction
// ═══════════════════════════════════════════════════════════════════════════
mod construction {
    use super::*;

    #[test]
    fn new_matrix_is_empty() {
        let pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn default_matrix_is_empty() {
        let pm = ProjectionMatrix::default();
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn with_mapping_registry_starts_empty_backends() {
        let reg = MappingRegistry::new();
        let pm = ProjectionMatrix::with_mapping_registry(reg);
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn with_known_rules_registry() {
        let reg = known_rules();
        let pm = ProjectionMatrix::with_mapping_registry(reg);
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn register_single_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "test-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn register_multiple_backends() {
        let mut pm = ProjectionMatrix::new();
        for i in 0..10 {
            pm.register_backend(
                format!("be-{i}"),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                50,
            );
        }
        assert_eq!(pm.backend_count(), 10);
    }

    #[test]
    fn register_overwrites_same_id() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "same-id",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "same-id",
            manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
            Dialect::Claude,
            90,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn register_backend_with_empty_manifest() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("empty-caps", CapabilityManifest::new(), Dialect::OpenAi, 50);
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn register_backend_with_zero_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "zero-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            0,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn register_backend_with_max_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "max-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Codex,
            100,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn set_source_dialect_does_not_panic() {
        let mut pm = ProjectionMatrix::new();
        for &d in all_dialects() {
            pm.set_source_dialect(d);
        }
    }

    #[test]
    fn set_mapping_features_does_not_panic() {
        let mut pm = ProjectionMatrix::new();
        pm.set_mapping_features(vec![
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
        ]);
    }

    #[test]
    fn register_one_backend_per_dialect() {
        let mut pm = ProjectionMatrix::new();
        for (i, &d) in all_dialects().iter().enumerate() {
            pm.register_backend(
                format!("be-{}", d.label()),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                (i as u32) * 10,
            );
        }
        assert_eq!(pm.backend_count(), all_dialects().len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Route lookup (dialect → engine)
// ═══════════════════════════════════════════════════════════════════════════
mod route_lookup {
    use super::*;

    #[test]
    fn single_backend_exact_cap_match() {
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
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "only");
        assert!(result.fallback_chain.is_empty());
    }

    #[test]
    fn selects_backend_with_full_coverage() {
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
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "full");
    }

    #[test]
    fn empty_requirements_selects_any() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be-a",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(CapabilityRequirements::default());
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be-a");
    }

    #[test]
    fn route_with_emulated_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu",
            manifest(&[(Capability::ToolRead, SupportLevel::Emulated)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "emu");
        assert_eq!(result.required_emulations.len(), 1);
        assert_eq!(
            result.required_emulations[0].capability,
            Capability::ToolRead
        );
        assert_eq!(result.required_emulations[0].strategy, "adapter");
    }

    #[test]
    fn route_prefers_compatible_over_incompatible() {
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
            "incompatible",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            90,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "compatible");
    }

    #[test]
    fn route_many_capabilities() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "rich",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
                (Capability::ToolEdit, SupportLevel::Native),
                (Capability::ToolBash, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "rich");
        assert!(result.required_emulations.is_empty());
    }

    #[test]
    fn backend_with_extra_caps_still_selected() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "over-provisioned",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
                (Capability::ToolBash, SupportLevel::Native),
                (Capability::Logprobs, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "over-provisioned");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Passthrough routing (same dialect → same engine)
// ═══════════════════════════════════════════════════════════════════════════
mod passthrough_routing {
    use super::*;

    #[test]
    fn passthrough_prefers_same_dialect_openai() {
        let mut pm = ProjectionMatrix::new();
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
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "openai");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn passthrough_prefers_same_dialect_claude() {
        let mut pm = ProjectionMatrix::new();
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
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "claude");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn passthrough_prefers_same_dialect_gemini() {
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
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "gemini");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "gemini-be");
    }

    #[test]
    fn passthrough_prefers_same_dialect_codex() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "codex-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Codex,
            50,
        );
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "codex");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "codex-be");
    }

    #[test]
    fn passthrough_prefers_same_dialect_kimi() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "kimi-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Kimi,
            50,
        );
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "kimi");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "kimi-be");
    }

    #[test]
    fn passthrough_prefers_same_dialect_copilot() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "copilot-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Copilot,
            50,
        );
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "copilot");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "copilot-be");
    }

    #[test]
    fn passthrough_bonus_overrides_lower_priority() {
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
        let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), "claude");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-low");
    }

    #[test]
    fn non_passthrough_does_not_apply_bonus() {
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
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-high");
    }

    #[test]
    fn passthrough_perfect_fidelity_for_same_dialect() {
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
        assert!(
            (result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON,
            "same-dialect passthrough should have perfect fidelity"
        );
    }

    #[test]
    fn passthrough_with_empty_reqs() {
        let mut pm = ProjectionMatrix::new();
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
        let wo = passthrough_work_order(CapabilityRequirements::default(), "claude");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Mapped routing (cross-dialect)
// ═══════════════════════════════════════════════════════════════════════════
mod mapped_routing {
    use super::*;

    #[test]
    fn mapped_mode_selects_best_fidelity() {
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
                warning: "partial".into(),
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

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-be");
    }

    #[test]
    fn mapped_cross_dialect_openai_to_claude() {
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

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
        assert!(result.fidelity_score.mapping_fidelity > 0.0);
    }

    #[test]
    fn mapped_cross_dialect_claude_to_openai() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);

        pm.register_backend(
            "openai-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-be");
        assert!(result.fidelity_score.mapping_fidelity > 0.0);
    }

    #[test]
    fn mapped_prefers_lossless_over_lossy_target() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "streaming".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "lossy".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec!["streaming".into()]);

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

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn mapped_unsupported_feature_yields_zero_fidelity() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "not supported".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec!["image_input".into()]);

        pm.register_backend(
            "codex-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Codex,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!(
            result.fidelity_score.mapping_fidelity < f64::EPSILON,
            "unsupported feature should yield zero fidelity"
        );
    }

    #[test]
    fn mapped_source_dialect_from_work_order_vendor_config() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec![features::STREAMING.into()]);

        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );

        let wo = mapped_work_order(require_caps(&[Capability::Streaming]), "openai");
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn mapped_no_source_dialect_assumes_perfect_fidelity() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be-a",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        // No source dialect set, no vendor config
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!(
            (result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON,
            "no source dialect should assume perfect fidelity"
        );
    }

    #[test]
    fn mapped_with_multiple_features_mixed_fidelity() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "streaming".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "partial".into(),
            },
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "thinking".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec![
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
        ]);

        pm.register_backend(
            "gemini-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // 1 lossless out of 3 total, 2 supported out of 3
        // fidelity = 0.7 * (1/3) + 0.3 * (2/3)
        let expected = 0.7 * (1.0 / 3.0) + 0.3 * (2.0 / 3.0);
        assert!(
            (result.fidelity_score.mapping_fidelity - expected).abs() < 0.01,
            "mixed fidelity: got {}, expected {}",
            result.fidelity_score.mapping_fidelity,
            expected
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Fidelity assessment
// ═══════════════════════════════════════════════════════════════════════════
mod fidelity_assessment {
    use super::*;

    #[test]
    fn same_dialect_always_perfect_fidelity() {
        for &d in all_dialects() {
            let mut pm = ProjectionMatrix::new();
            pm.set_source_dialect(d);
            pm.register_backend(
                "be",
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                50,
            );
            let wo = work_order(require_caps(&[Capability::Streaming]));
            let result = pm.project(&wo).unwrap();
            assert!(
                (result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON,
                "dialect {:?} same-dialect should have perfect fidelity",
                d
            );
        }
    }

    #[test]
    fn fidelity_with_all_lossless_features() {
        let mut reg = MappingRegistry::new();
        for &feat in &["tool_use", "streaming", "thinking"] {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Claude,
                feature: feat.into(),
                fidelity: Fidelity::Lossless,
            });
        }

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec![
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
        ]);

        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // All lossless: 0.7 * 1.0 + 0.3 * 1.0 = 1.0
        assert!(
            (result.fidelity_score.mapping_fidelity - 1.0).abs() < f64::EPSILON,
            "all lossless should give 1.0"
        );
    }

    #[test]
    fn fidelity_with_all_lossy_features() {
        let mut reg = MappingRegistry::new();
        for &feat in &["tool_use", "streaming"] {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Gemini,
                feature: feat.into(),
                fidelity: Fidelity::LossyLabeled {
                    warning: "lossy".into(),
                },
            });
        }

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

        pm.register_backend(
            "gemini-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Gemini,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // 0 lossless, 2 supported out of 2
        // fidelity = 0.7 * 0.0 + 0.3 * 1.0 = 0.3
        assert!(
            (result.fidelity_score.mapping_fidelity - 0.3).abs() < 0.01,
            "all lossy should give 0.3, got {}",
            result.fidelity_score.mapping_fidelity
        );
    }

    #[test]
    fn fidelity_with_all_unsupported_features() {
        let mut reg = MappingRegistry::new();
        for &feat in &["tool_use", "streaming"] {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Codex,
                feature: feat.into(),
                fidelity: Fidelity::Unsupported {
                    reason: "nope".into(),
                },
            });
        }

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);

        pm.register_backend(
            "codex-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Codex,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!(
            result.fidelity_score.mapping_fidelity < f64::EPSILON,
            "all unsupported should give 0.0"
        );
    }

    #[test]
    fn fidelity_constrains_selection_over_priority() {
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

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "openai-low");
    }

    #[test]
    fn fidelity_no_features_configured_uses_heuristic() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        // No set_mapping_features → uses rank_targets heuristic

        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Should get 0.8 from the heuristic
        assert!(
            (result.fidelity_score.mapping_fidelity - 0.8).abs() < 0.01,
            "heuristic fidelity should be 0.8, got {}",
            result.fidelity_score.mapping_fidelity
        );
    }

    #[test]
    fn fidelity_empty_mapping_registry_cross_dialect() {
        let mut pm = ProjectionMatrix::with_mapping_registry(MappingRegistry::new());
        pm.set_source_dialect(Dialect::OpenAi);

        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Empty registry + different dialect → 0 fidelity
        assert!(
            result.fidelity_score.mapping_fidelity < f64::EPSILON,
            "empty registry cross-dialect should be 0.0"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Route ranking
// ═══════════════════════════════════════════════════════════════════════════
mod route_ranking {
    use super::*;

    #[test]
    fn ranking_by_priority() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "mid",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "high");
    }

    #[test]
    fn fallback_chain_sorted_descending() {
        let mut pm = ProjectionMatrix::new();
        for (id, prio) in [("a", 90), ("b", 60), ("c", 30), ("d", 10)] {
            pm.register_backend(
                id,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                prio,
            );
        }
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let scores: Vec<f64> = result.fallback_chain.iter().map(|e| e.score.total).collect();
        for w in scores.windows(2) {
            assert!(w[0] >= w[1], "not sorted descending: {:?}", scores);
        }
    }

    #[test]
    fn fallback_excludes_selected() {
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
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        for entry in &result.fallback_chain {
            assert_ne!(entry.backend_id, result.selected_backend);
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
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "full");
        assert!(result.fallback_chain.iter().any(|e| e.backend_id == "partial"));
    }

    #[test]
    fn deterministic_id_sort_on_tie() {
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
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Same score → sorted by id ascending → "a-backend" is first
        assert_eq!(result.selected_backend, "a-backend");
    }

    #[test]
    fn priority_normalized_to_max() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "only",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            100,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
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
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "zero");
    }

    #[test]
    fn capability_coverage_impacts_ranking() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "three-caps",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "one-cap",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "three-caps");
        assert!(result.fidelity_score.capability_coverage > 0.9);
    }

    #[test]
    fn score_total_is_weighted_sum() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let s = &result.fidelity_score;
        let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
        assert!(
            (s.total - expected).abs() < 0.01,
            "total {} != weighted sum {}",
            s.total,
            expected
        );
    }

    #[test]
    fn many_backends_ranking_stability() {
        let mut pm = ProjectionMatrix::new();
        for i in 0..20 {
            pm.register_backend(
                format!("be-{i:02}"),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                i * 5,
            );
        }
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let r1 = pm.project(&wo).unwrap();
        let r2 = pm.project(&wo).unwrap();
        assert_eq!(r1.selected_backend, r2.selected_backend);
        assert_eq!(r1.fallback_chain.len(), r2.fallback_chain.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Matrix population with all dialect pairs
// ═══════════════════════════════════════════════════════════════════════════
mod all_dialect_pairs {
    use super::*;

    #[test]
    fn project_each_dialect_as_single_backend() {
        for &d in all_dialects() {
            let mut pm = ProjectionMatrix::new();
            pm.register_backend(
                format!("be-{}", d.label()),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                50,
            );
            let wo = work_order(require_caps(&[Capability::Streaming]));
            let result = pm.project(&wo).unwrap();
            assert_eq!(result.selected_backend, format!("be-{}", d.label()));
        }
    }

    #[test]
    fn all_dialects_compete_selects_highest_priority() {
        let mut pm = ProjectionMatrix::new();
        let dialects = all_dialects();
        for (i, &d) in dialects.iter().enumerate() {
            pm.register_backend(
                format!("be-{}", d.label()),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                (i as u32 + 1) * 10,
            );
        }
        assert_eq!(pm.backend_count(), dialects.len());
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // Last registered has highest priority
        let last = dialects.last().unwrap();
        assert_eq!(result.selected_backend, format!("be-{}", last.label()));
    }

    #[test]
    fn passthrough_for_each_dialect() {
        let dialect_names = ["openai", "claude", "gemini", "codex", "kimi", "copilot"];
        let dialects = all_dialects();

        for (&d, name) in dialects.iter().zip(dialect_names.iter()) {
            let mut pm = ProjectionMatrix::new();
            // Register two backends: matching and non-matching
            pm.register_backend(
                "matching",
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                50,
            );
            let other = if d == Dialect::OpenAi {
                Dialect::Claude
            } else {
                Dialect::OpenAi
            };
            pm.register_backend(
                "other",
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                other,
                50,
            );
            let wo = passthrough_work_order(require_caps(&[Capability::Streaming]), name);
            let result = pm.project(&wo).unwrap();
            assert_eq!(
                result.selected_backend, "matching",
                "passthrough for {} should select matching backend",
                name
            );
        }
    }

    #[test]
    fn known_rules_covers_all_same_dialect_pairs() {
        let reg = known_rules();
        for &d in all_dialects() {
            for &feat in &[
                features::TOOL_USE,
                features::STREAMING,
                features::THINKING,
                features::IMAGE_INPUT,
                features::CODE_EXEC,
            ] {
                let rule = reg.lookup(d, d, feat);
                assert!(
                    rule.is_some(),
                    "missing same-dialect rule: {:?} -> {:?} for {}",
                    d,
                    d,
                    feat
                );
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "same-dialect should be lossless: {:?} for {}",
                    d,
                    feat
                );
            }
        }
    }

    #[test]
    fn all_cross_dialect_streaming_has_rules() {
        let reg = known_rules();
        for &src in all_dialects() {
            for &tgt in all_dialects() {
                if src == tgt {
                    continue;
                }
                let rule = reg.lookup(src, tgt, features::STREAMING);
                assert!(
                    rule.is_some(),
                    "missing streaming rule: {:?} -> {:?}",
                    src,
                    tgt
                );
            }
        }
    }

    #[test]
    fn fidelity_scoring_all_dialect_pairs_with_known_rules() {
        let reg = known_rules();
        for &src in all_dialects() {
            let mut pm = ProjectionMatrix::with_mapping_registry(reg.clone());
            pm.set_source_dialect(src);
            pm.set_mapping_features(vec![features::STREAMING.into()]);

            for &tgt in all_dialects() {
                pm.register_backend(
                    format!("be-{}", tgt.label()),
                    manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                    tgt,
                    50,
                );
            }

            let wo = work_order(require_caps(&[Capability::Streaming]));
            let result = pm.project(&wo).unwrap();
            assert!(
                result.fidelity_score.mapping_fidelity >= 0.0,
                "fidelity should be non-negative for {:?}",
                src
            );
        }
    }

    #[test]
    fn matrix_with_all_six_dialects_has_correct_count() {
        let mut pm = ProjectionMatrix::new();
        for &d in all_dialects() {
            pm.register_backend(
                format!("be-{}", d.label()),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                50,
            );
        }
        assert_eq!(pm.backend_count(), 6);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Edge cases
// ═══════════════════════════════════════════════════════════════════════════
mod edge_cases {
    use super::*;

    #[test]
    fn empty_matrix_returns_error() {
        let pm = ProjectionMatrix::new();
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn empty_matrix_with_empty_reqs_returns_error() {
        let pm = ProjectionMatrix::new();
        let wo = empty_work_order();
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::EmptyMatrix));
    }

    #[test]
    fn no_suitable_backend_all_unsupported_caps() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "useless",
            manifest(&[(Capability::Logprobs, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn no_suitable_backend_empty_manifest() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("empty", CapabilityManifest::new(), Dialect::OpenAi, 50);
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let err = pm.project(&wo).unwrap_err();
        assert!(matches!(err, ProjectionError::NoSuitableBackend { .. }));
    }

    #[test]
    fn error_display_empty_matrix() {
        let err = ProjectionError::EmptyMatrix;
        let msg = err.to_string();
        assert!(msg.contains("empty"));
    }

    #[test]
    fn error_display_no_suitable() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "test reason".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("test reason"));
    }

    #[test]
    fn error_equality() {
        let e1 = ProjectionError::EmptyMatrix;
        let e2 = ProjectionError::EmptyMatrix;
        assert_eq!(e1, e2);

        let e3 = ProjectionError::NoSuitableBackend {
            reason: "a".into(),
        };
        let e4 = ProjectionError::NoSuitableBackend {
            reason: "a".into(),
        };
        assert_eq!(e3, e4);
    }

    #[test]
    fn error_inequality() {
        let e1 = ProjectionError::EmptyMatrix;
        let e2 = ProjectionError::NoSuitableBackend {
            reason: "x".into(),
        };
        assert_ne!(e1, e2);
    }

    #[test]
    fn single_backend_empty_requirements_succeeds() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("be", CapabilityManifest::new(), Dialect::OpenAi, 50);
        let wo = empty_work_order();
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be");
    }

    #[test]
    fn overwrite_backend_changes_dialect() {
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
            Dialect::Claude,
            90,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.required_emulations.len(), 1);
    }

    #[test]
    fn passthrough_with_unknown_dialect_in_config() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let mut config = RuntimeConfig::default();
        config.vendor.insert(
            "abp".into(),
            serde_json::json!({ "mode": "passthrough", "source_dialect": "unknown_dialect" }),
        );
        let wo = WorkOrderBuilder::new("test")
            .requirements(require_caps(&[Capability::Streaming]))
            .config(config)
            .build();
        // Should still succeed — unknown dialect means no passthrough bonus
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be");
    }

    #[test]
    fn work_order_without_vendor_config() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be");
    }

    #[test]
    fn all_backends_same_score_deterministic() {
        let mut pm = ProjectionMatrix::new();
        for name in ["alpha", "beta", "gamma"] {
            pm.register_backend(
                name,
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                Dialect::OpenAi,
                50,
            );
        }
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let r1 = pm.project(&wo).unwrap();
        let r2 = pm.project(&wo).unwrap();
        assert_eq!(r1.selected_backend, r2.selected_backend);
        assert_eq!(r1.selected_backend, "alpha");
    }

    #[test]
    fn emulation_strategy_is_adapter() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "emu-be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        for emu in &result.required_emulations {
            assert_eq!(emu.strategy, "adapter");
        }
    }

    #[test]
    fn native_caps_no_emulations() {
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
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert!(result.required_emulations.is_empty());
    }

    #[test]
    fn mixed_native_and_emulated() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "mixed-be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
                (Capability::ToolWrite, SupportLevel::Native),
            ]),
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.required_emulations.len(), 1);
        assert_eq!(
            result.required_emulations[0].capability,
            Capability::ToolRead
        );
    }

    #[test]
    fn score_fields_in_valid_range() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let s = &result.fidelity_score;
        assert!((0.0..=1.0).contains(&s.capability_coverage));
        assert!((0.0..=1.0).contains(&s.mapping_fidelity));
        assert!((0.0..=1.0).contains(&s.priority));
        assert!(s.total >= 0.0);
    }

    #[test]
    fn projection_result_serializes() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let json = serde_json::to_string(&result);
        assert!(json.is_ok());
    }

    #[test]
    fn projection_error_serializes() {
        let err = ProjectionError::EmptyMatrix;
        let json = serde_json::to_string(&err).unwrap();
        assert!(!json.is_empty());
        let err2 = ProjectionError::NoSuitableBackend {
            reason: "test".into(),
        };
        let json2 = serde_json::to_string(&err2).unwrap();
        assert!(!json2.is_empty());
    }

    #[test]
    fn projection_error_roundtrip() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "no match".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, deserialized);
    }

    #[test]
    fn large_backend_count() {
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
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "be-099");
        assert_eq!(result.fallback_chain.len(), 99);
    }

    #[test]
    fn multiple_emulated_capabilities_counted() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
                (Capability::ToolWrite, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.required_emulations.len(), 3);
    }

    #[test]
    fn capability_coverage_full_emulated_is_one() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert!(
            (result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON,
            "fully emulated should still be 1.0 coverage"
        );
    }

    #[test]
    fn empty_requirements_coverage_is_one() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("be", CapabilityManifest::new(), Dialect::OpenAi, 50);
        let wo = empty_work_order();
        let result = pm.project(&wo).unwrap();
        assert!(
            (result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON,
        );
    }

    #[test]
    fn set_source_dialect_overrides_work_order_config() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::Claude);
        pm.set_mapping_features(vec![features::STREAMING.into()]);

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

        // Work order says openai, but matrix source_dialect overrides to claude
        let wo = mapped_work_order(require_caps(&[Capability::Streaming]), "openai");
        let result = pm.project(&wo).unwrap();
        // Claude source → Claude backend has perfect fidelity
        assert_eq!(result.selected_backend, "claude-be");
    }

    #[test]
    fn partial_match_selected_when_no_compatible() {
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
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ]));
        // No backend is fully compatible; best partial match should be selected
        let result = pm.project(&wo).unwrap();
        assert!(!result.selected_backend.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional coverage: various capabilities
// ═══════════════════════════════════════════════════════════════════════════
mod capability_variety {
    use super::*;

    #[test]
    fn tool_bash_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "bash-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolBash]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "bash-be");
    }

    #[test]
    fn tool_glob_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "glob-be",
            manifest(&[(Capability::ToolGlob, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolGlob]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "glob-be");
    }

    #[test]
    fn tool_grep_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "grep-be",
            manifest(&[(Capability::ToolGrep, SupportLevel::Native)]),
            Dialect::Gemini,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolGrep]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "grep-be");
    }

    #[test]
    fn logprobs_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "logprobs-be",
            manifest(&[(Capability::Logprobs, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Logprobs]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "logprobs-be");
    }

    #[test]
    fn mcp_client_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "mcp-be",
            manifest(&[(Capability::McpClient, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[Capability::McpClient]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "mcp-be");
    }

    #[test]
    fn session_resume_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "session-be",
            manifest(&[(Capability::SessionResume, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::SessionResume]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "session-be");
    }

    #[test]
    fn checkpointing_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "cp-be",
            manifest(&[(Capability::Checkpointing, SupportLevel::Native)]),
            Dialect::Codex,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Checkpointing]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "cp-be");
    }

    #[test]
    fn structured_output_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "struct-be",
            manifest(&[(Capability::StructuredOutputJsonSchema, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::StructuredOutputJsonSchema]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "struct-be");
    }

    #[test]
    fn tool_edit_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "edit-be",
            manifest(&[(Capability::ToolEdit, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolEdit]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "edit-be");
    }

    #[test]
    fn web_search_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "search-be",
            manifest(&[(Capability::ToolWebSearch, SupportLevel::Native)]),
            Dialect::Gemini,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolWebSearch]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "search-be");
    }

    #[test]
    fn web_fetch_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "fetch-be",
            manifest(&[(Capability::ToolWebFetch, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolWebFetch]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "fetch-be");
    }

    #[test]
    fn ask_user_capability() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "ask-be",
            manifest(&[(Capability::ToolAskUser, SupportLevel::Native)]),
            Dialect::Copilot,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolAskUser]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "ask-be");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: known_rules + ProjectionMatrix
// ═══════════════════════════════════════════════════════════════════════════
mod known_rules_integration {
    use super::*;

    #[test]
    fn known_rules_registry_not_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn known_rules_with_full_matrix() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
        ]);

        for &d in all_dialects() {
            pm.register_backend(
                format!("be-{}", d.label()),
                manifest(&[(Capability::Streaming, SupportLevel::Native)]),
                d,
                50,
            );
        }

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // OpenAi → OpenAi has perfect fidelity → should be selected
        assert_eq!(result.selected_backend, "be-OpenAI");
    }

    #[test]
    fn known_rules_cross_dialect_non_zero_fidelity() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_source_dialect(Dialect::OpenAi);
        pm.set_mapping_features(vec![features::STREAMING.into()]);

        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );

        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert!(
            result.fidelity_score.mapping_fidelity > 0.0,
            "OpenAI → Claude streaming should have non-zero fidelity"
        );
    }

    #[test]
    fn known_rules_image_input_codex_unsupported() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_unsupported());
    }

    #[test]
    fn known_rules_tool_use_openai_claude_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }
}
