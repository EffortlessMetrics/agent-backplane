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
//! Deep-pattern tests for the emulation framework.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_emulation::strategies::{
    StreamChunk, StreamingEmulation, ThinkingDetail, ThinkingEmulation, ToolUseEmulation,
    VisionEmulation,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
    emulate_code_execution, emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};
use std::collections::BTreeMap;

// ── Helpers ────────────────────────────────────────────────────────────

fn sys_user_conv(sys: &str, user: &str) -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, sys))
        .push(IrMessage::text(IrRole::User, user))
}

fn user_conv(user: &str) -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, user))
}

fn all_emulatable_caps() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

fn sample_disabled_caps() -> Vec<Capability> {
    vec![
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::Logprobs,
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. EmulationStrategy variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn strategy_system_prompt_injection_stores_prompt() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "custom prompt text".into(),
    };
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert_eq!(prompt, "custom prompt text");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_post_processing_stores_detail() {
    let s = EmulationStrategy::PostProcessing {
        detail: "parse json".into(),
    };
    if let EmulationStrategy::PostProcessing { detail } = &s {
        assert_eq!(detail, "parse json");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_disabled_stores_reason() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe operation".into(),
    };
    if let EmulationStrategy::Disabled { reason } = &s {
        assert_eq!(reason, "unsafe operation");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_variants_are_distinct() {
    let spi = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let pp = EmulationStrategy::PostProcessing { detail: "x".into() };
    let dis = EmulationStrategy::Disabled { reason: "x".into() };
    assert_ne!(spi, pp);
    assert_ne!(spi, dis);
    assert_ne!(pp, dis);
}

#[test]
fn strategy_same_variant_different_content_not_equal() {
    let a = EmulationStrategy::SystemPromptInjection {
        prompt: "alpha".into(),
    };
    let b = EmulationStrategy::SystemPromptInjection {
        prompt: "beta".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn strategy_clone_is_independent() {
    let original = EmulationStrategy::SystemPromptInjection {
        prompt: "original".into(),
    };
    let cloned = original.clone();
    assert_eq!(original, cloned);
    // They are equal but live at different locations.
    let json_a = serde_json::to_string(&original).unwrap();
    let json_b = serde_json::to_string(&cloned).unwrap();
    assert_eq!(json_a, json_b);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Emulation labels
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_label_equality() {
    assert_eq!(FidelityLabel::Native, FidelityLabel::Native);
}

#[test]
fn fidelity_emulated_label_carries_strategy() {
    let label = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    if let FidelityLabel::Emulated { strategy } = &label {
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    } else {
        panic!("expected Emulated");
    }
}

#[test]
fn fidelity_native_not_equal_to_emulated() {
    let native = FidelityLabel::Native;
    let emulated = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    assert_ne!(native, emulated);
}

#[test]
fn fidelity_emulated_labels_differ_by_strategy() {
    let a = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    let b = FidelityLabel::Emulated {
        strategy: emulate_stop_sequences(),
    };
    assert_ne!(a, b);
}

#[test]
fn fidelity_label_debug_includes_variant_name() {
    let native_dbg = format!("{:?}", FidelityLabel::Native);
    assert!(native_dbg.contains("Native"));

    let emulated_dbg = format!(
        "{:?}",
        FidelityLabel::Emulated {
            strategy: emulate_extended_thinking(),
        }
    );
    assert!(emulated_dbg.contains("Emulated"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Strategy selection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn resolve_strategy_uses_config_override_when_present() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "custom".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn resolve_strategy_falls_back_when_no_override() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn resolve_strategy_override_only_affects_targeted_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    // Overridden
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::Disabled { .. }
    ));
    // Not overridden — still uses default
    assert!(matches!(
        engine.resolve_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn resolve_strategy_multiple_overrides_all_effective() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::Disabled { .. }
    ));
    assert!(matches!(
        engine.resolve_strategy(&Capability::CodeExecution),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. System prompt injection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn injection_appends_text_block_to_existing_system_msg() {
    let mut conv = sys_user_conv("base instructions", "hi");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    assert!(sys.content.len() >= 2, "should have appended a block");
    let full = sys.text_content();
    assert!(full.contains("base instructions"));
    assert!(full.contains("Think step by step"));
}

#[test]
fn injection_creates_system_msg_at_position_zero() {
    let mut conv = user_conv("question");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn injection_preserves_non_system_messages_verbatim() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "world"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[1].text_content(), "hello");
    assert_eq!(conv.messages[2].text_content(), "world");
}

#[test]
fn multiple_injections_accumulate_in_system_msg() {
    let mut conv = sys_user_conv("start", "go");
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("Image inputs"));
}

#[test]
fn injection_into_empty_conversation_creates_single_message() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Client-side emulation (post-processing / non-injection)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn post_processing_strategy_recorded_but_conv_unchanged() {
    let original = sys_user_conv("sys", "user");
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn stop_sequences_post_processing_preserves_conversation() {
    let original = user_conv("test");
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(conv, original);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn disabled_strategy_produces_warning_not_applied() {
    let mut conv = user_conv("run code");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert!(!report.warnings.is_empty());
}

#[test]
fn disabled_strategy_warning_mentions_capability_name() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::Streaming]);
    assert!(report.warnings[0].contains("Streaming"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Emulation pipeline (chaining)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_all_emulatable_caps_applied_report() {
    let mut conv = sys_user_conv("system", "do everything");
    let engine = EmulationEngine::with_defaults();
    let caps = all_emulatable_caps();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), caps.len());
    assert!(report.warnings.is_empty());
}

#[test]
fn pipeline_all_disabled_caps_only_warnings() {
    let mut conv = user_conv("go");
    let engine = EmulationEngine::with_defaults();
    let caps = sample_disabled_caps();
    let report = engine.apply(&caps, &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), caps.len());
}

#[test]
fn pipeline_mixed_caps_splits_applied_and_warnings() {
    let mut conv = sys_user_conv("sys", "do it");
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::ImageInput,
        Capability::Streaming,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn pipeline_report_preserves_input_order_for_applied() {
    let mut conv = sys_user_conv("sys", "go");
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::StopSequences,
        Capability::ExtendedThinking,
        Capability::ImageInput,
    ];
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied[0].capability, Capability::StopSequences);
    assert_eq!(report.applied[1].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[2].capability, Capability::ImageInput);
}

#[test]
fn pipeline_sequential_apply_calls_accumulate_in_conversation() {
    let mut conv = sys_user_conv("sys", "go");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ImageInput], &mut conv);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("Image inputs"));
}

#[test]
fn pipeline_check_missing_equals_apply_structure() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StructuredOutputJsonSchema,
    ];
    let check = engine.check_missing(&caps);
    let mut conv = user_conv("test");
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
    for (a, b) in check.applied.iter().zip(apply.applied.iter()) {
        assert_eq!(a.capability, b.capability);
        assert_eq!(a.strategy, b.strategy);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Label propagation (compute_fidelity)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn compute_fidelity_native_caps_get_native_label() {
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
}

#[test]
fn compute_fidelity_emulated_caps_get_emulated_label() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn compute_fidelity_warnings_not_in_labels() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["Capability CodeExecution not emulated: reason".into()],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn compute_fidelity_emulated_overrides_native() {
    let native = vec![Capability::ExtendedThinking];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn compute_fidelity_returns_btreemap_for_deterministic_order() {
    let native = vec![Capability::ToolUse, Capability::Streaming];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    let keys: Vec<_> = labels.keys().collect();
    // BTreeMap sorts by Ord impl
    for pair in keys.windows(2) {
        assert!(pair[0] <= pair[1], "keys must be sorted");
    }
}

#[test]
fn compute_fidelity_empty_inputs_produce_empty_labels() {
    let labels = compute_fidelity(&[], &EmulationReport::default());
    assert!(labels.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Failure modes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn failure_disabled_cap_warning_contains_not_emulated() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn failure_has_unemulatable_true_when_warnings_present() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["something failed".into()],
    };
    assert!(report.has_unemulatable());
}

#[test]
fn failure_has_unemulatable_false_when_no_warnings() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    assert!(!report.has_unemulatable());
}

#[test]
fn failure_multiple_disabled_caps_produce_multiple_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
    ]);
    assert_eq!(report.warnings.len(), 3);
}

