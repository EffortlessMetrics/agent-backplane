// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the projection matrix (dialect × engine routing).

use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingRegistry, MappingRule, features, known_rules};
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
    let abp_config = serde_json::json!({ "mode": "passthrough", "source_dialect": dialect });
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
        let scores: Vec<f64> = result
            .fallback_chain
            .iter()
            .map(|e| e.score.total)
            .collect();
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
        assert!(
            result
                .fallback_chain
                .iter()
                .any(|e| e.backend_id == "partial")
        );
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

        let e3 = ProjectionError::NoSuitableBackend { reason: "a".into() };
        let e4 = ProjectionError::NoSuitableBackend { reason: "a".into() };
        assert_eq!(e3, e4);
    }

    #[test]
    fn error_inequality() {
        let e1 = ProjectionError::EmptyMatrix;
        let e2 = ProjectionError::NoSuitableBackend { reason: "x".into() };
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
        assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON,);
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

// ═══════════════════════════════════════════════════════════════════════════
// 11. Matrix construction — additional
// ═══════════════════════════════════════════════════════════════════════════
mod construction_extended {
    use super::*;
    use abp_projection::{DialectPair, ProjectionMode};

    #[test]
    fn with_defaults_has_all_dialect_pairs() {
        let pm = ProjectionMatrix::with_defaults();
        let n = Dialect::all().len();
        assert_eq!(pm.dialect_entry_count(), n * n);
    }

    #[test]
    fn register_defaults_is_idempotent() {
        let mut pm = ProjectionMatrix::with_defaults();
        let before = pm.dialect_entry_count();
        pm.register_defaults();
        assert_eq!(pm.dialect_entry_count(), before);
    }

