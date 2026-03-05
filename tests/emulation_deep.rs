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
//! Comprehensive deep tests for the ABP emulation layer.
//!
//! Covers: config construction & serde, engine lifecycle, labeled/transparent/hybrid
//! emulation, capability mapping, error handling, concurrency, receipt metadata,
//! performance overhead tracking, config validation, feature support detection,
//! fallback chains, and integration with capability negotiation.

use std::collections::BTreeMap;

use abp_core::ir::{IrConversation, IrMessage, IrRole};
use abp_core::Capability;
use abp_emulation::{
    apply_emulation, can_emulate, compute_fidelity, default_strategy, emulate_code_execution,
    emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output, EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport,
    EmulationStrategy, FidelityLabel,
};

// ── Helpers ────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn multi_turn_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base system prompt."))
        .push(IrMessage::text(IrRole::User, "First question"))
        .push(IrMessage::text(IrRole::Assistant, "First answer"))
        .push(IrMessage::text(IrRole::User, "Follow-up question"))
}

fn all_emulatable() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

fn all_disabled_by_default() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::McpClient,
        Capability::McpServer,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::PdfInput,
    ]
}

// ════════════════════════════════════════════════════════════════════════
// 1. EmulationConfig construction and serde
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_new_is_empty() {
    let cfg = EmulationConfig::new();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn config_default_is_empty() {
    let cfg = EmulationConfig::default();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn config_new_equals_default() {
    assert_eq!(EmulationConfig::new(), EmulationConfig::default());
}

#[test]
fn config_set_inserts_strategy() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(cfg.strategies.contains_key(&Capability::ExtendedThinking));
}

#[test]
fn config_set_overwrites_previous() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "first".into(),
        },
    );
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "second".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(matches!(
        cfg.strategies[&Capability::ExtendedThinking],
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_set_multiple_capabilities() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    cfg.set(Capability::ImageInput, emulate_image_input());
    cfg.set(Capability::StopSequences, emulate_stop_sequences());
    assert_eq!(cfg.strategies.len(), 3);
}

#[test]
fn config_serde_roundtrip_empty() {
    let cfg = EmulationConfig::new();
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_serde_roundtrip_with_system_prompt_injection() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think carefully.".into(),
        },
    );
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_serde_roundtrip_with_post_processing() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::StructuredOutputJsonSchema,
        EmulationStrategy::PostProcessing {
            detail: "validate JSON".into(),
        },
    );
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_serde_roundtrip_with_disabled() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_serde_roundtrip_all_strategy_types() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think.".into(),
        },
    );
    cfg.set(
        Capability::StopSequences,
        EmulationStrategy::PostProcessing {
            detail: "truncate".into(),
        },
    );
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "nope".into(),
        },
    );
    let json = serde_json::to_string(&cfg).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, decoded);
}

#[test]
fn config_serde_json_shape_has_strategies_key() {
    let cfg = EmulationConfig::new();
    let val: serde_json::Value = serde_json::to_value(&cfg).unwrap();
    assert!(val.as_object().unwrap().contains_key("strategies"));
}

#[test]
fn config_clone_is_independent() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    let mut cloned = cfg.clone();
    cloned.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "removed".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert_eq!(cloned.strategies.len(), 2);
}

// ════════════════════════════════════════════════════════════════════════
// 2. EmulationEngine lifecycle
// ════════════════════════════════════════════════════════════════════════

#[test]
fn engine_with_defaults_resolves_default_strategies() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn engine_new_with_config_uses_overrides() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn engine_new_with_empty_config_uses_defaults() {
    let engine = EmulationEngine::new(EmulationConfig::new());
    let s = engine.resolve_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn engine_clone_is_independent() {
    let engine = EmulationEngine::with_defaults();
    let cloned = engine.clone();
    // Both resolve identically
    assert_eq!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        cloned.resolve_strategy(&Capability::ExtendedThinking),
    );
}

#[test]
fn engine_debug_impl_exists() {
    let engine = EmulationEngine::with_defaults();
    let debug = format!("{engine:?}");
    assert!(debug.contains("EmulationEngine"));
}

// ════════════════════════════════════════════════════════════════════════
// 3. Labeled emulation (features marked as emulated)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn labeled_emulation_report_records_every_applied_strategy() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = simple_conv();
    let report = engine.apply(&all_emulatable(), &mut conv);
    assert_eq!(report.applied.len(), all_emulatable().len());
}