#[test]
fn failure_explicit_disable_via_override_produces_warning() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = user_conv("test");
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("user disabled"));
}

#[test]
fn failure_disabled_cap_does_not_mutate_conversation() {
    let original = user_conv("test");
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::CodeExecution, Capability::Streaming],
        &mut conv,
    );
    assert_eq!(conv, original);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Feature detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn feature_detect_extended_thinking_emulatable() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn feature_detect_structured_output_emulatable() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn feature_detect_image_input_emulatable() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn feature_detect_stop_sequences_emulatable() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn feature_detect_code_execution_not_emulatable() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn feature_detect_streaming_not_emulatable() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn feature_detect_tool_use_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn feature_detect_logprobs_not_emulatable() {
    assert!(!can_emulate(&Capability::Logprobs));
}

#[test]
fn feature_detect_seed_determinism_not_emulatable() {
    assert!(!can_emulate(&Capability::SeedDeterminism));
}

#[test]
fn feature_detect_consistent_with_default_strategy() {
    let all_caps = vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::ImageInput,
        Capability::StopSequences,
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::Logprobs,
        Capability::PdfInput,
        Capability::SeedDeterminism,
    ];
    for cap in &all_caps {
        let strategy = default_strategy(cap);
        let emulatable = can_emulate(cap);
        assert_eq!(
            emulatable,
            !matches!(strategy, EmulationStrategy::Disabled { .. }),
            "can_emulate mismatch for {cap:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Cost tracking (report metrics)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cost_report_is_empty_for_no_caps() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
    assert_eq!(report.applied.len(), 0);
    assert_eq!(report.warnings.len(), 0);
}

#[test]
fn cost_report_counts_applied_correctly() {
    let engine = EmulationEngine::with_defaults();
    let caps = all_emulatable_caps();
    let report = engine.check_missing(&caps);
    assert_eq!(report.applied.len(), caps.len());
}

#[test]
fn cost_report_counts_warnings_correctly() {
    let engine = EmulationEngine::with_defaults();
    let caps = sample_disabled_caps();
    let report = engine.check_missing(&caps);
    assert_eq!(report.warnings.len(), caps.len());
}

#[test]
fn cost_report_not_empty_when_has_applied() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
}

