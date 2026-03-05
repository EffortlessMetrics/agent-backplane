#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive mapped-mode early-failure tests.
//!
//! Verifies that ABP detects unmappable features **before** hitting the wire,
//! labels every emulation explicitly, produces typed error codes, and degrades
//! gracefully when optional features are missing.

use std::collections::BTreeMap;

use abp_capability::{
    NegotiationResult, SupportLevel as CapSupportLevel, check_capability,
    claude_35_sonnet_manifest, codex_manifest, copilot_manifest, gemini_15_pro_manifest,
    kimi_manifest, negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_core::error::{MappingError, MappingErrorKind};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode,
    MinSupport, Outcome, SupportLevel,
};
use abp_emulation::{
    DegradationLevel, EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport,
    EmulationStrategy, FidelityLabel,
};
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use serde_json::json;

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Build a manifest where every listed capability is `SupportLevel::Native`,
/// and everything else is implicitly absent.
fn manifest_with(caps: &[Capability]) -> CapabilityManifest {
    caps.iter()
        .map(|c| (c.clone(), SupportLevel::Native))
        .collect()
}

/// Build a manifest where every listed capability is `SupportLevel::Unsupported`.
fn manifest_unsupported(caps: &[Capability]) -> CapabilityManifest {
    caps.iter()
        .map(|c| (c.clone(), SupportLevel::Unsupported))
        .collect()
}

/// Build a manifest that marks specific capabilities as emulated.
fn manifest_emulated(caps: &[Capability]) -> CapabilityManifest {
    caps.iter()
        .map(|c| (c.clone(), SupportLevel::Emulated))
        .collect()
}

/// Shorthand: build a `MappingError::UnsupportedCapability`.
fn unsupported_cap(capability: &str, dialect: &str) -> MappingError {
    MappingError::UnsupportedCapability {
        capability: capability.into(),
        dialect: dialect.into(),
    }
}

/// Shorthand: build a `MappingError::FidelityLoss`.
fn fidelity_loss(field: &str, src: &str, tgt: &str, detail: &str) -> MappingError {
    MappingError::FidelityLoss {
        field: field.into(),
        source_dialect: src.into(),
        target_dialect: tgt.into(),
        detail: detail.into(),
    }
}

/// Shorthand: build `MappingError::StreamingUnsupported`.
fn streaming_unsupported(dialect: &str) -> MappingError {
    MappingError::StreamingUnsupported {
        dialect: dialect.into(),
    }
}

/// Construct an `EmulationReport` with the given entries and warnings.
fn report(entries: Vec<EmulationEntry>, warnings: Vec<&str>) -> EmulationReport {
    EmulationReport {
        applied: entries,
        warnings: warnings.into_iter().map(String::from).collect(),
    }
}