#[test]
fn labeled_emulation_entry_has_matching_capability() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(report.applied[0].capability, Capability::ImageInput);
}

#[test]
fn labeled_emulation_entry_has_matching_strategy() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let expected = engine.resolve_strategy(&Capability::ImageInput);
    assert_eq!(report.applied[0].strategy, expected);
}

#[test]
fn labeled_fidelity_emulated_for_all_applied() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&all_emulatable(), &mut conv);
    let labels = compute_fidelity(&[], &report);
    for cap in &all_emulatable() {
        assert!(
            matches!(labels[cap], FidelityLabel::Emulated { .. }),
            "expected Emulated for {cap:?}"
        );
    }
}

#[test]
fn labeled_fidelity_native_for_native_caps() {
    let report = EmulationReport::default();
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let labels = compute_fidelity(&native, &report);
    for cap in &native {
        assert_eq!(labels[cap], FidelityLabel::Native);
    }
}

#[test]
fn labeled_fidelity_mixed_native_and_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn labeled_fidelity_emulated_carries_strategy_detail() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    } else {
        panic!("expected Emulated label");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 4. Transparent emulation (invisible to caller – post-processing)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn transparent_post_processing_does_not_mutate_conv() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn transparent_stop_sequences_does_not_mutate_conv() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn transparent_post_processing_still_recorded_in_report() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn transparent_multiple_post_processing_none_mutate() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[
            Capability::StructuredOutputJsonSchema,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    assert_eq!(conv, original);
}

// ════════════════════════════════════════════════════════════════════════
// 5. Hybrid emulation strategies
// ════════════════════════════════════════════════════════════════════════

#[test]
fn hybrid_system_prompt_and_post_processing_together() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,           // SystemPromptInjection
            Capability::StructuredOutputJsonSchema, // PostProcessing
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    // System prompt was modified
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    // But conversation still has original structure
    assert_eq!(conv.messages.len(), 2);
}

#[test]
fn hybrid_all_emulatable_strategies_compose() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&all_emulatable(), &mut conv);
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn hybrid_emulatable_plus_disabled_caps() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::Streaming,
            Capability::CodeExecution,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn hybrid_config_overrides_mixed_with_defaults() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::CodeExecution, emulate_code_execution());
    // leave ExtendedThinking at default
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_only_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert!(report.warnings.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 6. Capability emulation mapping
// ════════════════════════════════════════════════════════════════════════

#[test]
fn default_strategy_extended_thinking_is_system_prompt() {
    let s = default_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_structured_output_is_post_processing() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_code_execution_is_disabled() {
    let s = default_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_image_input_is_system_prompt() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_stop_sequences_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_streaming_is_disabled() {
    let s = default_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_use_is_disabled() {
    let s = default_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_all_disabled_caps_are_disabled() {
    for cap in all_disabled_by_default() {
        let s = default_strategy(&cap);
        assert!(
            matches!(s, EmulationStrategy::Disabled { .. }),
            "{cap:?} should be Disabled by default"
        );
    }
}

#[test]
fn default_strategy_all_emulatable_caps_are_not_disabled() {
    for cap in all_emulatable() {
        let s = default_strategy(&cap);
        assert!(
            !matches!(s, EmulationStrategy::Disabled { .. }),
            "{cap:?} should NOT be Disabled by default"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// 7. Emulation error handling
// ════════════════════════════════════════════════════════════════════════

#[test]
fn disabled_strategy_produces_warning_not_panic() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
    assert!(!report.warnings.is_empty());
}

#[test]
fn disabled_warning_message_contains_capability_name() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::Streaming], &mut conv);
    assert!(report.warnings[0].contains("Streaming"));
}

#[test]
fn disabled_warning_message_contains_not_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ToolUse], &mut conv);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn multiple_disabled_produce_multiple_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::CodeExecution,
        ],
        &mut conv,
    );
    assert_eq!(report.warnings.len(), 3);
}

#[test]
fn disabled_does_not_modify_conversation() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&all_disabled_by_default(), &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn report_has_unemulatable_when_disabled_present() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.has_unemulatable());
}

#[test]
fn report_no_unemulatable_when_all_succeed() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&all_emulatable(), &mut conv);
    assert!(!report.has_unemulatable());
}

// ════════════════════════════════════════════════════════════════════════
// 8. Multiple concurrent emulations (sequential application)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_system_prompt_injections_accumulate() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image"));
}

#[test]
fn apply_twice_accumulates_system_prompts() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ImageInput], &mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image"));
}