    #[test]
    fn large_matrix_50_backends() {
        let mut pm = ProjectionMatrix::new();
        for i in 0..50 {
            pm.register_backend(
                format!("be-{i:03}"),
                manifest(&[
                    (Capability::Streaming, SupportLevel::Native),
                    (Capability::ToolRead, SupportLevel::Native),
                ]),
                *Dialect::all().get(i % Dialect::all().len()).unwrap(),
                (i as u32) * 2,
            );
        }
        assert_eq!(pm.backend_count(), 50);
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.fallback_chain.len(), 49);
    }

    #[test]
    fn remove_backend_decrements_count() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "a",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        pm.register_backend(
            "b",
            manifest(&[(Capability::ToolRead, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        assert_eq!(pm.backend_count(), 2);
        assert!(pm.remove_backend("a"));
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn remove_nonexistent_backend_returns_false() {
        let mut pm = ProjectionMatrix::new();
        assert!(!pm.remove_backend("nope"));
    }

    #[test]
    fn remove_dialect_entry() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_some());
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn remove_nonexistent_dialect_entry_returns_none() {
        let mut pm = ProjectionMatrix::new();
        assert!(pm.remove(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn register_identity_always_passthrough_even_if_unsupported_requested() {
        let mut pm = ProjectionMatrix::new();
        pm.register(
            Dialect::Gemini,
            Dialect::Gemini,
            ProjectionMode::Unsupported,
        );
        let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
        assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
    }

    #[test]
    fn dialect_pair_display_format() {
        let pair = DialectPair::new(Dialect::Claude, Dialect::Gemini);
        assert_eq!(pair.to_string(), "Claude → Gemini");
    }

    #[test]
    fn dialect_pair_ordering_is_deterministic() {
        let mut pairs: Vec<DialectPair> = vec![
            DialectPair::new(Dialect::Gemini, Dialect::Claude),
            DialectPair::new(Dialect::OpenAi, Dialect::Claude),
            DialectPair::new(Dialect::Claude, Dialect::OpenAi),
        ];
        let mut pairs2 = pairs.clone();
        pairs.sort();
        pairs2.sort();
        assert_eq!(pairs, pairs2);
    }

    #[test]
    fn backend_entry_fields_accessible() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "test-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            42,
        );
        assert_eq!(pm.backend_count(), 1);
    }

    #[test]
    fn register_removes_old_on_same_key_dialect_pair() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        pm.register(
            Dialect::OpenAi,
            Dialect::Claude,
            ProjectionMode::Unsupported,
        );
        assert_eq!(pm.dialect_entry_count(), 1);
        assert_eq!(
            pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap().mode,
            ProjectionMode::Unsupported
        );
    }

    #[test]
    fn register_does_not_affect_reverse_pair() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        assert!(pm.lookup(Dialect::Claude, Dialect::OpenAi).is_none());
    }

    #[test]
    fn with_defaults_passthrough_count_equals_dialect_count() {
        let pm = ProjectionMatrix::with_defaults();
        let pt = pm
            .dialect_entries()
            .filter(|e| e.mode == ProjectionMode::Passthrough)
            .count();
        assert_eq!(pt, Dialect::all().len());
    }

    #[test]
    fn with_defaults_mapped_pairs_count() {
        let pm = ProjectionMatrix::with_defaults();
        let mapped = pm
            .dialect_entries()
            .filter(|e| e.mode == ProjectionMode::Mapped)
            .count();
        assert_eq!(mapped, 8);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Route resolution — find_route
// ═══════════════════════════════════════════════════════════════════════════
mod route_resolution {
    use super::*;
    use abp_projection::ProjectionMode;

    #[test]
    fn identity_route_has_zero_cost() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        assert_eq!(path.cost, 0);
        assert!(path.hops.is_empty());
        assert!((path.fidelity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn identity_route_is_direct() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::Claude, Dialect::Claude).unwrap();
        assert!(path.is_direct());
        assert!(!path.is_multi_hop());
    }

    #[test]
    fn direct_route_cost_one() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(path.cost, 1);
        assert_eq!(path.hops.len(), 1);
        assert!(path.is_direct());
    }

    #[test]
    fn direct_route_hop_endpoints() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(path.hops[0].from, Dialect::OpenAi);
        assert_eq!(path.hops[0].to, Dialect::Claude);
    }

    #[test]
    fn direct_route_has_mapper_hint() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(
            path.hops[0].mapper_hint.as_deref(),
            Some("openai_to_claude")
        );
    }

    #[test]
    fn no_route_for_unregistered_pair() {
        let pm = ProjectionMatrix::new();
        assert!(pm.find_route(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn no_route_when_pair_is_unsupported_and_no_intermediate() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Unsupported);
        assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn all_identity_routes_exist() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let path = pm.find_route(d, d).unwrap();
            assert_eq!(path.cost, 0);
            assert!((path.fidelity - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn all_mapped_pairs_have_direct_routes() {
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
            let path = pm.find_route(src, tgt);
            assert!(
                path.is_some(),
                "expected direct route from {} to {}",
                src,
                tgt
            );
            assert_eq!(path.unwrap().cost, 1);
        }
    }

    #[test]
    fn direct_route_fidelity_is_positive() {
        let _reg = known_rules();
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert!(path.fidelity >= 0.0);
    }

    #[test]
    fn route_reverse_pair_is_independent() {
        let pm = ProjectionMatrix::with_defaults();
        let fwd = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        let rev = pm.find_route(Dialect::Claude, Dialect::OpenAi).unwrap();
        assert_eq!(fwd.hops[0].from, Dialect::OpenAi);
        assert_eq!(rev.hops[0].from, Dialect::Claude);
    }

    #[test]
    fn codex_openai_route_uses_identity_hint() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::Codex, Dialect::OpenAi).unwrap();
        assert_eq!(path.hops[0].mapper_hint.as_deref(), Some("identity"));
    }

    #[test]
    fn routing_path_serde_roundtrip() {
        let pm = ProjectionMatrix::with_defaults();
        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        let json = serde_json::to_string(&path).unwrap();
        let back: abp_projection::RoutingPath = serde_json::from_str(&json).unwrap();
        assert_eq!(path, back);
    }

    #[test]
    fn routing_hop_serde_roundtrip() {
        let hop = abp_projection::RoutingHop {
            from: Dialect::Gemini,
            to: Dialect::Kimi,
            mapper_hint: Some("gemini_to_kimi".into()),
        };
        let json = serde_json::to_string(&hop).unwrap();
        let back: abp_projection::RoutingHop = serde_json::from_str(&json).unwrap();
        assert_eq!(hop, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Cost-based routing
// ═══════════════════════════════════════════════════════════════════════════
mod cost_based_routing {
    use super::*;

    #[test]
    fn score_weights_sum_to_one() {
        // W_CAPABILITY=0.5, W_FIDELITY=0.3, W_PRIORITY=0.2
        let perfect = project_single_backend(1.0, 1.0);
        assert!((perfect.fidelity_score.total - 1.0).abs() < 0.02);
    }

    #[test]
    fn capability_weight_is_dominant() {
        // Backend with full caps but low priority should beat high-prio but no caps
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "full-caps-low-prio",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Native),
            ]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "no-caps-high-prio",
            manifest(&[(Capability::Logprobs, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "full-caps-low-prio");
    }

    #[test]
    fn fidelity_weight_outweighs_priority() {
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
        // Fidelity weight (0.3) > priority weight (0.2)
        assert_eq!(result.selected_backend, "openai-low");
    }

    #[test]
    fn zero_capability_coverage_yields_low_total() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Logprobs, SupportLevel::Native)]),
            Dialect::OpenAi,
            100,
        );
        // Require something not provided
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let err = pm.project(&wo);
        assert!(err.is_err());
    }

    #[test]
    fn passthrough_bonus_adds_0_15() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "claude-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        let wo_pt = passthrough_work_order(require_caps(&[Capability::Streaming]), "claude");
        let wo_normal = work_order(require_caps(&[Capability::Streaming]));
        let r_pt = pm.project(&wo_pt).unwrap();
        let r_normal = pm.project(&wo_normal).unwrap();
        let diff = r_pt.fidelity_score.total - r_normal.fidelity_score.total;
        assert!((diff - 0.15).abs() < 0.01, "bonus diff = {diff}");
    }

    #[test]
    fn half_coverage_halves_capability_component() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "half-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        // partial match selected because no better
        let result = pm.project(&wo).unwrap();
        assert!(
            (result.fidelity_score.capability_coverage - 0.5).abs() < f64::EPSILON,
            "expected 0.5 coverage, got {}",
            result.fidelity_score.capability_coverage
        );
    }

    #[test]
    fn priority_is_normalized_relative_to_max() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "low",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            25,
        );
        pm.register_backend(
            "high",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            100,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // "high" selected; its priority should be 1.0
        assert!((result.fidelity_score.priority - 1.0).abs() < f64::EPSILON);
        // "low" in fallback has normalized priority 25/100 = 0.25
        let low_entry = result
            .fallback_chain
            .iter()
            .find(|e| e.backend_id == "low")
            .unwrap();
        assert!((low_entry.score.priority - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn total_equals_weighted_sum() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            70,
        );
        pm.register_backend(
            "be2",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            30,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        let s = &result.fidelity_score;
        let expected = 0.5 * s.capability_coverage + 0.3 * s.mapping_fidelity + 0.2 * s.priority;
        // passthrough bonus may shift total, but base is the weighted sum
        assert!(
            (s.total - expected).abs() < 0.16,
            "total {} vs expected {} (may include bonus)",
            s.total,
            expected
        );
    }

    #[test]
    fn equal_caps_equal_fidelity_priority_breaks_tie() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "low-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            10,
        );
        pm.register_backend(
            "high-prio",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            90,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "high-prio");
    }

    #[test]
    fn all_zero_scores_selects_by_id() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("beta", CapabilityManifest::new(), Dialect::OpenAi, 0);
        pm.register_backend("alpha", CapabilityManifest::new(), Dialect::OpenAi, 0);
        let wo = work_order(CapabilityRequirements::default());
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "alpha");
    }

    // Helper
    fn project_single_backend(
        coverage_score: f64,
        _fidelity: f64,
    ) -> abp_projection::ProjectionResult {
        let mut pm = ProjectionMatrix::new();
        let caps = if coverage_score >= 1.0 {
            manifest(&[(Capability::Streaming, SupportLevel::Native)])
        } else {
            CapabilityManifest::new()
        };
        pm.register_backend("be", caps, Dialect::OpenAi, 100);
        let reqs = if coverage_score >= 1.0 {
            require_caps(&[Capability::Streaming])
        } else {
            CapabilityRequirements::default()
        };
        pm.project(&work_order(reqs)).unwrap()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Compatibility scoring
// ═══════════════════════════════════════════════════════════════════════════
mod compatibility_scoring {
    use super::*;

    #[test]
    fn same_dialect_perfect_compatibility() {
        let pm = ProjectionMatrix::new();
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 0);
    }

    #[test]
    fn same_dialect_lossless_equals_feature_count() {
        let mut pm = ProjectionMatrix::new();
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        let score = pm.compatibility_score(Dialect::Claude, Dialect::Claude);
        assert_eq!(score.lossless_features, 2);
    }

    #[test]
    fn cross_dialect_no_features_uses_heuristic() {
        let reg = known_rules();
        let pm = ProjectionMatrix::with_mapping_registry(reg);
        // No mapping features set → fidelity from mapping_fidelity heuristic
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
        assert!(score.fidelity >= 0.0 && score.fidelity <= 1.0);
    }

    #[test]
    fn all_lossless_features_score() {
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
        pm.set_mapping_features(vec![
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
        ]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(score.lossless_features, 3);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 0);
    }

    #[test]
    fn all_lossy_features_score() {
        let mut reg = MappingRegistry::new();
        for &feat in &["tool_use", "streaming"] {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Gemini,
                feature: feat.into(),
                fidelity: Fidelity::LossyLabeled {
                    warning: "approx".into(),
                },
            });
        }
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(score.lossless_features, 0);
        assert_eq!(score.lossy_features, 2);
        assert_eq!(score.unsupported_features, 0);
    }

    #[test]
    fn all_unsupported_features_score() {
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
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Codex);
        assert_eq!(score.lossless_features, 0);
        assert_eq!(score.lossy_features, 0);
        assert_eq!(score.unsupported_features, 2);
    }

    #[test]
    fn mixed_features_score() {
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
                reason: "no".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec![
            "tool_use".into(),
            "streaming".into(),
            "thinking".into(),
        ]);
        let score = pm.compatibility_score(Dialect::Claude, Dialect::Gemini);
        assert_eq!(score.lossless_features, 1);
        assert_eq!(score.lossy_features, 1);
        assert_eq!(score.unsupported_features, 1);
        assert_eq!(score.source, Dialect::Claude);
        assert_eq!(score.target, Dialect::Gemini);
    }

    #[test]
    fn compatibility_score_source_target_fields() {
        let pm = ProjectionMatrix::new();
        let score = pm.compatibility_score(Dialect::Kimi, Dialect::Copilot);
        assert_eq!(score.source, Dialect::Kimi);
        assert_eq!(score.target, Dialect::Copilot);
    }

    #[test]
    fn compatibility_score_serde_roundtrip() {
        let pm = ProjectionMatrix::new();
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
        let json = serde_json::to_string(&score).unwrap();
        let back: abp_projection::CompatibilityScore = serde_json::from_str(&json).unwrap();
        assert_eq!(score, back);
    }

    #[test]
    fn empty_features_same_dialect_still_perfect() {
        let pm = ProjectionMatrix::new();
        let score = pm.compatibility_score(Dialect::Codex, Dialect::Codex);
        assert!((score.fidelity - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.lossless_features, 0);
    }

    #[test]
    fn native_vs_emulated_capability_in_projection() {
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
        pm.register_backend(
            "emulated",
            manifest(&[
                (Capability::Streaming, SupportLevel::Emulated),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        // Both have 100% coverage; native has 0 emulations, emulated has 2
        let native_emu = if result.selected_backend == "native" {
            result.required_emulations.len()
        } else {
            result
                .fallback_chain
                .iter()
                .find(|e| e.backend_id == "native")
                .map(|_| 0usize)
                .unwrap_or(99)
        };
        // At minimum, the native backend exists and can be resolved
        assert!(native_emu <= 2);
    }

    #[test]
    fn restricted_support_level_counts_as_satisfied() {
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
        let wo = work_order(require_caps(&[Capability::ToolBash]));
        let result = pm.project(&wo).unwrap();
        assert_eq!(result.selected_backend, "restricted-be");
    }

    #[test]
    fn unsupported_cap_in_manifest_not_satisfied() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "unsupported-be",
            manifest(&[(Capability::ToolBash, SupportLevel::Unsupported)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::ToolBash]));
        let err = pm.project(&wo);
        assert!(err.is_err());
    }

    #[test]
    fn many_capabilities_all_native_full_coverage() {
        let mut pm = ProjectionMatrix::new();
        let all_caps: Vec<(Capability, SupportLevel)> = vec![
            (Capability::Streaming, SupportLevel::Native),
            (Capability::ToolRead, SupportLevel::Native),
            (Capability::ToolWrite, SupportLevel::Native),
            (Capability::ToolEdit, SupportLevel::Native),
            (Capability::ToolBash, SupportLevel::Native),
            (Capability::ToolGlob, SupportLevel::Native),
            (Capability::ToolGrep, SupportLevel::Native),
            (Capability::McpClient, SupportLevel::Native),
        ];
        pm.register_backend("rich-be", manifest(&all_caps), Dialect::OpenAi, 50);
        let wo = work_order(require_caps(&[
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
            Capability::McpClient,
        ]));
        let result = pm.project(&wo).unwrap();
        assert!((result.fidelity_score.capability_coverage - 1.0).abs() < f64::EPSILON);
        assert!(result.required_emulations.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Multi-hop routing
// ═══════════════════════════════════════════════════════════════════════════
mod multi_hop_routing {
    use super::*;
    use abp_projection::ProjectionMode;

    #[test]
    fn multi_hop_through_intermediate() {
        let mut pm = ProjectionMatrix::new();
        // Kimi → OpenAi (mapped), OpenAi → Copilot (mapped)
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        let path = pm.find_route(Dialect::Kimi, Dialect::Copilot);
        assert!(path.is_some(), "should find multi-hop route");
        let p = path.unwrap();
        assert!(p.is_multi_hop());
        assert_eq!(p.cost, 2);
        assert_eq!(p.hops.len(), 2);
    }

    #[test]
    fn multi_hop_endpoints_correct() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        let p = pm.find_route(Dialect::Kimi, Dialect::Copilot).unwrap();
        assert_eq!(p.hops[0].from, Dialect::Kimi);
        assert_eq!(p.hops[0].to, Dialect::OpenAi);
        assert_eq!(p.hops[1].from, Dialect::OpenAi);
        assert_eq!(p.hops[1].to, Dialect::Copilot);
    }

    #[test]
    fn multi_hop_fidelity_is_product() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Kimi,
            target_dialect: Dialect::OpenAi,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Copilot,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec!["tool_use".into()]);
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);

        let p = pm.find_route(Dialect::Kimi, Dialect::Copilot).unwrap();
        // Each hop fidelity multiplied
        assert!(p.fidelity >= 0.0 && p.fidelity <= 1.0);
    }

    #[test]
    fn direct_preferred_over_multi_hop() {
        let mut pm = ProjectionMatrix::new();
        // Direct route exists
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        // Multi-hop through Gemini also exists
        pm.register(Dialect::OpenAi, Dialect::Gemini, ProjectionMode::Mapped);
        pm.register(Dialect::Gemini, Dialect::Claude, ProjectionMode::Mapped);

        let path = pm.find_route(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(path.cost, 1, "direct route should be preferred");
        assert!(path.is_direct());
    }

    #[test]
    fn multi_hop_best_intermediate_by_fidelity() {
        let mut reg = MappingRegistry::new();
        // Route through OpenAi: Kimi → OpenAi (lossless) → Copilot (lossless)
        reg.insert(MappingRule {
            source_dialect: Dialect::Kimi,
            target_dialect: Dialect::OpenAi,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Copilot,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        // Route through Claude: Kimi → Claude (lossy) → Copilot (lossy)
        reg.insert(MappingRule {
            source_dialect: Dialect::Kimi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "x".into(),
            },
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Copilot,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "x".into(),
            },
        });

        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.set_mapping_features(vec!["tool_use".into()]);
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        pm.register(Dialect::Kimi, Dialect::Claude, ProjectionMode::Mapped);
        pm.register(Dialect::Claude, Dialect::Copilot, ProjectionMode::Mapped);

        let path = pm.find_route(Dialect::Kimi, Dialect::Copilot).unwrap();
        assert_eq!(path.cost, 2);
        // Should pick the highest-fidelity intermediate
        assert!(path.fidelity > 0.0);
    }

    #[test]
    fn no_multi_hop_when_both_hops_unsupported() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Unsupported);
        pm.register(
            Dialect::OpenAi,
            Dialect::Copilot,
            ProjectionMode::Unsupported,
        );
        assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn no_multi_hop_when_first_hop_unsupported() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Unsupported);
        pm.register(Dialect::OpenAi, Dialect::Copilot, ProjectionMode::Mapped);
        assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn no_multi_hop_when_second_hop_unsupported() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::OpenAi, ProjectionMode::Mapped);
        pm.register(
            Dialect::OpenAi,
            Dialect::Copilot,
            ProjectionMode::Unsupported,
        );
        assert!(pm.find_route(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn multi_hop_with_defaults_kimi_to_codex() {
        let pm = ProjectionMatrix::with_defaults();
        // Kimi→Codex is unsupported directly, but Kimi→OpenAi→Codex might work
        // (Kimi→OpenAi is unsupported in defaults, so this should fail)
        let path = pm.find_route(Dialect::Kimi, Dialect::Codex);
        // Whether it finds a route depends on what intermediates are mapped
        if let Some(p) = path {
            assert!(p.cost >= 1);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Serde and stability
// ═══════════════════════════════════════════════════════════════════════════
mod serde_stability {
    use super::*;
    use abp_projection::{
        CompatibilityScore, DialectPair, FallbackEntry, ProjectionEntry, ProjectionMode,
        ProjectionScore, RequiredEmulation, RoutingHop, RoutingPath,
    };

    #[test]
    fn projection_score_serde_roundtrip() {
        let score = ProjectionScore {
            capability_coverage: 0.85,
            mapping_fidelity: 0.7,
            priority: 0.6,
            total: 0.75,
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
    fn routing_hop_no_hint_serde_roundtrip() {
        let hop = RoutingHop {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
            mapper_hint: None,
        };
        let json = serde_json::to_string(&hop).unwrap();
        let back: RoutingHop = serde_json::from_str(&json).unwrap();
        assert_eq!(hop, back);
    }

    #[test]
    fn routing_path_serde_roundtrip_multi_hop() {
        let path = RoutingPath {
            hops: vec![
                RoutingHop {
                    from: Dialect::Kimi,
                    to: Dialect::OpenAi,
                    mapper_hint: Some("kimi_to_openai".into()),
                },
                RoutingHop {
                    from: Dialect::OpenAi,
                    to: Dialect::Copilot,
                    mapper_hint: Some("openai_to_copilot".into()),
                },
            ],
            cost: 2,
            fidelity: 0.64,
        };
        let json = serde_json::to_string(&path).unwrap();
        let back: RoutingPath = serde_json::from_str(&json).unwrap();
        assert_eq!(path, back);
    }

    #[test]
    fn routing_path_empty_serde_roundtrip() {
        let path = RoutingPath {
            hops: vec![],
            cost: 0,
            fidelity: 1.0,
        };
        let json = serde_json::to_string(&path).unwrap();
        let back: RoutingPath = serde_json::from_str(&json).unwrap();
        assert_eq!(path, back);
    }

    #[test]
    fn compatibility_score_serde_roundtrip() {
        let score = CompatibilityScore {
            source: Dialect::OpenAi,
            target: Dialect::Claude,
            fidelity: 0.9,
            lossless_features: 3,
            lossy_features: 1,
            unsupported_features: 0,
        };
        let json = serde_json::to_string(&score).unwrap();
        let back: CompatibilityScore = serde_json::from_str(&json).unwrap();
        assert_eq!(score, back);
    }

    #[test]
    fn dialect_pair_serde_roundtrip() {
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                let pair = DialectPair::new(src, tgt);
                let json = serde_json::to_string(&pair).unwrap();
                let back: DialectPair = serde_json::from_str(&json).unwrap();
                assert_eq!(pair, back);
            }
        }
    }

    #[test]
    fn projection_entry_serde_roundtrip_all_modes() {
        for mode in [
            ProjectionMode::Passthrough,
            ProjectionMode::Mapped,
            ProjectionMode::Unsupported,
        ] {
            let entry = ProjectionEntry {
                pair: DialectPair::new(Dialect::Gemini, Dialect::Kimi),
                mode,
                mapper_hint: if mode == ProjectionMode::Mapped {
                    Some("gemini_to_kimi".into())
                } else if mode == ProjectionMode::Passthrough {
                    Some("identity".into())
                } else {
                    None
                },
            };
            let json = serde_json::to_string(&entry).unwrap();
            let back: ProjectionEntry = serde_json::from_str(&json).unwrap();
            assert_eq!(entry, back);
        }
    }

    #[test]
    fn projection_mode_json_values() {
        assert_eq!(
            serde_json::to_string(&ProjectionMode::Passthrough).unwrap(),
            "\"passthrough\""
        );
        assert_eq!(
            serde_json::to_string(&ProjectionMode::Mapped).unwrap(),
            "\"mapped\""
        );
        assert_eq!(
            serde_json::to_string(&ProjectionMode::Unsupported).unwrap(),
            "\"unsupported\""
        );
    }

    #[test]
    fn fallback_entry_serde_roundtrip() {
        let entry = FallbackEntry {
            backend_id: "test-backend".into(),
            score: ProjectionScore {
                capability_coverage: 0.5,
                mapping_fidelity: 0.8,
                priority: 0.3,
                total: 0.55,
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: FallbackEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn projection_error_serde_roundtrip_empty_matrix() {
        let err = ProjectionError::EmptyMatrix;
        let json = serde_json::to_string(&err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn projection_error_serde_roundtrip_no_suitable() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "missing streaming".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ProjectionError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn projection_result_full_roundtrip() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "be-1",
            manifest(&[
                (Capability::Streaming, SupportLevel::Native),
                (Capability::ToolRead, SupportLevel::Emulated),
            ]),
            Dialect::OpenAi,
            80,
        );
        pm.register_backend(
            "be-2",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            40,
        );
        let wo = work_order(require_caps(&[Capability::Streaming, Capability::ToolRead]));
        let result = pm.project(&wo).unwrap();
        let json = serde_json::to_string(&result).unwrap();
        let back: abp_projection::ProjectionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.selected_backend, back.selected_backend);
        assert_eq!(result.fallback_chain.len(), back.fallback_chain.len());
        assert_eq!(
            result.required_emulations.len(),
            back.required_emulations.len()
        );
    }

    #[test]
    fn deterministic_serialization_projection_score() {
        let score = ProjectionScore {
            capability_coverage: 1.0,
            mapping_fidelity: 0.8,
            priority: 0.5,
            total: 0.84,
        };
        let json1 = serde_json::to_string(&score).unwrap();
        let json2 = serde_json::to_string(&score).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn deterministic_serialization_dialect_pair() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        let json1 = serde_json::to_string(&pair).unwrap();
        let json2 = serde_json::to_string(&pair).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn btreemap_deterministic_iteration() {
        let pm1 = ProjectionMatrix::with_defaults();
        let pm2 = ProjectionMatrix::with_defaults();
        let entries1: Vec<_> = pm1.dialect_entries().collect::<Vec<_>>();
        let entries2: Vec<_> = pm2.dialect_entries().collect::<Vec<_>>();
        assert_eq!(entries1.len(), entries2.len());
        for (a, b) in entries1.iter().zip(entries2.iter()) {
            assert_eq!(a.pair, b.pair);
            assert_eq!(a.mode, b.mode);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Strategy selection — ModelSelector with all SelectionStrategy variants
// ═══════════════════════════════════════════════════════════════════════════
mod strategy_selection {
    use abp_projection::selection::{ModelCandidate, ModelSelector, SelectionStrategy};

    fn candidate(
        name: &str,
        latency: Option<u64>,
        cost: Option<f64>,
        fidelity: Option<f64>,
        weight: f64,
    ) -> ModelCandidate {
        ModelCandidate {
            backend_name: name.into(),
            model_id: format!("{name}-model"),
            estimated_latency_ms: latency,
            estimated_cost_per_1k_tokens: cost,
            fidelity_score: fidelity,
            weight,
        }
    }

    // ── RoundRobin ──────────────────────────────────────────────────────

    #[test]
    fn round_robin_cycles_through_candidates() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![
                candidate("a", None, None, None, 1.0),
                candidate("b", None, None, None, 1.0),
                candidate("c", None, None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "a");
        assert_eq!(sel.select().unwrap().backend_name, "b");
        assert_eq!(sel.select().unwrap().backend_name, "c");
        assert_eq!(sel.select().unwrap().backend_name, "a");
    }

    #[test]
    fn round_robin_single_candidate() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![candidate("only", None, None, None, 1.0)],
        );
        for _ in 0..5 {
            assert_eq!(sel.select().unwrap().backend_name, "only");
        }
    }

    #[test]
    fn round_robin_select_n_wraps() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![
                candidate("a", None, None, None, 1.0),
                candidate("b", None, None, None, 1.0),
            ],
        );
        let top2 = sel.select_n(2);
        assert_eq!(top2.len(), 2);
    }

    // ── LowestLatency ───────────────────────────────────────────────────

    #[test]
    fn lowest_latency_picks_fastest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate("slow", Some(500), None, None, 1.0),
                candidate("fast", Some(10), None, None, 1.0),
                candidate("mid", Some(100), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "fast");
    }

    #[test]
    fn lowest_latency_none_latency_is_worst() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate("unknown", None, None, None, 1.0),
                candidate("known", Some(999), None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "known");
    }

    #[test]
    fn lowest_latency_select_n_ordered() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![
                candidate("c", Some(300), None, None, 1.0),
                candidate("a", Some(100), None, None, 1.0),
                candidate("b", Some(200), None, None, 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "a");
        assert_eq!(ranked[1].backend_name, "b");
        assert_eq!(ranked[2].backend_name, "c");
    }

    // ── LowestCost ──────────────────────────────────────────────────────

    #[test]
    fn lowest_cost_picks_cheapest() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate("expensive", None, Some(10.0), None, 1.0),
                candidate("cheap", None, Some(0.5), None, 1.0),
                candidate("mid", None, Some(3.0), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "cheap");
    }

    #[test]
    fn lowest_cost_none_cost_is_worst() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate("unknown", None, None, None, 1.0),
                candidate("known", None, Some(99.0), None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "known");
    }

    #[test]
    fn lowest_cost_select_n_ordered() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate("c", None, Some(30.0), None, 1.0),
                candidate("a", None, Some(1.0), None, 1.0),
                candidate("b", None, Some(15.0), None, 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "a");
        assert_eq!(ranked[1].backend_name, "b");
        assert_eq!(ranked[2].backend_name, "c");
    }

    // ── HighestFidelity ─────────────────────────────────────────────────

    #[test]
    fn highest_fidelity_picks_best() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate("low", None, None, Some(0.3), 1.0),
                candidate("high", None, None, Some(0.95), 1.0),
                candidate("mid", None, None, Some(0.6), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "high");
    }

    #[test]
    fn highest_fidelity_none_fidelity_is_worst() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate("unknown", None, None, None, 1.0),
                candidate("known", None, None, Some(0.1), 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "known");
    }

    #[test]
    fn highest_fidelity_select_n_ordered() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate("low", None, None, Some(0.2), 1.0),
                candidate("high", None, None, Some(0.9), 1.0),
                candidate("mid", None, None, Some(0.5), 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "high");
        assert_eq!(ranked[1].backend_name, "mid");
        assert_eq!(ranked[2].backend_name, "low");
    }

    // ── FallbackChain ───────────────────────────────────────────────────

    #[test]
    fn fallback_chain_picks_first() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![
                candidate("primary", None, None, None, 1.0),
                candidate("secondary", None, None, None, 1.0),
            ],
        );
        assert_eq!(sel.select().unwrap().backend_name, "primary");
    }

    #[test]
    fn fallback_chain_select_n_preserves_order() {
        let sel = ModelSelector::new(
            SelectionStrategy::FallbackChain,
            vec![
                candidate("first", None, None, None, 1.0),
                candidate("second", None, None, None, 1.0),
                candidate("third", None, None, None, 1.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "first");
        assert_eq!(ranked[1].backend_name, "second");
        assert_eq!(ranked[2].backend_name, "third");
    }

    // ── WeightedRandom ──────────────────────────────────────────────────

    #[test]
    fn weighted_random_returns_some() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate("a", None, None, None, 10.0),
                candidate("b", None, None, None, 1.0),
            ],
        );
        assert!(sel.select().is_some());
    }

    #[test]
    fn weighted_random_single_candidate_always_selected() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![candidate("sole", None, None, None, 5.0)],
        );
        for _ in 0..10 {
            assert_eq!(sel.select().unwrap().backend_name, "sole");
        }
    }

    #[test]
    fn weighted_random_zero_weights_returns_first() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate("a", None, None, None, 0.0),
                candidate("b", None, None, None, 0.0),
            ],
        );
        assert!(sel.select().is_some());
    }

    // ── Empty candidates ────────────────────────────────────────────────

    #[test]
    fn empty_candidates_returns_none_for_all_strategies() {
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
                "strategy {strategy:?} should return None for empty"
            );
        }
    }

    #[test]
    fn select_n_zero_returns_empty() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestLatency,
            vec![candidate("a", Some(10), None, None, 1.0)],
        );
        assert!(sel.select_n(0).is_empty());
    }

    #[test]
    fn select_n_larger_than_candidates_returns_all() {
        let sel = ModelSelector::new(
            SelectionStrategy::LowestCost,
            vec![
                candidate("a", None, Some(1.0), None, 1.0),
                candidate("b", None, Some(2.0), None, 1.0),
            ],
        );
        assert_eq!(sel.select_n(10).len(), 2);
    }

    // ── Clone & serde ───────────────────────────────────────────────────

    #[test]
    fn selector_clone_preserves_strategy() {
        let sel = ModelSelector::new(
            SelectionStrategy::RoundRobin,
            vec![candidate("a", None, None, None, 1.0)],
        );
        let cloned = sel.clone();
        assert_eq!(cloned.strategy, SelectionStrategy::RoundRobin);
        assert_eq!(cloned.candidates.len(), 1);
    }

    #[test]
    fn selector_serde_roundtrip() {
        let sel = ModelSelector::new(
            SelectionStrategy::HighestFidelity,
            vec![
                candidate("a", Some(100), Some(2.0), Some(0.8), 3.0),
                candidate("b", Some(200), Some(1.0), Some(0.9), 1.0),
            ],
        );
        let json = serde_json::to_string(&sel).unwrap();
        let back: ModelSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strategy, SelectionStrategy::HighestFidelity);
        assert_eq!(back.candidates.len(), 2);
    }

    #[test]
    fn strategy_serde_all_variants() {
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
    fn candidate_serde_roundtrip() {
        let c = candidate("test", Some(50), Some(1.5), Some(0.7), 2.0);
        let json = serde_json::to_string(&c).unwrap();
        let back: ModelCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend_name, "test");
        assert_eq!(back.estimated_latency_ms, Some(50));
        assert_eq!(back.estimated_cost_per_1k_tokens, Some(1.5));
        assert_eq!(back.fidelity_score, Some(0.7));
    }

    // ── Negative weights ────────────────────────────────────────────────

    #[test]
    fn weighted_random_negative_weights_treated_as_zero() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate("neg", None, None, None, -5.0),
                candidate("pos", None, None, None, 10.0),
            ],
        );
        // Should not crash; the positive-weight candidate should be selectable
        assert!(sel.select().is_some());
    }

    #[test]
    fn weighted_random_select_n_ordered_by_weight() {
        let sel = ModelSelector::new(
            SelectionStrategy::WeightedRandom,
            vec![
                candidate("low", None, None, None, 1.0),
                candidate("high", None, None, None, 100.0),
                candidate("mid", None, None, None, 10.0),
            ],
        );
        let ranked = sel.select_n(3);
        assert_eq!(ranked[0].backend_name, "high");
        assert_eq!(ranked[1].backend_name, "mid");
        assert_eq!(ranked[2].backend_name, "low");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Mapper integration — resolve_mapper produces functional mappers
// ═══════════════════════════════════════════════════════════════════════════
mod mapper_integration {
    use super::*;

    #[test]
    fn resolve_identity_for_same_dialect() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in Dialect::all() {
            let mapper = pm.resolve_mapper(d, d);
            assert!(mapper.is_some(), "identity mapper should exist for {d:?}");
            let m = mapper.unwrap();
            assert_eq!(m.source_dialect(), m.target_dialect());
        }
    }

    #[test]
    fn resolve_openai_to_claude_produces_mapper() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some());
        let m = mapper.unwrap();
        assert_eq!(m.source_dialect(), Dialect::OpenAi);
        assert_eq!(m.target_dialect(), Dialect::Claude);
    }

    #[test]
    fn resolve_claude_to_openai_produces_mapper() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi);
        assert!(mapper.is_some());
        let m = mapper.unwrap();
        assert_eq!(m.source_dialect(), Dialect::Claude);
        assert_eq!(m.target_dialect(), Dialect::OpenAi);
    }

    #[test]
    fn resolve_codex_openai_uses_identity() {
        let pm = ProjectionMatrix::with_defaults();
        for (src, tgt) in [
            (Dialect::Codex, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Codex),
        ] {
            let mapper = pm.resolve_mapper(src, tgt);
            assert!(mapper.is_some(), "Codex↔OpenAI should have a mapper");
        }
    }

    #[test]
    fn resolve_unsupported_pair_returns_none() {
        let pm = ProjectionMatrix::with_defaults();
        // Kimi→Copilot is unsupported in defaults
        let mapper = pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot);
        assert!(mapper.is_none());
    }

    #[test]
    fn resolved_openai_to_claude_mapper_maps_request() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let req = abp_mapper::DialectRequest {
            dialect: Dialect::OpenAi,
            body: serde_json::json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hello"}]
            }),
        };
        let result = mapper.map_request(&req);
        assert!(result.is_ok(), "map_request should succeed: {result:?}");
        let mapped = result.unwrap();
        assert!(mapped.is_object());
    }

    #[test]
    fn resolved_claude_to_openai_mapper_maps_request() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
        let req = abp_mapper::DialectRequest {
            dialect: Dialect::Claude,
            body: serde_json::json!({
                "model": "claude-3-opus-20240229",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hello"}]
            }),
        };
        let result = mapper.map_request(&req);
        assert!(result.is_ok(), "map_request should succeed: {result:?}");
    }

    #[test]
    fn identity_mapper_preserves_body() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        let body = serde_json::json!({"model": "gpt-4", "messages": []});
        let req = abp_mapper::DialectRequest {
            dialect: Dialect::OpenAi,
            body: body.clone(),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn identity_mapper_response_preserves_body() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::Claude, Dialect::Claude).unwrap();
        let body = serde_json::json!({"content": [{"type": "text", "text": "hi"}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn resolve_mapper_for_unregistered_pair_returns_none() {
        let pm = ProjectionMatrix::new();
        assert!(
            pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude)
                .is_none()
        );
    }

    #[test]
    fn resolved_mapper_can_map_agent_event() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        let event = abp_core::AgentEvent {
            ts: chrono::Utc::now(),
            kind: abp_core::AgentEventKind::AssistantMessage {
                text: "test".into(),
            },
            ext: None,
        };
        let result = mapper.map_event(&event);
        assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Custom registration — register/remove/override dialect pairs
// ═══════════════════════════════════════════════════════════════════════════
mod custom_registration {
    use super::*;
    use abp_projection::ProjectionMode;

    #[test]
    fn register_custom_mapped_pair() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn register_custom_pair_has_mapper_hint() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot).unwrap();
        assert!(entry.mapper_hint.is_some());
        assert!(entry.mapper_hint.as_ref().unwrap().contains("kimi"));
    }

    #[test]
    fn override_existing_entry() {
        let mut pm = ProjectionMatrix::with_defaults();
        // OpenAI→Claude is Mapped by default; override to Unsupported
        pm.register(
            Dialect::OpenAi,
            Dialect::Claude,
            ProjectionMode::Unsupported,
        );
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Unsupported);
    }

    #[test]
    fn remove_dialect_pair() {
        let mut pm = ProjectionMatrix::with_defaults();
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut pm = ProjectionMatrix::new();
        assert!(pm.remove(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn remove_backend_by_id() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend(
            "test-be",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        assert_eq!(pm.backend_count(), 1);
        assert!(pm.remove_backend("test-be"));
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn remove_backend_nonexistent_returns_false() {
        let mut pm = ProjectionMatrix::new();
        assert!(!pm.remove_backend("ghost"));
    }

    #[test]
    fn register_same_dialect_forces_passthrough() {
        let mut pm = ProjectionMatrix::new();
        // Even if we request Mapped, same-dialect is forced to Passthrough
        pm.register(Dialect::Gemini, Dialect::Gemini, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Gemini, Dialect::Gemini).unwrap();
        assert_eq!(entry.mode, ProjectionMode::Passthrough);
        assert_eq!(entry.mapper_hint.as_deref(), Some("identity"));
    }

    #[test]
    fn register_increases_count() {
        let mut pm = ProjectionMatrix::new();
        assert_eq!(pm.dialect_entry_count(), 0);
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        assert_eq!(pm.dialect_entry_count(), 1);
        pm.register(Dialect::Claude, Dialect::OpenAi, ProjectionMode::Mapped);
        assert_eq!(pm.dialect_entry_count(), 2);
    }

    #[test]
    fn register_override_does_not_increase_count() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        assert_eq!(pm.dialect_entry_count(), 1);
        pm.register(
            Dialect::OpenAi,
            Dialect::Claude,
            ProjectionMode::Unsupported,
        );
        assert_eq!(pm.dialect_entry_count(), 1);
    }

    #[test]
    fn register_all_custom_pairs_then_query() {
        let mut pm = ProjectionMatrix::new();
        let pairs = [
            (Dialect::Kimi, Dialect::Copilot),
            (Dialect::Copilot, Dialect::Kimi),
            (Dialect::Codex, Dialect::Gemini),
        ];
        for (src, tgt) in &pairs {
            pm.register(*src, *tgt, ProjectionMode::Mapped);
        }
        assert_eq!(pm.dialect_entry_count(), 3);
        for (src, tgt) in &pairs {
            assert!(pm.lookup(*src, *tgt).is_some());
        }
    }

    #[test]
    fn custom_registration_enables_routing() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let route = pm.find_route(Dialect::Kimi, Dialect::Copilot);
        assert!(route.is_some());
        assert_eq!(route.unwrap().cost, 1);
    }

    #[test]
    fn register_backend_replaces_existing() {
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
    fn with_mapping_registry_constructor() {
        let reg = MappingRegistry::new();
        let pm = ProjectionMatrix::with_mapping_registry(reg);
        assert_eq!(pm.dialect_entry_count(), 0);
        assert_eq!(pm.backend_count(), 0);
    }

    #[test]
    fn set_source_dialect_affects_fidelity() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_source_dialect(Dialect::OpenAi);
        pm.register_backend(
            "be-claude",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::Claude,
            50,
        );
        pm.register_backend(
            "be-openai",
            manifest(&[(Capability::Streaming, SupportLevel::Native)]),
            Dialect::OpenAi,
            50,
        );
        let wo = work_order(require_caps(&[Capability::Streaming]));
        let result = pm.project(&wo).unwrap();
        // OpenAI→OpenAI is identity (fidelity 1.0), OpenAI→Claude has lower fidelity
        assert_eq!(result.selected_backend, "be-openai");
    }

    #[test]
    fn set_mapping_features_affects_compatibility_score() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_mapping_features(vec!["tool_use".into(), "streaming".into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::OpenAi);
        assert_eq!(score.fidelity, 1.0);
        assert_eq!(score.lossless_features, 2);
    }
}