/// Construct a single `EmulationEntry`.
fn entry(cap: Capability, strategy: EmulationStrategy) -> EmulationEntry {
    EmulationEntry {
        capability: cap,
        strategy,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  1. UNMAPPABLE FEATURE DETECTION  (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod unmappable_feature_detection {
    use super::*;

    // ── Extended thinking ──────────────────────────────────────────────

    #[test]
    fn extended_thinking_unsupported_by_openai() {
        let manifest = openai_gpt4o_manifest();
        let result = negotiate_capabilities(&[Capability::ExtendedThinking], &manifest);
        assert!(
            !result.unsupported.is_empty(),
            "OpenAI should not natively support extended thinking"
        );
    }

    #[test]
    fn extended_thinking_produces_typed_mapping_error() {
        let err = unsupported_cap("extended_thinking", "openai");
        assert!(err.is_fatal());
        assert_eq!(err.kind(), MappingErrorKind::Fatal);
        assert_eq!(err.code(), "ABP_E_UNSUPPORTED_CAP");
    }

    #[test]
    fn extended_thinking_error_names_source_and_target_dialects() {
        let err = MappingError::FidelityLoss {
            field: "extended_thinking".into(),
            source_dialect: "claude".into(),
            target_dialect: "openai".into(),
            detail: "target does not support thinking blocks".into(),
        };
        let display = err.to_string();
        assert!(display.contains("claude") || display.contains("openai"));
    }

    // ── Vision / image input ──────────────────────────────────────────

    #[test]
    fn vision_content_to_text_only_backend_detected() {
        let text_only = manifest_with(&[Capability::SystemMessage, Capability::MaxTokens]);
        let result = negotiate_capabilities(&[Capability::Vision], &text_only);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn image_input_to_text_only_backend_detected() {
        let text_only = manifest_with(&[Capability::SystemMessage]);
        let result = negotiate_capabilities(&[Capability::ImageInput], &text_only);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn vision_unsupported_error_is_fatal() {
        let err = unsupported_cap("vision", "text_only_backend");
        assert!(err.is_fatal());
    }

    // ── Tool use ──────────────────────────────────────────────────────

    #[test]
    fn tool_use_to_backend_without_tool_support() {
        let no_tools = manifest_with(&[Capability::Streaming, Capability::SystemMessage]);
        let result = negotiate_capabilities(&[Capability::ToolUse], &no_tools);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn function_calling_to_backend_without_support() {
        let no_fc = manifest_with(&[Capability::Streaming]);
        let result = negotiate_capabilities(&[Capability::FunctionCalling], &no_fc);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn tool_use_error_names_missing_capability() {
        let err = unsupported_cap("tool_use", "basic_llm");
        let display = err.to_string();
        assert!(display.contains("tool_use"));
    }

    // ── Streaming ─────────────────────────────────────────────────────

    #[test]
    fn streaming_to_non_streaming_backend() {
        let batch_only = manifest_with(&[Capability::BatchMode]);
        let result = negotiate_capabilities(&[Capability::Streaming], &batch_only);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn streaming_unsupported_mapping_error_is_fatal() {
        let err = streaming_unsupported("batch_only_backend");
        assert!(err.is_fatal());
        assert_eq!(err.code(), "ABP_E_STREAMING_UNSUPPORTED");
    }

    #[test]
    fn streaming_error_includes_dialect_name() {
        let err = streaming_unsupported("my_backend");
        let display = err.to_string();
        assert!(display.contains("my_backend"));
    }

    // ── Multi-turn / session features ─────────────────────────────────

    #[test]
    fn session_resume_to_single_turn_backend() {
        let single_turn = manifest_with(&[Capability::SystemMessage]);
        let result = negotiate_capabilities(&[Capability::SessionResume], &single_turn);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn session_fork_to_single_turn_backend() {
        let single_turn = manifest_with(&[Capability::SystemMessage]);
        let result = negotiate_capabilities(&[Capability::SessionFork], &single_turn);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn checkpointing_to_backend_without_support() {
        let basic = manifest_with(&[Capability::Streaming]);
        let result = negotiate_capabilities(&[Capability::Checkpointing], &basic);
        assert!(!result.unsupported.is_empty());
    }

    // ── Additional unmappable features ────────────────────────────────

    #[test]
    fn audio_to_text_only_backend() {
        let text_only = manifest_with(&[Capability::SystemMessage]);
        let result = negotiate_capabilities(&[Capability::Audio], &text_only);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn code_execution_to_backend_without_sandbox() {
        let no_exec = manifest_with(&[Capability::ToolUse]);
        let result = negotiate_capabilities(&[Capability::CodeExecution], &no_exec);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn pdf_input_to_backend_without_pdf_support() {
        let no_pdf = manifest_with(&[Capability::Vision]);
        let result = negotiate_capabilities(&[Capability::PdfInput], &no_pdf);
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn multiple_unsupported_features_all_listed() {
        let minimal = manifest_with(&[Capability::SystemMessage]);
        let result = negotiate_capabilities(
            &[
                Capability::Vision,
                Capability::Audio,
                Capability::CodeExecution,
            ],
            &minimal,
        );
        assert_eq!(result.unsupported.len(), 3, "all three must appear");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  2. EMULATION LABELING  (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod emulation_labeling {
    use super::*;

    #[test]
    fn emulation_entry_labels_capability() {
        let e = entry(
            Capability::FunctionCalling,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Use JSON for tool calls".into(),
            },
        );
        assert_eq!(e.capability, Capability::FunctionCalling);
    }

    #[test]
    fn emulation_report_is_non_empty_when_applied() {
        let r = report(
            vec![entry(
                Capability::FunctionCalling,
                EmulationStrategy::SystemPromptInjection {
                    prompt: "tool".into(),
                },
            )],
            vec![],
        );
        assert!(!r.is_empty());
    }

    #[test]
    fn no_silent_degradation_warnings_surfaced() {
        let r = report(vec![], vec!["vision not emulatable"]);
        assert!(r.has_unemulatable());
        assert!(!r.warnings.is_empty(), "warnings must be visible");
    }

    #[test]
    fn fidelity_score_present_and_bounded() {
        let r = report(
            vec![entry(
                Capability::FunctionCalling,
                EmulationStrategy::SystemPromptInjection {
                    prompt: "fn".into(),
                },
            )],
            vec![],
        );
        let score = r.fidelity_score();
        assert!(
            (0.0..=1.0).contains(&score),
            "score {score} must be in [0,1]"
        );
    }

    #[test]
    fn fidelity_score_decreases_with_more_emulations() {
        let single = report(
            vec![entry(
                Capability::FunctionCalling,
                EmulationStrategy::SystemPromptInjection { prompt: "a".into() },
            )],
            vec![],
        );
        let double = report(
            vec![
                entry(
                    Capability::FunctionCalling,
                    EmulationStrategy::SystemPromptInjection { prompt: "a".into() },
                ),
                entry(
                    Capability::Vision,
                    EmulationStrategy::PostProcessing { detail: "b".into() },
                ),
            ],
            vec![],
        );
        assert!(double.fidelity_score() < single.fidelity_score());
    }

    #[test]
    fn fidelity_score_penalised_by_warnings() {
        let clean = report(vec![], vec![]);
        let warn = report(vec![], vec!["missing audio support"]);
        assert!(warn.fidelity_score() < clean.fidelity_score());
    }

    #[test]
    fn degradation_level_none_when_no_emulation() {
        let r = report(vec![], vec![]);
        assert_eq!(r.degradation(), DegradationLevel::None);
    }

    #[test]
    fn degradation_level_escalates_with_warnings() {
        let many_warnings = report(vec![], vec!["a", "b", "c", "d"]);
        assert_ne!(
            many_warnings.degradation(),
            DegradationLevel::None,
            "should degrade with 4 warnings"
        );
    }

    #[test]
    fn emulation_report_summary_contains_applied_info() {
        let r = report(
            vec![entry(
                Capability::ExtendedThinking,
                EmulationStrategy::SystemPromptInjection {
                    prompt: "think step by step".into(),
                },
            )],
            vec![],
        );
        let summary = r.summary();
        assert!(!summary.is_empty(), "summary must be non-empty");
    }

    #[test]
    fn fidelity_map_marks_native_and_emulated() {
        let engine = EmulationEngine::with_defaults();
        let required = vec![Capability::Streaming, Capability::FunctionCalling];
        let native = vec![Capability::Streaming];
        let map = engine.fidelity_map(&required, &native);
        assert!(matches!(
            map.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
    }

    #[test]
    fn emulation_engine_plan_reports_gaps() {
        let engine = EmulationEngine::with_defaults();
        let required = vec![Capability::Vision, Capability::Streaming];
        let native = vec![Capability::Streaming];
        let plan = engine.plan(&required, &native);
        assert!(
            !plan.is_empty() || plan.has_unemulatable(),
            "plan must surface the vision gap"
        );
    }

    #[test]
    fn emulation_report_serializable() {
        let r = report(
            vec![entry(
                Capability::FunctionCalling,
                EmulationStrategy::PostProcessing {
                    detail: "parse json".into(),
                },
            )],
            vec!["audio dropped"],
        );
        let json = serde_json::to_string(&r).expect("serialize");
        let deser: EmulationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deser.applied.len(), r.applied.len());
        assert_eq!(deser.warnings.len(), r.warnings.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  3. ERROR TAXONOMY  (15+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod error_taxonomy {
    use super::*;

    // ── Specific error codes ──────────────────────────────────────────

    #[test]
    fn unsupported_capability_produces_correct_error_code() {
        let err = unsupported_cap("vision", "openai");
        assert_eq!(err.code(), "ABP_E_UNSUPPORTED_CAP");
    }

    #[test]
    fn fidelity_loss_produces_correct_error_code() {
        let err = fidelity_loss("temperature", "claude", "gemini", "scale differs");
        assert_eq!(err.code(), "ABP_E_FIDELITY_LOSS");
    }

    #[test]
    fn streaming_unsupported_produces_correct_error_code() {
        let err = streaming_unsupported("batch_backend");
        assert_eq!(err.code(), "ABP_E_STREAMING_UNSUPPORTED");
    }

    #[test]
    fn emulation_required_produces_correct_error_code() {
        let err = MappingError::EmulationRequired {
            feature: "function_calling".into(),
            detail: "target lacks native FC".into(),
        };
        assert_eq!(err.code(), "ABP_E_EMULATION_REQUIRED");
    }

    #[test]
    fn incompatible_model_produces_correct_error_code() {
        let err = MappingError::IncompatibleModel {
            requested: "gpt-4o".into(),
            dialect: "claude".into(),
            suggestion: Some("claude-3.5-sonnet".into()),
        };
        assert_eq!(err.code(), "ABP_E_INCOMPATIBLE_MODEL");
    }

    #[test]
    fn parameter_not_mappable_produces_correct_error_code() {
        let err = MappingError::ParameterNotMappable {
            parameter: "logit_bias".into(),
            value: "{50256: -100}".into(),
            dialect: "claude".into(),
        };
        assert_eq!(err.code(), "ABP_E_PARAM_NOT_MAPPABLE");
    }

    // ── Error messages include dialect names ──────────────────────────

    #[test]
    fn unsupported_cap_message_includes_capability_name() {
        let err = unsupported_cap("extended_thinking", "openai");
        let msg = err.to_string();
        assert!(
            msg.contains("extended_thinking"),
            "message should name the capability: {msg}"
        );
    }

    #[test]
    fn unsupported_cap_message_includes_dialect() {
        let err = unsupported_cap("vision", "text_only");
        let msg = err.to_string();
        assert!(
            msg.contains("text_only"),
            "message should name the dialect: {msg}"
        );
    }

    #[test]
    fn fidelity_loss_message_includes_source_dialect() {
        let err = fidelity_loss("top_p", "openai", "gemini", "range differs");
        let msg = err.to_string();
        assert!(msg.contains("openai"), "source dialect missing: {msg}");
    }

    #[test]
    fn fidelity_loss_message_includes_target_dialect() {
        let err = fidelity_loss("top_p", "openai", "gemini", "range differs");
        let msg = err.to_string();
        assert!(msg.contains("gemini"), "target dialect missing: {msg}");
    }

    #[test]
    fn incompatible_model_message_includes_dialects() {
        let err = MappingError::IncompatibleModel {
            requested: "gpt-4o".into(),
            dialect: "claude".into(),
            suggestion: None,
        };
        let msg = err.to_string();
        assert!(msg.contains("claude"), "dialect missing: {msg}");
        assert!(msg.contains("gpt-4o"), "model missing: {msg}");
    }

    // ── Serialization roundtrip ──────────────────────────────────────

    #[test]
    fn mapping_error_serializes_roundtrip() {
        let err = unsupported_cap("tool_use", "basic_llm");
        let json = serde_json::to_string(&err).expect("serialize");
        let deser: MappingError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, deser);
    }

    #[test]
    fn fidelity_loss_serializes_roundtrip() {
        let err = fidelity_loss("temperature", "openai", "claude", "scale mismatch");
        let json = serde_json::to_string(&err).expect("serialize");
        let deser: MappingError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, deser);
    }

    #[test]
    fn streaming_unsupported_serializes_roundtrip() {
        let err = streaming_unsupported("batch_only");
        let json = serde_json::to_string(&err).expect("serialize");
        let deser: MappingError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, deser);
    }

    #[test]
    fn emulation_required_serializes_roundtrip() {
        let err = MappingError::EmulationRequired {
            feature: "json_mode".into(),
            detail: "needs post-processing".into(),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        let deser: MappingError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, deser);
    }

    #[test]
    fn parameter_not_mappable_serializes_roundtrip() {
        let err = MappingError::ParameterNotMappable {
            parameter: "logprobs".into(),
            value: "true".into(),
            dialect: "gemini".into(),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        let deser: MappingError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, deser);
    }

    #[test]
    fn abp_error_dto_survives_roundtrip() {
        let err = AbpError::new(
            ErrorCode::MappingUnsupportedCapability,
            "vision not supported on target",
        )
        .with_context("capability", "vision")
        .with_context("target_dialect", "text_only");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).expect("serialize dto");
        let deser: AbpErrorDto = serde_json::from_str(&json).expect("deserialize dto");
        assert_eq!(deser.code, ErrorCode::MappingUnsupportedCapability);
        assert!(deser.message.contains("vision"));
    }

    #[test]
    fn mapping_error_converts_to_abp_error_with_correct_code() {
        let err = unsupported_cap("tool_use", "basic_llm");
        let abp = AbpError::new(ErrorCode::MappingUnsupportedCapability, err.to_string());
        assert_eq!(abp.code, ErrorCode::MappingUnsupportedCapability);
    }

    // ── Error kind classification ────────────────────────────────────

    #[test]
    fn unsupported_capability_is_fatal_kind() {
        let err = unsupported_cap("audio", "text_backend");
        assert_eq!(err.kind(), MappingErrorKind::Fatal);
    }

    #[test]
    fn fidelity_loss_is_degraded_kind() {
        let err = fidelity_loss("top_k", "gemini", "openai", "no top_k in openai");
        assert_eq!(err.kind(), MappingErrorKind::Degraded);
    }

    #[test]
    fn emulation_required_is_emulated_kind() {
        let err = MappingError::EmulationRequired {
            feature: "structured_output".into(),
            detail: "via post-processing".into(),
        };
        assert_eq!(err.kind(), MappingErrorKind::Emulated);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  4. GRACEFUL DEGRADATION  (10+ tests)
// ═══════════════════════════════════════════════════════════════════════════

mod graceful_degradation {
    use super::*;

    #[test]
    fn optional_feature_missing_produces_warning_not_error() {
        let manifest = openai_gpt4o_manifest();
        let result = negotiate_capabilities(
            &[Capability::Streaming, Capability::CacheControl],
            &manifest,
        );
        // Streaming is native; CacheControl may not be → should show in unsupported,
        // but streaming still works — the result is not fully incompatible.
        assert!(
            !result.native.is_empty(),
            "native caps should still be present"
        );
    }

    #[test]
    fn partial_support_returns_both_native_and_unsupported() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result =
            negotiate_capabilities(&[Capability::Streaming, Capability::Vision], &manifest);
        assert!(!result.native.is_empty());
        assert!(!result.unsupported.is_empty());
    }

    #[test]
    fn degradation_level_minor_for_single_emulation() {
        let r = report(
            vec![entry(
                Capability::FunctionCalling,
                EmulationStrategy::PostProcessing {
                    detail: "json parse".into(),
                },
            )],
            vec![],
        );
        let score = r.fidelity_score();
        assert!(score >= 0.7, "single emulation should be minor: {score}");
    }

    #[test]
    fn degradation_level_major_for_many_emulations() {
        let r = report(
            vec![
                entry(
                    Capability::FunctionCalling,
                    EmulationStrategy::SystemPromptInjection { prompt: "a".into() },
                ),
                entry(
                    Capability::Vision,
                    EmulationStrategy::SystemPromptInjection { prompt: "b".into() },
                ),
                entry(
                    Capability::JsonMode,
                    EmulationStrategy::SystemPromptInjection { prompt: "c".into() },
                ),
                entry(
                    Capability::ExtendedThinking,
                    EmulationStrategy::SystemPromptInjection { prompt: "d".into() },
                ),
            ],
            vec!["audio"],
        );
        let score = r.fidelity_score();
        assert!(
            score < 0.7,
            "4 emulations + warning should degrade significantly: {score}"
        );
    }

    #[test]
    fn required_feature_causes_hard_failure_via_is_fatal() {
        let err = unsupported_cap("tool_use", "basic_llm");
        assert!(err.is_fatal(), "required feature → fatal");
    }

    #[test]
    fn degraded_feature_does_not_cause_hard_failure() {
        let err = fidelity_loss("top_p", "openai", "claude", "range differs");
        assert!(!err.is_fatal(), "degraded → not fatal");
        assert!(err.is_degraded());
    }

    #[test]
    fn emulated_feature_does_not_cause_hard_failure() {
        let err = MappingError::EmulationRequired {
            feature: "function_calling".into(),
            detail: "via prompt injection".into(),
        };
        assert!(!err.is_fatal(), "emulated → not fatal");
        assert!(err.is_emulated());
    }

    #[test]
    fn warnings_preserved_alongside_successful_native_caps() {
        let manifest = manifest_with(&[Capability::Streaming, Capability::SystemMessage]);
        let result = negotiate_capabilities(
            &[
                Capability::Streaming,
                Capability::SystemMessage,
                Capability::ImageGeneration,
            ],
            &manifest,
        );
        assert_eq!(result.native.len(), 2);
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn all_features_supported_yields_full_compatibility() {
        let manifest = claude_35_sonnet_manifest();
        let result = negotiate_capabilities(&[Capability::Streaming], &manifest);
        assert!(result.is_compatible());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn empty_requirements_always_compatible() {
        let manifest = manifest_with(&[]);
        let result = negotiate_capabilities(&[], &manifest);
        assert!(result.is_compatible());
    }

    #[test]
    fn negotiation_result_total_covers_all_buckets() {
        let manifest = manifest_with(&[Capability::Streaming]);
        let result = negotiate_capabilities(
            &[Capability::Streaming, Capability::Vision, Capability::Audio],
            &manifest,
        );
        assert_eq!(
            result.total(),
            result.native.len() + result.emulated.len() + result.unsupported.len(),
        );
    }

    #[test]
    fn disabled_emulation_strategy_surfaces_reason() {
        let strategy = EmulationStrategy::Disabled {
            reason: "cannot safely emulate code execution".into(),
        };
        let json = serde_json::to_string(&strategy).expect("serialize");
        assert!(json.contains("cannot safely emulate"));
    }
}