#[test]
fn apply_same_cap_twice_duplicates_injection() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap().text_content();
    // The injection text should appear twice
    let count = sys.matches("Think step by step").count();
    assert_eq!(count, 2);
}

#[test]
fn separate_engines_apply_independently() {
    let engine1 = EmulationEngine::with_defaults();
    let mut cfg2 = EmulationConfig::new();
    cfg2.set(Capability::CodeExecution, emulate_code_execution());
    let engine2 = EmulationEngine::new(cfg2);

    let mut conv1 = user_only_conv();
    let mut conv2 = user_only_conv();

    let r1 = engine1.apply(&[Capability::CodeExecution], &mut conv1);
    let r2 = engine2.apply(&[Capability::CodeExecution], &mut conv2);

    // engine1 disables it, engine2 emulates it
    assert!(r1.applied.is_empty());
    assert_eq!(r1.warnings.len(), 1);
    assert_eq!(r2.applied.len(), 1);
    assert!(r2.warnings.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 9. Emulation metadata in receipts (fidelity labels)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_map_serde_roundtrip() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    let json = serde_json::to_string(&labels).unwrap();
    let decoded: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

#[test]
fn fidelity_native_serde_roundtrip() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn fidelity_emulated_serde_roundtrip() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::SystemPromptInjection {
            prompt: "test".into(),
        },
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn fidelity_map_empty_when_no_inputs() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_warnings_not_included_in_labels() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    let labels = compute_fidelity(&[], &report);
    // CodeExecution is in warnings, not applied, so not in labels
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn fidelity_emulated_overrides_native_for_same_cap() {
    // If a cap appears in both native and report.applied, emulated wins
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::Streaming,
            strategy: EmulationStrategy::PostProcessing {
                detail: "buffer".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert!(matches!(
        labels[&Capability::Streaming],
        FidelityLabel::Emulated { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 10. Emulation performance overhead tracking (report completeness)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn report_is_empty_when_no_caps_requested() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

#[test]
fn report_not_empty_when_emulations_applied() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(!report.is_empty());
}

#[test]
fn report_not_empty_when_only_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(!report.is_empty());
}

#[test]
fn report_applied_count_matches_emulatable_input_count() {
    let engine = EmulationEngine::with_defaults();
    let emulatable = all_emulatable();
    let mut conv = user_only_conv();
    let report = engine.apply(&emulatable, &mut conv);
    assert_eq!(report.applied.len(), emulatable.len());
}

#[test]
fn report_warning_count_matches_disabled_input_count() {
    let engine = EmulationEngine::with_defaults();
    let disabled = all_disabled_by_default();
    let expected_count = disabled.len();
    let mut conv = user_only_conv();
    let report = engine.apply(&disabled, &mut conv);
    assert_eq!(report.warnings.len(), expected_count);
}

#[test]
fn report_serde_roundtrip_with_applied_and_warnings() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StopSequences,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "truncate".into(),
                },
            },
        ],
        warnings: vec!["Cannot emulate Streaming".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

// ════════════════════════════════════════════════════════════════════════
// 11. Config validation (strategy override correctness)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_override_enables_disabled_default() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::CodeExecution, emulate_code_execution());
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn config_override_disables_emulatable_default() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "policy".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn config_override_changes_strategy_type() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::StructuredOutputJsonSchema,
        emulate_structured_output(),
    );
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::StructuredOutputJsonSchema);
    // Override changes from PostProcessing to SystemPromptInjection
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn config_unset_capabilities_use_defaults() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::CodeExecution, emulate_code_execution());
    let engine = EmulationEngine::new(cfg);
    // ExtendedThinking not in config, should use default
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn config_strategies_map_uses_btreemap_ordering() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::ToolUse, emulate_code_execution());
    cfg.set(Capability::ExtendedThinking, emulate_extended_thinking());
    cfg.set(Capability::ImageInput, emulate_image_input());
    // BTreeMap should be deterministically ordered
    let json1 = serde_json::to_string(&cfg).unwrap();
    let json2 = serde_json::to_string(&cfg).unwrap();
    assert_eq!(json1, json2);
}

// ════════════════════════════════════════════════════════════════════════
// 12. Feature support detection (can_emulate)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn can_emulate_extended_thinking_true() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn can_emulate_structured_output_true() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn can_emulate_image_input_true() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn can_emulate_stop_sequences_true() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn cannot_emulate_code_execution() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn cannot_emulate_streaming() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn cannot_emulate_tool_use() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn cannot_emulate_any_disabled_default() {
    for cap in all_disabled_by_default() {
        assert!(!can_emulate(&cap), "expected cannot_emulate for {cap:?}");
    }
}