#[test]
fn cost_report_not_empty_when_has_warnings() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["warn".into()],
    };
    assert!(!report.is_empty());
}

#[test]
fn cost_duplicate_cap_entries_are_tracked_separately() {
    let mut conv = sys_user_conv("sys", "go");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    // Each appearance is tracked
    assert_eq!(report.applied.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_strategy_system_prompt_injection_roundtrip() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "think hard".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_strategy_post_processing_roundtrip() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate json output".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_strategy_disabled_roundtrip() {
    let s = EmulationStrategy::Disabled {
        reason: "not safe".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_strategy_json_contains_type_tag() {
    let s = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"system_prompt_injection\""));
}

#[test]
fn serde_fidelity_native_roundtrip() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn serde_fidelity_emulated_roundtrip() {
    let label = FidelityLabel::Emulated {
        strategy: emulate_stop_sequences(),
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn serde_fidelity_native_json_contains_fidelity_tag() {
    let json = serde_json::to_string(&FidelityLabel::Native).unwrap();
    assert!(json.contains("\"fidelity\":\"native\""));
}

#[test]
fn serde_fidelity_emulated_json_contains_fidelity_tag() {
    let label = FidelityLabel::Emulated {
        strategy: emulate_extended_thinking(),
    };
    let json = serde_json::to_string(&label).unwrap();
    assert!(json.contains("\"fidelity\":\"emulated\""));
}

#[test]
fn serde_config_empty_roundtrip() {
    let config = EmulationConfig::new();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn serde_config_with_overrides_roundtrip() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ExtendedThinking, emulate_extended_thinking());
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate code".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn serde_report_full_roundtrip() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: emulate_extended_thinking(),
            },
            EmulationEntry {
                capability: Capability::StopSequences,
                strategy: emulate_stop_sequences(),
            },
        ],
        warnings: vec!["cannot emulate code".into(), "streaming unavailable".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn serde_entry_roundtrip() {
    let entry = EmulationEntry {
        capability: Capability::ImageInput,
        strategy: emulate_image_input(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let decoded: EmulationEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn serde_fidelity_map_roundtrip() {
    let mut labels = BTreeMap::new();
    labels.insert(Capability::Streaming, FidelityLabel::Native);
    labels.insert(
        Capability::ExtendedThinking,
        FidelityLabel::Emulated {
            strategy: emulate_extended_thinking(),
        },
    );
    let json = serde_json::to_string(&labels).unwrap();
    let decoded: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

#[test]
fn serde_config_deterministic_serialization() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ToolUse, emulate_extended_thinking());
    config.set(Capability::CodeExecution, emulate_stop_sequences());
    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2, "BTreeMap-based config must be deterministic");
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_no_emulation_needed_empty_caps() {
    let mut conv = sys_user_conv("sys", "hi");
    let original = conv.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn edge_all_emulatable_applied_to_conversation() {
    let mut conv = sys_user_conv("base", "question");
    let engine = EmulationEngine::with_defaults();
    let caps = all_emulatable_caps();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image inputs"));
}

#[test]
fn edge_empty_pipeline_empty_report() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_config_new_equals_default() {
    assert_eq!(EmulationConfig::new(), EmulationConfig::default());
}

#[test]
fn edge_engine_with_defaults_resolves_all_defaults() {
    let engine = EmulationEngine::with_defaults();
    for cap in &all_emulatable_caps() {
        let resolved = engine.resolve_strategy(cap);
        let expected = default_strategy(cap);
        assert_eq!(resolved, expected, "mismatch for {cap:?}");
    }
}

#[test]
fn edge_free_function_apply_emulation_equivalent_to_engine() {
    let config = EmulationConfig::new();
    let caps = vec![Capability::ExtendedThinking, Capability::CodeExecution];

    let mut conv1 = user_conv("test");
    let report1 = apply_emulation(&config, &caps, &mut conv1);

    let mut conv2 = user_conv("test");
    let engine = EmulationEngine::new(config);
    let report2 = engine.apply(&caps, &mut conv2);

    assert_eq!(conv1, conv2);
    assert_eq!(report1.applied.len(), report2.applied.len());
    assert_eq!(report1.warnings.len(), report2.warnings.len());
}

#[test]
fn edge_named_strategy_emulate_structured_output_is_injection() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn edge_named_strategy_emulate_code_execution_is_injection() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn edge_named_strategy_emulate_extended_thinking_is_injection() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn edge_named_strategy_emulate_image_input_is_injection() {
    let s = emulate_image_input();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn edge_named_strategy_emulate_stop_sequences_is_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn edge_config_set_replaces_previous_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "first".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "second".into(),
        },
    );
    assert!(matches!(
        config.strategies[&Capability::ExtendedThinking],
        EmulationStrategy::PostProcessing { .. }
    ));
}

// ── strategies module edge cases ────────────────────────────────────────

#[test]
fn thinking_brief_contains_step_by_step() {
    let t = ThinkingEmulation::brief();
    assert!(t.prompt_text().contains("step by step"));
}

#[test]
fn thinking_standard_contains_thinking_tags() {
    let t = ThinkingEmulation::standard();
    let text = t.prompt_text();
    assert!(text.contains("<thinking>"));
    assert!(text.contains("</thinking>"));
}

#[test]
fn thinking_detailed_contains_sub_problems() {
    let t = ThinkingEmulation::detailed();
    assert!(t.prompt_text().contains("sub-problems"));
}

#[test]
fn thinking_extract_tags_present() {
    let (thinking, answer) =
        ThinkingEmulation::extract_thinking("<thinking>step 1</thinking>The answer");
    assert_eq!(thinking, "step 1");
    assert_eq!(answer, "The answer");
}

#[test]
fn thinking_extract_no_tags() {
    let (thinking, answer) = ThinkingEmulation::extract_thinking("plain answer");
    assert!(thinking.is_empty());
    assert_eq!(answer, "plain answer");
}

#[test]
fn thinking_to_block_returns_some_when_tags_present() {
    let block = ThinkingEmulation::to_thinking_block("<thinking>analysis</thinking>result");
    assert!(block.is_some());
    if let Some(IrContentBlock::Thinking { text }) = block {
        assert_eq!(text, "analysis");
    }
}

#[test]
fn thinking_to_block_returns_none_when_no_tags() {
    let block = ThinkingEmulation::to_thinking_block("no tags here");
    assert!(block.is_none());
}

#[test]
fn thinking_detail_serde_roundtrip() {
    for detail in &[
        ThinkingDetail::Brief,
        ThinkingDetail::Standard,
        ThinkingDetail::Detailed,
    ] {
        let json = serde_json::to_string(detail).unwrap();
        let decoded: ThinkingDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(*detail, decoded);
    }
}

#[test]
fn tool_use_parse_valid_call() {
    let text = r#"<tool_call>{"name": "read", "arguments": {"path": "/a"}}</tool_call>"#;
    let results = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(results.len(), 1);
    let call = results[0].as_ref().unwrap();
    assert_eq!(call.name, "read");
}

#[test]
fn tool_use_parse_no_calls() {
    let results = ToolUseEmulation::parse_tool_calls("just plain text");
    assert!(results.is_empty());
}

#[test]
fn tool_use_prompt_empty_tools_returns_empty() {
    assert!(ToolUseEmulation::tools_to_prompt(&[]).is_empty());
}

#[test]
fn tool_use_prompt_includes_tool_name() {
    let tools = vec![IrToolDefinition {
        name: "my_tool".into(),
        description: "does stuff".into(),
        parameters: serde_json::Value::Null,
    }];
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(prompt.contains("my_tool"));
    assert!(prompt.contains("does stuff"));
}

#[test]
fn tool_use_extract_text_outside() {
    let text = "before <tool_call>{\"name\":\"x\",\"arguments\":{}}</tool_call> after";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("before"));
    assert!(outside.contains("after"));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn tool_use_format_result_success_and_error() {
    let success = ToolUseEmulation::format_tool_result("t", "ok", false);
    assert!(success.contains("returned:"));
    let error = ToolUseEmulation::format_tool_result("t", "fail", true);
    assert!(error.contains("error"));
}

#[test]
fn vision_has_images_detects_image_blocks() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    ));
    assert!(VisionEmulation::has_images(&conv));
}