#[test]
fn can_emulate_all_emulatable() {
    for cap in all_emulatable() {
        assert!(can_emulate(&cap), "expected can_emulate for {cap:?}");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 13. Emulation fallback chains (check_missing + apply consistency)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn check_missing_and_apply_agree_on_applied_count() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::Streaming,
        Capability::ImageInput,
    ];
    let check = engine.check_missing(&caps);
    let mut conv = user_only_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
}

#[test]
fn check_missing_and_apply_agree_on_warning_count() {
    let engine = EmulationEngine::with_defaults();
    let caps = vec![Capability::CodeExecution, Capability::ToolUse];
    let check = engine.check_missing(&caps);
    let mut conv = user_only_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

#[test]
fn check_missing_does_not_mutate_anything() {
    let engine = EmulationEngine::with_defaults();
    // check_missing takes no conversation, just capabilities
    let report = engine.check_missing(&all_emulatable());
    assert_eq!(report.applied.len(), all_emulatable().len());
}

#[test]
fn fallback_config_override_changes_check_missing_result() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::CodeExecution, emulate_code_execution());
    let engine = EmulationEngine::new(cfg);
    let report = engine.check_missing(&[Capability::CodeExecution]);
    // Should now be applied, not warned
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn fallback_override_to_disabled_produces_warning() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "blocked".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

// ════════════════════════════════════════════════════════════════════════
// 14. Integration with capability negotiation (fidelity computation)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn compute_fidelity_all_native() {
    let native = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::CodeExecution,
    ];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 3);
    for label in labels.values() {
        assert_eq!(*label, FidelityLabel::Native);
    }
}

#[test]
fn compute_fidelity_all_emulated() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&all_emulatable(), &mut conv);
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), all_emulatable().len());
    for label in labels.values() {
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }
}

#[test]
fn compute_fidelity_native_plus_emulated_no_overlap() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn compute_fidelity_emulated_wins_over_native_on_collision() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "override".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[Capability::ExtendedThinking], &report);
    // Emulated should overwrite the native entry
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 15. Factory function tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn factory_emulate_structured_output_is_system_prompt() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn factory_emulate_structured_output_mentions_json() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_structured_output() {
        assert!(prompt.contains("JSON"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn factory_emulate_code_execution_is_system_prompt() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn factory_emulate_extended_thinking_is_system_prompt() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn factory_emulate_image_input_is_system_prompt() {
    let s = emulate_image_input();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn factory_emulate_stop_sequences_is_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn factory_strategies_serde_roundtrip() {
    let strategies = vec![
        emulate_structured_output(),
        emulate_code_execution(),
        emulate_extended_thinking(),
        emulate_image_input(),
        emulate_stop_sequences(),
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, decoded);
    }
}

// ════════════════════════════════════════════════════════════════════════
// 16. System prompt injection mechanics
// ════════════════════════════════════════════════════════════════════════

#[test]
fn injection_appends_to_existing_system_message() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("You are helpful."));
    assert!(text.contains("Think step by step"));
}

#[test]
fn injection_creates_system_message_when_absent() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn injection_prepends_system_message_at_position_zero() {
    let mut conv = user_only_conv();
    assert_eq!(conv.messages[0].role, IrRole::User);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn injection_preserves_multi_turn_structure() {
    let mut conv = multi_turn_conv();
    let orig_len = conv.messages.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    // No new messages added, just content appended to existing system
    assert_eq!(conv.messages.len(), orig_len);
}

#[test]
fn injection_on_user_only_conv_adds_one_message() {
    let mut conv = user_only_conv();
    let orig_len = conv.messages.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages.len(), orig_len + 1);
}

// ════════════════════════════════════════════════════════════════════════
// 17. Strategy serde roundtrips
// ════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_system_prompt_serde_roundtrip() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "Be precise.".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn strategy_post_processing_serde_roundtrip() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate output".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn strategy_disabled_serde_roundtrip() {
    let s = EmulationStrategy::Disabled {
        reason: "not safe".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn strategy_serde_tag_is_type() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let val: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert!(val.get("type").is_some());
    assert_eq!(val["type"], "system_prompt_injection");
}

#[test]
fn strategy_post_processing_tag() {
    let s = EmulationStrategy::PostProcessing {
        detail: "test".into(),
    };
    let val: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert_eq!(val["type"], "post_processing");
}

#[test]
fn strategy_disabled_tag() {
    let s = EmulationStrategy::Disabled {
        reason: "test".into(),
    };
    let val: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert_eq!(val["type"], "disabled");
}

// ════════════════════════════════════════════════════════════════════════
// 18. EmulationEntry and EmulationReport deep tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn entry_clone_equality() {
    let entry = EmulationEntry {
        capability: Capability::ExtendedThinking,
        strategy: EmulationStrategy::SystemPromptInjection {
            prompt: "test".into(),
        },
    };
    assert_eq!(entry, entry.clone());
}

#[test]
fn entry_serde_roundtrip() {
    let entry = EmulationEntry {
        capability: Capability::ImageInput,
        strategy: emulate_image_input(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let decoded: EmulationEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn report_clone_equality() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec!["test".into()],
    };
    assert_eq!(report, report.clone());
}

#[test]
fn report_serde_roundtrip_empty() {
    let report = EmulationReport::default();
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

// ════════════════════════════════════════════════════════════════════════
// 19. Free function apply_emulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn free_fn_with_default_config() {
    let cfg = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&cfg, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn free_fn_with_override_config() {
    let mut cfg = EmulationConfig::new();
    cfg.set(Capability::CodeExecution, emulate_code_execution());
    let mut conv = user_only_conv();
    let report = apply_emulation(&cfg, &[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(conv.system_message().is_some());
}

#[test]
fn free_fn_empty_caps_noop() {
    let cfg = EmulationConfig::new();
    let original = user_only_conv();
    let mut conv = original.clone();
    let report = apply_emulation(&cfg, &[], &mut conv);
    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn free_fn_matches_engine_behavior() {
    let cfg = EmulationConfig::new();
    let caps = [Capability::ExtendedThinking, Capability::CodeExecution];

    let mut conv1 = user_only_conv();
    let r1 = apply_emulation(&cfg, &caps, &mut conv1);

    let mut conv2 = user_only_conv();
    let engine = EmulationEngine::new(cfg.clone());
    let r2 = engine.apply(&caps, &mut conv2);

    assert_eq!(r1.applied.len(), r2.applied.len());
    assert_eq!(r1.warnings.len(), r2.warnings.len());
    assert_eq!(conv1, conv2);
}

// ════════════════════════════════════════════════════════════════════════
// 20. Edge cases and corner scenarios
// ════════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_gets_system_message_on_injection() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn conversation_with_only_assistant_gets_system_prepended() {
    let mut conv =
        IrConversation::new().push(IrMessage::text(IrRole::Assistant, "I am the assistant."));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
}

#[test]
fn system_prompt_with_content_blocks_gets_text_appended() {
    let sys_msg = IrMessage::text(IrRole::System, "Original.");
    let mut conv = IrConversation::new()
        .push(sys_msg)
        .push(IrMessage::text(IrRole::User, "Hi"));
    let before = conv.messages[0].content.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    // Injection appends a content block to the existing system message
    assert_eq!(conv.messages[0].content.len(), before + 1);
}

#[test]
fn all_caps_at_once_no_panic() {
    let mut caps = all_emulatable();
    caps.extend(all_disabled_by_default());
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len() + report.warnings.len(), caps.len());
}

#[test]
fn duplicate_capabilities_in_input_produce_duplicate_entries() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
}

#[test]
fn duplicate_disabled_caps_produce_duplicate_warnings() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_only_conv();
    let report = engine.apply(
        &[Capability::CodeExecution, Capability::CodeExecution],
        &mut conv,
    );
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn config_with_all_emulatable_overridden_to_disabled() {
    let mut cfg = EmulationConfig::new();
    for cap in all_emulatable() {
        cfg.set(
            cap,
            EmulationStrategy::Disabled {
                reason: "blocked by policy".into(),
            },
        );
    }
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_only_conv();
    let report = engine.apply(&all_emulatable(), &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), all_emulatable().len());
}

#[test]
fn config_with_all_disabled_overridden_to_emulate() {
    let mut cfg = EmulationConfig::new();
    for cap in all_disabled_by_default() {
        cfg.set(
            cap.clone(),
            EmulationStrategy::SystemPromptInjection {
                prompt: format!("Emulating {cap:?}"),
            },
        );
    }
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_only_conv();
    let report = engine.apply(&all_disabled_by_default(), &mut conv);
    assert_eq!(report.applied.len(), all_disabled_by_default().len());
    assert!(report.warnings.is_empty());
}