#[test]
fn vision_has_images_false_for_text_only() {
    let conv = user_conv("text only");
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_replace_returns_count() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "d1".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "d2".into(),
            },
        ],
    ));
    let count = VisionEmulation::replace_images_with_placeholders(&mut conv);
    assert_eq!(count, 2);
}

#[test]
fn vision_apply_full_pipeline_replaces_and_injects() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "d".into(),
        }],
    ));
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    assert!(conv.system_message().is_some());
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("1 image(s)"));
}

#[test]
fn vision_no_images_noop() {
    let mut conv = user_conv("no images");
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 0);
    assert!(conv.system_message().is_none());
}

#[test]
fn streaming_split_and_reassemble_roundtrip() {
    let text = "Hello world, this is a longer text for streaming emulation testing.";
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_fixed_split_roundtrip() {
    let text = "abcdefghijklmnop";
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed(text);
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, text);
}

#[test]
fn streaming_only_last_chunk_is_final() {
    let emu = StreamingEmulation::new(3);
    let chunks = emu.split_into_chunks("hello world");
    for (i, chunk) in chunks.iter().enumerate() {
        if i == chunks.len() - 1 {
            assert!(chunk.is_final);
        } else {
            assert!(!chunk.is_final);
        }
    }
}

#[test]
fn streaming_chunk_indices_are_sequential() {
    let emu = StreamingEmulation::new(4);
    let chunks = emu.split_into_chunks("a longer sentence for testing");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }
}

#[test]
fn streaming_empty_text_produces_single_final_chunk() {
    let emu = StreamingEmulation::new(10);
    let chunks = emu.split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].is_final);
    assert!(chunks[0].content.is_empty());
}

#[test]
fn streaming_minimum_chunk_size_is_one() {
    let emu = StreamingEmulation::new(0);
    assert_eq!(emu.chunk_size(), 1);
}

#[test]
fn streaming_chunk_serde_roundtrip() {
    let chunk = StreamChunk {
        content: "hello".into(),
        index: 0,
        is_final: true,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, decoded);
}
